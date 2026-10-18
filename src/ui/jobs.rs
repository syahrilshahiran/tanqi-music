//! One queue for every background operation (bulk audio and lyrics
//! analysis).
//! Jobs run one at a time, in order: they contend for the network, the
//! user's single local Ollama model and the CPU.

use super::api;
use app_lib::lyrics_analyzer::LyricsTrack;
use app_lib::models::{MissingCover, Track, TrackSummary};
use dioxus::prelude::*;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_finished(self) -> bool {
        matches!(self, JobStatus::Done | JobStatus::Failed | JobStatus::Cancelled)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobKind {
    Audio,
    /// One Track's audio analysis, asked for from its analysis window (separate from the bulk one, so the Library button only reflects its own job).
    AudioTrack,
    Lyrics,
    Covers,
    /// Fetching artist photos, separate from album covers so each button only reflects its own job.
    ArtistPictures,
    /// One track's lyrics translation, asked for from the lyrics modal or panel.
    Translation,
    /// One track's lyrics deep dive, asked for from the lyrics modal.
    DeepDive,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub label: String,
    pub status: JobStatus,
    /// 0-1 where the work can report it; None means indeterminate.
    pub progress: Option<f64>,
    pub detail: Option<String>,
    pub error: Option<String>,
    pub finished_at: Option<std::time::Instant>,
    /// Where clicking this job leads (see `ui::targets`).
    pub target: String,
}

/// Handed to a job's own work.
#[derive(Clone, Copy)]
pub struct JobControls {
    id: Signal<String>,
    jobs: Signal<Vec<Job>>,
    cancelled: Signal<HashSet<String>>,
}

impl JobControls {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.peek().contains(&*self.id.peek())
    }

    pub fn report(&self, progress: Option<f64>, detail: Option<String>) {
        let id = self.id.peek().clone();
        let mut jobs = self.jobs;
        for job in jobs.write().iter_mut() {
            if job.id == id {
                job.progress = progress;
                job.detail = detail.clone();
            }
        }
    }
}

type JobFuture = Pin<Box<dyn Future<Output = Result<(), String>>>>;

#[derive(Clone)]
pub struct JobSpec {
    pub kind: JobKind,
    pub label: String,
    pub run: Rc<dyn Fn(JobControls) -> JobFuture>,
    /// Body of the completion notification; None means "no notification".
    /// Argument is whether the job was cancelled.
    pub notify_on_done: Option<Rc<dyn Fn(bool) -> Option<String>>>,
    /// Where clicking this job's notification leads (see `ui::targets`); None = the page its kind belongs to.
    pub opens: Option<String>,
}

impl JobSpec {
    pub fn new<F, Fut>(kind: JobKind, label: impl Into<String>, run: F) -> Self
    where
        F: Fn(JobControls) -> Fut + 'static,
        Fut: Future<Output = Result<(), String>> + 'static,
    {
        Self { kind, label: label.into(), run: Rc::new(move |c| Box::pin(run(c))), notify_on_done: None, opens: None }
    }

    /// Clicking this job's notification opens `target` (e.g. `eq:<track id>`).
    pub fn opens(mut self, target: impl Into<String>) -> Self {
        self.opens = Some(target.into());
        self
    }

    /// The target of its notification: the one set with `opens`, else the page for its kind.
    pub fn target(&self) -> String {
        self.opens.clone().unwrap_or_else(|| {
            match self.kind {
                JobKind::Audio | JobKind::AudioTrack => "eq",
                JobKind::Lyrics | JobKind::Translation | JobKind::DeepDive => "lyrics",
                JobKind::Covers | JobKind::ArtistPictures => "library",
            }
            .to_string()
        })
    }

    pub fn notify(mut self, f: impl Fn(bool) -> Option<String> + 'static) -> Self {
        self.notify_on_done = Some(Rc::new(f));
        self
    }
}

#[derive(Clone, Copy)]
pub struct Jobs {
    pub jobs: Signal<Vec<Job>>,
    queue: Signal<VecDeque<(String, JobSpec)>>,
    cancelled: Signal<HashSet<String>>,
    running: Signal<bool>,
    counter: Signal<u64>,
}

pub fn use_jobs_provider() -> Jobs {
    let jobs = use_context_provider(|| Jobs {
        jobs: Signal::new(Vec::new()),
        queue: Signal::new(VecDeque::new()),
        cancelled: Signal::new(HashSet::new()),
        running: Signal::new(false),
        counter: Signal::new(0),
    });

    // Finished jobs stay in the list until the user dismisses them (the ×
    // on a row, or "Clear finished"), so a result is never missed while away.
    jobs
}

pub fn use_jobs() -> Jobs {
    use_context::<Jobs>()
}

impl Jobs {

    fn patch(&self, id: &str, f: impl FnOnce(&mut Job)) {
        let mut jobs = self.jobs;
        let mut w = jobs.write();
        if let Some(job) = w.iter_mut().find(|j| j.id == id) {
            f(job);
        }
    }

    pub fn enqueue(&self, spec: JobSpec) -> String {
        self.enqueue_all(vec![spec]).remove(0)
    }

    pub fn enqueue_all(&self, specs: Vec<JobSpec>) -> Vec<String> {
        let mut jobs = self.jobs;
        let mut queue = self.queue;
        let mut counter = self.counter;
        let mut ids = Vec::new();
        for spec in specs {
            let n = *counter.peek() + 1;
            counter.set(n);
            let id = format!("job-{n}");
            jobs.write().push(Job {
                id: id.clone(),
                kind: spec.kind,
                label: spec.label.clone(),
                status: JobStatus::Queued,
                progress: None,
                detail: None,
                error: None,
                finished_at: None,
                target: spec.target(),
            });
            queue.write().push_back((id.clone(), spec));
            ids.push(id);
        }
        self.drain();
        ids
    }

    fn drain(&self) {
        if *self.running.peek() {
            return;
        }
        let this = *self;
        let mut running = self.running;
        running.set(true);
        // On the root scope, not the caller's: `enqueue` is usually called from
        // a modal or a page, and a task spawned there dies when that closes,
        // which would leave its job showing "Running" forever and the queue
        // stuck.
        dioxus::core::spawn_forever(async move {
            loop {
                let mut q = this.queue;
                let next = q.write().pop_front();
                let Some((id, spec)) = next else { break };

                // Cancelled while still queued - never start it.
                if this.cancelled.peek().contains(&id) {
                    this.patch(&id, |j| {
                        j.status = JobStatus::Cancelled;
                        j.finished_at = Some(std::time::Instant::now());
                    });
                    continue;
                }

                this.patch(&id, |j| j.status = JobStatus::Running);
                let controls = JobControls { id: Signal::new(id.clone()), jobs: this.jobs, cancelled: this.cancelled };
                let result = (spec.run)(controls).await;
                let cancelled = this.cancelled.peek().contains(&id);
                match result {
                    Ok(()) => {
                        this.patch(&id, |j| {
                            j.status = if cancelled { JobStatus::Cancelled } else { JobStatus::Done };
                            j.detail = None;
                            j.finished_at = Some(std::time::Instant::now());
                        });
                        let body = match &spec.notify_on_done {
                            Some(f) => f(cancelled),
                            None if cancelled => None,
                            None => Some(format!("{} finished", spec.label)),
                        };
                        if let Some(body) = body {
                            let _ = api::send_notification("Tanqi".into(), body, Some(spec.target())).await;
                        }
                    }
                    Err(e) => {
                        this.patch(&id, |j| {
                            j.status = JobStatus::Failed;
                            j.error = Some(e);
                            j.detail = None;
                            j.finished_at = Some(std::time::Instant::now());
                        });
                        let _ = api::send_notification("Tanqi".into(), format!("{} failed", spec.label), Some(spec.target())).await;
                    }
                }
                let mut c = this.cancelled;
                c.write().remove(&id);
            }
            let mut running = this.running;
            running.set(false);
        });
    }

    /// Drops a queued job outright, or asks a running one to stop at its
    /// next checkpoint.
    pub fn cancel(&self, id: &str) {
        let mut cancelled = self.cancelled;
        cancelled.write().insert(id.to_string());
        self.patch(id, |j| {
            if j.status == JobStatus::Queued {
                j.status = JobStatus::Cancelled;
                j.finished_at = Some(std::time::Instant::now());
            }
        });
        let mut queue = self.queue;
        queue.write().retain(|(qid, _)| qid != id);
    }

    pub fn cancel_all(&self) {
        let ids: Vec<String> = self.jobs.peek().iter().filter(|j| !j.status.is_finished()).map(|j| j.id.clone()).collect();
        let mut cancelled = self.cancelled;
        for id in &ids {
            cancelled.write().insert(id.clone());
        }
        let mut jobs = self.jobs;
        for j in jobs.write().iter_mut() {
            if j.status == JobStatus::Queued {
                j.status = JobStatus::Cancelled;
                j.finished_at = Some(std::time::Instant::now());
            }
        }
        let mut queue = self.queue;
        queue.write().clear();
    }

    pub fn dismiss(&self, id: &str) {
        let mut jobs = self.jobs;
        jobs.write().retain(|j| j.id != id);
    }

    pub fn dismiss_finished(&self) {
        let mut jobs = self.jobs;
        jobs.write().retain(|j| !j.status.is_finished());
    }
}

// ---- Job builders ----

/// How one item of a batch turned out.
enum Outcome {
    Done,
    Skipped,
    Failed,
}

#[derive(Clone, Copy, Default)]
struct Counts {
    done: usize,
    skipped: usize,
    failed: usize,
}

fn tally(done: usize, skipped: usize, failed: usize, cancelled: bool) -> String {
    let mut bits = vec![format!("{done} done")];
    if skipped > 0 {
        bits.push(format!("{skipped} skipped"));
    }
    if failed > 0 {
        bits.push(format!("{failed} failed"));
    }
    format!("{}{}", bits.join(", "), if cancelled { " (stopped early)" } else { "" })
}

/// A job that runs `step` over every item in order. Owns the loop: the
/// cancel check, the progress reports and the outcome counts. Each job
/// supplies only what one item does and how the result is worded.
fn batch_job<T, Fut>(
    kind: JobKind,
    label: String,
    items: Vec<T>,
    title: impl Fn(&T) -> String + 'static,
    step: impl Fn(T) -> Fut + 'static,
    summary: impl Fn(Counts, bool) -> String + 'static,
) -> JobSpec
where
    T: Clone + 'static,
    Fut: Future<Output = Outcome> + 'static,
{
    let counts = Rc::new(RefCell::new(Counts::default()));
    let for_notify = counts.clone();
    let n = items.len();
    let (title, step) = (Rc::new(title), Rc::new(step));
    JobSpec::new(kind, label, move |c| {
        let (items, counts, title, step) = (items.clone(), counts.clone(), title.clone(), step.clone());
        async move {
            let mut tally = Counts::default();
            for (i, item) in items.into_iter().enumerate() {
                // Checked between items: one already in flight can't be aborted.
                if c.is_cancelled() {
                    break;
                }
                let name = title(&item);
                c.report(Some(i as f64 / n as f64), Some(name.clone()));
                match step(item).await {
                    Outcome::Done => tally.done += 1,
                    Outcome::Skipped => tally.skipped += 1,
                    Outcome::Failed => tally.failed += 1,
                }
                c.report(Some((i + 1) as f64 / n as f64), Some(name));
            }
            *counts.borrow_mut() = tally;
            Ok(())
        }
    })
    .notify(move |cancelled| Some(summary(*for_notify.borrow(), cancelled)))
}

pub fn fetch_covers_job(albums: Vec<MissingCover>) -> JobSpec {
    let n = albums.len();
    batch_job(
        JobKind::Covers,
        format!("Fetch covers · {n} albums"),
        albums,
        |a| format!("{} - {}", a.artist, a.album),
        |a| async move {
            match api::fetch_album_cover(a.artist, a.album, a.ids).await {
                Ok(true) => Outcome::Done,
                Ok(false) => Outcome::Skipped,
                Err(_) => Outcome::Failed,
            }
        },
        |c, cancelled| {
            let mut s = format!("Covers: {} found, {} not on iTunes", c.done, c.skipped);
            if c.failed > 0 {
                s.push_str(&format!(", {} failed", c.failed));
            }
            if cancelled {
                s.push_str(" (stopped early)");
            }
            s
        },
    )
}

/// Looks up each artist's picture on YouTube Music. An artist with no exact
/// match is skipped, not failed: the Artists tab just keeps showing an album
/// cover for them.
pub fn fetch_artist_pictures_job(artists: Vec<String>) -> JobSpec {
    let n = artists.len();
    batch_job(
        JobKind::ArtistPictures,
        format!("Artist pictures · {n} artists"),
        artists,
        |name| name.clone(),
        |name| async move {
            match api::fetch_artist_picture(name).await {
                Ok(true) => Outcome::Done,
                Ok(false) => Outcome::Skipped,
                Err(_) => Outcome::Failed,
            }
        },
        |c, cancelled| {
            let mut s = format!("Artist pictures: {} found, {} not matched", c.done, c.skipped);
            if c.failed > 0 {
                s.push_str(&format!(", {} failed", c.failed));
            }
            if cancelled {
                s.push_str(" (stopped early)");
            }
            s
        },
    )
}

/// Audio-only, not lyrics: lyrics analysis goes through the local Ollama
/// model and is far heavier per track, so it stays a separate job.
pub fn bulk_audio_analysis_job(tracks: Vec<Track>) -> JobSpec {
    let n = tracks.len();
    batch_job(
        JobKind::Audio,
        format!("Analyze audio · {n} tracks"),
        tracks,
        |t| t.title.clone(),
        |t| async move {
            // force=false: anything already analyzed resolves from cache.
            match api::analyze_audio(t.id, t.title, t.artist, t.thumbnail_url, false).await {
                Ok(_) => Outcome::Done,
                Err(_) => Outcome::Failed,
            }
        },
        |c, cancelled| format!("Audio analysis: {}", tally(c.done, 0, c.failed, cancelled)),
    )
}

/// Analyzes (or re-analyzes, with `force`) one Track's audio and saves it, as
/// a queued job: the window that asked can be closed meanwhile, the job waits
/// its turn behind other work, and the saved analysis is there next time.
pub fn audio_analysis_job(track: TrackSummary, force: bool) -> JobSpec {
    let title = track.title.clone();
    let track_id = track.id.clone();
    let done_title = title.clone();
    JobSpec::new(JobKind::AudioTrack, format!("{title} · audio analysis"), move |c| {
        let t = track.clone();
        async move {
            c.report(None, Some("Measuring bass, mid, treble, tempo and key".to_string()));
            api::analyze_audio(t.id, t.title, t.artist, t.thumbnail_url, force).await.map(|_| ())
        }
    })
    .opens(format!("eq:{track_id}"))
    .notify(move |cancelled| (!cancelled).then(|| format!("Audio analysis ready: {done_title}")))
}

/// A track with no lyrics to analyze (none found, empty, instrumental) is
/// skipped; the analyzer decides that, not this job.
pub fn bulk_lyrics_analysis_job(tracks: Vec<Track>) -> JobSpec {
    let n = tracks.len();
    batch_job(
        JobKind::Lyrics,
        format!("Analyze lyrics · {n} tracks"),
        tracks,
        |t| t.title.clone(),
        |t| async move {
            let track = LyricsTrack::from(&t);
            let step = app_lib::with_analyzer(move |a| async move {
                a.analyze_track(app_lib::app::metadata(), &track, false).await
            })
            .await;
            match step {
                Ok(Some(_)) => Outcome::Done,
                Ok(None) => Outcome::Skipped,
                Err(_) => Outcome::Failed,
            }
        },
        |c, cancelled| format!("Lyrics analysis: {}", tally(c.done, c.skipped, c.failed, cancelled)),
    )
}

/// Translates one track's lyrics with the AI model and saves the result
/// (the same saved translation the lyrics modal and panel read). It runs in
/// the queue, so the modal that asked can be closed while it works: the job
/// is app-wide, and the translation is simply there next time it is looked
/// up.
pub fn translate_lyrics_job(track: LyricsTrack, lyrics: String, force: bool) -> JobSpec {
    let title = track.title.clone();
    let track_id = track.id.clone();
    let done_title = title.clone();
    // The song comes first so it still reads when the row is narrow; the
    // model takes roughly 0.7 s a line, in batches of 30.
    let lines = lyrics.lines().filter(|l| !l.trim().is_empty()).count();
    let detail = format!("Translating {lines} lines · about {} s", (lines as f64 * 0.7).ceil() as i64);
    JobSpec::new(JobKind::Translation, format!("{title} · translation"), move |c| {
        let (track, lyrics, detail) = (track.clone(), lyrics.clone(), detail.clone());
        async move {
            c.report(None, Some(detail));
            app_lib::with_analyzer(move |a| async move { a.translate(&track, &lyrics, force).await }).await.map(|_| ())
        }
    })
    .opens(format!("lyrics:{track_id}"))
    .notify(move |cancelled| (!cancelled).then(|| format!("Translation ready: {done_title}")))
}

/// Runs (or re-runs, with `force`) one track's lyrics deep dive with the
/// local model and saves it, as a queued job: the lyrics modal that asked can
/// be closed meanwhile, and the saved analysis is there next time it opens.
pub fn deep_dive_job(track: LyricsTrack, lyrics: String, force: bool) -> JobSpec {
    let title = track.title.clone();
    let track_id = track.id.clone();
    let done_title = title.clone();
    JobSpec::new(JobKind::DeepDive, format!("{title} · deep dive"), move |c| {
        let (track, lyrics) = (track.clone(), lyrics.clone());
        async move {
            c.report(None, Some("Reading the lyrics with the AI model".to_string()));
            app_lib::with_analyzer(move |a| async move { a.analyze(&track, &lyrics, force).await }).await.map(|_| ())
        }
    })
    .opens(format!("lyrics:{track_id}"))
    .notify(move |cancelled| (!cancelled).then(|| format!("Deep dive ready: {done_title}")))
}

/// Waits (polling the queue) until the job `id` has finished or been
/// cleared, and returns how it ended: `Ok` when it completed, else its error.
/// Runs on the root scope so it outlives whatever asked.
pub async fn wait_for_job(jobs: Jobs, id: String) -> Result<(), String> {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let state = jobs.jobs.peek().iter().find(|j| j.id == id).map(|j| (j.status, j.error.clone()));
        match state {
            Some((JobStatus::Done, _)) => return Ok(()),
            Some((JobStatus::Failed, e)) => return Err(e.unwrap_or_else(|| "the job failed".to_string())),
            Some((JobStatus::Cancelled, _)) => return Err("cancelled".to_string()),
            // Cleared by the user once finished; the result is saved if it worked.
            None => return Ok(()),
            _ => {}
        }
    }
}
