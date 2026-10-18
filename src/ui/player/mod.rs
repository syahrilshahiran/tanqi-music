//! Player state: queue, shuffle/repeat, EQ, volume. The Web Audio graph and
//! gapless scheduling live in `assets/engine.js`; this side decides *what*
//! plays and tells the engine.

use super::api;
use super::suggestion::Suggested;
use super::util::file_url;
use app_lib::mpris::{MprisCommand, NowPlaying};
use app_lib::models::{AudioAnalysis, HeadphoneProfile, TrackSummary};
use dioxus::prelude::*;
use serde_json::{json, Value};

pub use super::correction::{parse_parametric_eq, CorrectionProfile};
mod eq;
mod media_session;
mod queue;
mod session;

pub use media_session::use_mpris;
use media_session::push_now_playing;
use super::engine::{cmd, volume_gain, waveform_from};
pub use super::play_queue::RepeatMode;
use super::play_queue::{build_shuffle_order, resolve_next_index};

/// Starting-point frequencies for a never-analyzed track.
pub const EQ_FREQUENCIES: [f32; 10] = [31.0, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
const DEFAULT_EQ_Q: f32 = 1.4;

/// Loudness normalization for an analysis with no measured LUFS (made before it existed): how far its
/// `overall_db` may be pulled toward `TARGET_LEVEL_DBFS`, and the target itself (gated RMS dBFS).
const MAX_NORMALIZATION_DB: f32 = 8.0;
const TARGET_LEVEL_DBFS: f32 = -14.0;

const VOLUME_STORAGE_KEY: &str = "tanqi-volume";
/// The Play queue (ids and which one is current) and the position in the current Track,
/// kept so the app can pick up where it left off. The position changes every few seconds,
/// so it is stored apart from the (possibly long) queue.

/// One live tone-EQ band; both `freq` and the band count are freely
/// user-adjustable.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LiveEqBand {
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
}

fn default_eq_bands() -> Vec<LiveEqBand> {
    EQ_FREQUENCIES.iter().map(|&freq| LiveEqBand { freq, gain: 0.0, q: DEFAULT_EQ_Q }).collect()
}

/// A track's Suggested EQ at the chosen strength: the live bands and the preamp that goes with them.
fn suggested_from(a: &AudioAnalysis, strength: f32) -> (Vec<LiveEqBand>, f32) {
    let s = Suggested::of(a, strength);
    (s.triples().map(|(freq, gain, q)| LiveEqBand { freq, gain, q }).collect(), s.preamp)
}

fn norm_db(analysis: &Option<AudioAnalysis>) -> Option<f32> {
    analysis.as_ref().map(|a| match a.loudness_lufs {
        // Measured to BS.1770: toward the target LUFS, boosts held back by the track's true peak.
        Some(lufs) => app_lib::audio::normalisation_gain_db(lufs, a.true_peak_db),
        // An analysis from before loudness was measured: the old gated-RMS estimate.
        None => (TARGET_LEVEL_DBFS - a.overall_db).clamp(-MAX_NORMALIZATION_DB, MAX_NORMALIZATION_DB),
    })
}

#[derive(Clone, PartialEq)]
struct PendingLoad {
    token: u64,
    index: usize,
    analysis: Option<AudioAnalysis>,
    /// Where in the Track to start (non-zero when picking up a restored session).
    offset: f64,
}

#[derive(Clone, PartialEq)]
struct NextInfo {
    index: usize,
    id: String,
    analysis: Option<AudioAnalysis>,
}

#[derive(Clone, Copy)]
pub struct Player {
    pub queue: Signal<Vec<TrackSummary>>,
    pub current_index: Signal<Option<usize>>,
    pub is_playing: Signal<bool>,
    pub loading: Signal<bool>,
    pub error: Signal<Option<String>>,
    /// Updated ~20x/s while playing; read it only where it's displayed.
    pub position: Signal<f64>,
    pub duration: Signal<f64>,
    /// Normalized 0-1 peaks across the whole track (empty while nothing's
    /// loaded).
    pub waveform: Signal<Vec<f32>>,
    pub eq_enabled: Signal<bool>,
    /// How much of a track's Suggested EQ is applied, 0.0-1.0 (the Suggested EQ strength setting).
    pub eq_strength: Signal<f32>,
    /// The current track's cached EQ analysis; None = not analyzed yet.
    pub analysis: Signal<Option<AudioAnalysis>>,
    pub eq_bands: Signal<Vec<LiveEqBand>>,
    pub eq_preamp: Signal<f32>,
    pub correction_profile: Signal<Option<CorrectionProfile>>,
    pub correction_enabled: Signal<bool>,
    /// Headphone corrections saved by name, and the one in use (None: no correction).
    pub headphones: Signal<Vec<HeadphoneProfile>>,
    pub active_headphone: Signal<Option<i64>>,
    pub shuffle: Signal<bool>,
    pub repeat: Signal<RepeatMode>,
    pub volume: Signal<f32>,
    pub muted: Signal<bool>,
    // internals
    shuffle_order: Signal<Option<Vec<usize>>>,
    load_token: Signal<u64>,
    next_token: Signal<u64>,
    user_touched_eq: Signal<bool>,
    pending: Signal<Option<PendingLoad>>,
    /// Set after a restart restored the last session: the current Track is not loaded yet
    /// (nothing plays until the user presses play), and this is where in it to start.
    resume_at: Signal<Option<f64>>,
    next_info: Signal<Option<NextInfo>>,
    correction_structure: Signal<bool>,
    /// The app's shared settings, handed in by whoever provides the player.
    settings: Signal<app_lib::models::AppSettings>,
}

pub fn use_player_provider(settings: Signal<app_lib::models::AppSettings>) -> Player {
    let player = use_context_provider(|| Player {
        queue: Signal::new(vec![]),
        current_index: Signal::new(None),
        is_playing: Signal::new(false),
        loading: Signal::new(false),
        error: Signal::new(None),
        position: Signal::new(0.0),
        duration: Signal::new(0.0),
        waveform: Signal::new(vec![]),
        eq_enabled: Signal::new(true),
        eq_strength: Signal::new(app_lib::models::DEFAULT_EQ_STRENGTH_PERCENT as f32 / 100.0),
        analysis: Signal::new(None),
        eq_bands: Signal::new(default_eq_bands()),
        eq_preamp: Signal::new(0.0),
        correction_profile: Signal::new(None),
        correction_enabled: Signal::new(true),
        headphones: Signal::new(vec![]),
        active_headphone: Signal::new(None),
        shuffle: Signal::new(false),
        repeat: Signal::new(RepeatMode::Off),
        volume: Signal::new(1.0),
        muted: Signal::new(false),
        shuffle_order: Signal::new(None),
        load_token: Signal::new(0),
        next_token: Signal::new(0),
        user_touched_eq: Signal::new(false),
        pending: Signal::new(None),
        resume_at: Signal::new(None),
        next_info: Signal::new(None),
        correction_structure: Signal::new(true),
        settings,
    });

    // Engine -> Rust events (time ticks, loaded, promoted, ended...).
    use_future(move || async move {
        let mut ev = document::eval(
            "tqEngine.setEmitter((e) => dioxus.send(e)); await new Promise(() => {});",
        );
        while let Ok(v) = ev.recv::<Value>().await {
            player.on_event(v);
        }
    });

    // An audio analysis finished while its song was already playing: the song was loaded without
    // one, so give the player the new result now (suggested EQ and loudness) instead of waiting for
    // the next time the song is started.
    let jobs = super::jobs::use_jobs();
    use_effect(move || {
        let finished = jobs
            .jobs
            .read()
            .iter()
            .filter(|j| matches!(j.kind, super::jobs::JobKind::Audio | super::jobs::JobKind::AudioTrack) && j.status == super::jobs::JobStatus::Done)
            .count();
        if finished == 0 {
            return;
        }
        let Some(track) = player.current_track() else { return };
        if player.analysis.peek().is_some() || !player.audio_analysis_on() {
            return;
        }
        spawn(async move {
            if let Ok(Some(a)) = api::get_audio_analysis(track.id.clone()).await {
                // Still the same song, and still without one?
                if player.current_track().map(|t| t.id) == Some(track.id) && player.analysis.peek().is_none() {
                    let mut p = player;
                    p.analysis.set(Some(a.clone()));
                    player.seed_eq_from(&Some(a));
                }
            }
        });
    });

    // The EQ button starts as it was left (on the first time); the strength is the one chosen in Settings.
    use_future(move || async move {
        player.restore_eq_enabled().await;
        if let Ok(settings) = api::get_settings().await {
            let mut p = player;
            p.eq_strength.set(settings.eq_strength_percent as f32 / 100.0);
        }
    });

    // Saved headphone corrections, and the one that was in use last time.
    use_future(move || async move {
        let (Ok(list), Ok(active)) = (api::list_headphone_profiles().await, api::get_active_headphone().await) else { return };
        let mut p = player;
        p.headphones.set(list);
        player.apply_headphone(active);
    });

    // Restore the stored volume once.
    use_future(move || async move {
        let mut ev = document::eval(&format!(
            "try {{ dioxus.send(localStorage.getItem('{VOLUME_STORAGE_KEY}')) }} catch (e) {{ dioxus.send(null) }}"
        ));
        if let Ok(Some(raw)) = ev.recv::<Option<String>>().await {
            if let Ok(v) = raw.parse::<f32>() {
                let mut vol = player.volume;
                vol.set(v.clamp(0.0, 1.0));
            }
        }
    });

    // Pick up where the last session left off, paused. Saving waits until this has
    // looked, or the empty queue of a fresh start would erase what it is about to read.
    let mut restored = use_signal(|| false);
    use_future(move || async move {
        player.restore_last().await;
        restored.set(true);
    });

    // Remember the Play queue whenever it or the current Track changes, and the
    // position in the Track every few seconds while playing.
    use_effect(move || {
        let _ = ((player.queue)(), (player.current_index)());
        if restored() {
            player.persist_queue();
        }
    });
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if *player.is_playing.peek() {
                player.persist_position();
            }
        }
    });

    // Live-apply EQ (gain/freq/preamp/on-off, add/remove/reset) to the graph.
    use_effect(move || {
        let bands: Vec<Value> = (player.eq_bands)()
            .iter()
            .map(|b| json!({"freq": b.freq, "gain": b.gain, "q": b.q}))
            .collect();
        cmd("setEq", json!({"bands": bands, "preampDb": (player.eq_preamp)(), "enabled": (player.eq_enabled)()}));
    });

    // Volume / mute (setTargetAtTime in the engine avoids clicks), persisted.
    use_effect(move || {
        let (v, m) = ((player.volume)(), (player.muted)());
        cmd("setVolume", json!({"gain": volume_gain(v, m)}));
        document::eval(&format!("try {{ localStorage.setItem('{VOLUME_STORAGE_KEY}', '{v}') }} catch (e) {{}}"));
    });

    // Correction profile (imported headphone EQ).
    use_effect(move || {
        let enabled = (player.correction_enabled)();
        let profile = (player.correction_profile)();
        let structure = *player.correction_structure.peek();
        let js = profile.as_ref().map(|p| {
            json!({
                "preampDb": p.preamp_db,
                "bands": p.bands.iter().map(|b| json!({"type": b.kind.js(), "freq": b.freq, "gain": b.gain, "q": b.q})).collect::<Vec<_>>()
            })
        });
        cmd("setCorrection", json!({"profile": js, "enabled": enabled, "structure": structure}));
        let mut s = player.correction_structure;
        s.set(false);
    });

    player
}

pub fn use_player() -> Player {
    use_context::<Player>()
}

impl Player {
    // ---- derived state ----

    pub fn current_track(&self) -> Option<TrackSummary> {
        let idx = (self.current_index)()?;
        (self.queue)().get(idx).cloned()
    }

    pub fn get_position(&self) -> f64 {
        *self.position.peek()
    }

    // ---- events from the engine ----

    fn on_event(&self, v: Value) {
        let mut this = *self;
        match v["ev"].as_str().unwrap_or("") {
            "time" => {
                this.position.set(v["t"].as_f64().unwrap_or(0.0));
            }
            "loaded" => {
                let token = v["token"].as_u64().unwrap_or(0);
                let Some(p) = this.pending.peek().clone() else { return };
                if p.token != token || token != *this.load_token.peek() {
                    return;
                }
                this.pending.set(None);
                let duration = v["duration"].as_f64().unwrap_or(0.0);
                let waveform = waveform_from(&v["waveform"]);
                this.start_track(p, duration, waveform);
            }
            "loadError" => {
                let token = v["token"].as_u64().unwrap_or(0);
                if token == *this.load_token.peek() {
                    this.error.set(Some(v["message"].as_str().unwrap_or("playback failed").to_string()));
                    this.loading.set(false);
                }
            }
            "promoted" => {
                let key = v["key"].as_str().unwrap_or("").to_string();
                let Some(next) = this.next_info.peek().clone() else { return };
                if next.id != key {
                    return;
                }
                this.next_info.set(None);
                this.duration.set(v["duration"].as_f64().unwrap_or(0.0));
                this.waveform.set(waveform_from(&v["waveform"]));
                this.analysis.set(next.analysis.clone());
                this.position.set(0.0);
                this.current_index.set(Some(next.index));
                this.is_playing.set(true);
                this.seed_eq_from(&next.analysis);
                this.schedule_prefetch(next.index);
            }
            "nextMiss" => {
                if let (Some(idx), q) = (*this.current_index.peek(), this.queue.peek().clone()) {
                    if let Some(t) = q.get(idx) {
                        this.load_and_play(t.clone(), idx);
                    }
                }
            }
            "ended" => {
                this.is_playing.set(false);
                let d = *this.duration.peek();
                this.position.set(d);
                this.play_next();
            }
            _ => {}
        }
    }

    // ---- loading ----

    /// Resolves the engine URL for a local-file track and fetches its cached
    /// analysis.
    async fn resolve_source(track: &TrackSummary, with_analysis: bool) -> Result<(String, Option<AudioAnalysis>), String> {
        let Some(path) = app_lib::local_ref::decode(&track.id) else {
            return Err("only local files can be played".to_string());
        };
        // With Audio analysis switched off a track plays as it is: no suggestion, no levelling.
        let analysis = if with_analysis { api::get_audio_analysis(track.id.clone()).await.ok().flatten() } else { None };
        Ok((file_url(path), analysis))
    }

    fn load_and_play(&self, track: TrackSummary, index: usize) {
        self.load_and_play_at(track, index, 0.0);
    }

    fn load_and_play_at(&self, track: TrackSummary, index: usize, offset: f64) {
        let mut this = *self;
        let token = *this.load_token.peek() + 1;
        this.load_token.set(token);
        this.resume_at.set(None);
        this.loading.set(true);
        this.error.set(None);
        this.position.set(offset);
        this.duration.set(0.0);
        this.waveform.set(vec![]);
        this.analysis.set(None);
        this.pending.set(None);
        // Clean up whatever the previous track downloaded.
        cmd("stop", json!({}));
        this.is_playing.set(false);

        let with_analysis = self.audio_analysis_on();
        spawn(async move {
            match Self::resolve_source(&track, with_analysis).await {
                Err(e) => {
                    if token == *this.load_token.peek() {
                        this.error.set(Some(e));
                        this.loading.set(false);
                    }
                }
                Ok((url, analysis)) => {
                    if token != *this.load_token.peek() {
                        return;
                    }
                    this.pending.set(Some(PendingLoad { token, index, analysis, offset }));
                    cmd("load", json!({"token": token, "url": url}));
                }
            }
        });
    }

    fn seed_eq_from(&self, analysis: &Option<AudioAnalysis>) {
        let mut this = *self;
        if !*self.user_touched_eq.peek() {
            let (bands, preamp) = this.suggested_for(analysis);
            this.eq_bands.set(bands);
            this.eq_preamp.set(preamp);
        }
        cmd("setNorm", json!({"db": norm_db(analysis)}));
    }

    fn start_track(&self, p: PendingLoad, duration: f64, waveform: Vec<f32>) {
        let mut this = *self;
        this.duration.set(duration);
        this.waveform.set(waveform);
        this.analysis.set(p.analysis.clone());
        // EQ must reach the engine before the source starts.
        if !*this.user_touched_eq.peek() {
            let (bands, preamp) = this.suggested_for(&p.analysis);
            let js: Vec<Value> = bands.iter().map(|b| json!({"freq": b.freq, "gain": b.gain, "q": b.q})).collect();
            cmd("setEq", json!({"bands": js, "preampDb": preamp, "enabled": *this.eq_enabled.peek()}));
            this.eq_bands.set(bands);
            this.eq_preamp.set(preamp);
        }
        cmd("start", json!({"token": p.token, "normDb": norm_db(&p.analysis), "offset": p.offset}));
        this.is_playing.set(true);
        this.loading.set(false);
        this.schedule_prefetch(p.index);
    }

    /// Downloads/decodes the track after `index` ahead of time. A failure
    /// just means that transition isn't gapless.
    // ---- transport ----

    /// Starts playing `track`; `queue`, if given, becomes the play queue
    /// with `track` as the starting point.
    pub fn play_track(&self, track: TrackSummary, queue: Option<Vec<TrackSummary>>) {
        let mut this = *self;
        let next_queue = queue.unwrap_or_else(|| vec![track.clone()]);
        let idx = next_queue.iter().position(|t| t.id == track.id).unwrap_or(0);
        self.reset_prefetch();
        if this.shuffle_order.peek().is_some() {
            this.shuffle_order.set(Some(build_shuffle_order(next_queue.len(), idx)));
        }
        this.queue.set(next_queue);
        this.current_index.set(Some(idx));
        self.load_and_play(track, idx);
    }

    pub fn toggle_play_pause(&self) {
        let mut this = *self;
        // Read it out first: `peek()` holds a borrow that clearing it (in `load_and_play_at`) would trip over.
        let resume_at = *this.resume_at.peek();
        if *this.is_playing.peek() {
            cmd("pause", json!({}));
            this.is_playing.set(false);
            self.persist_position();
        } else if let Some(offset) = resume_at {
            // The restored session's Track is not loaded yet: load it and start where it left off.
            let index = *this.current_index.peek();
            if let (Some(track), Some(index)) = (self.current_track(), index) {
                self.load_and_play_at(track, index, offset);
            }
        } else if this.current_index.peek().is_some() {
            cmd("resume", json!({}));
            this.is_playing.set(true);
        }
    }

    pub fn play_next(&self) {
        let mut this = *self;
        let Some(idx) = *this.current_index.peek() else { return };
        let queue = this.queue.peek().clone();
        let repeat = *this.repeat.peek();

        // Repeat-one restarts the buffer that's already decoded.
        if repeat == RepeatMode::One {
            this.position.set(0.0);
            cmd("seek", json!({"t": 0}));
            if !*this.is_playing.peek() {
                cmd("resume", json!({}));
                this.is_playing.set(true);
            }
            return;
        }

        let Some(ni) = resolve_next_index(idx, queue.len(), self.shuffle_order_if_on_peek().as_ref(), repeat) else { return };
        let Some(next) = queue.get(ni).cloned() else { return };
        this.current_index.set(Some(ni));

        // Already prefetched: start it immediately, no re-download.
        let prefetched = this.next_info.peek().clone();
        if let Some(info) = prefetched {
            if info.id == next.id {
                this.next_info.set(Some(NextInfo { index: ni, ..info.clone() }));
                cmd("startNext", json!({"key": info.id, "normDb": norm_db(&info.analysis)}));
                return;
            }
        }
        self.load_and_play(next, ni);
    }

    pub fn play_previous(&self) {
        let mut this = *self;
        let Some(idx) = *this.current_index.peek() else { return };
        let queue = this.queue.peek().clone();
        let prev_idx = match self.shuffle_order_if_on_peek() {
            Some(order) if order.len() == queue.len() => {
                let pos = order.iter().position(|&i| i == idx).unwrap_or(0);
                if pos == 0 {
                    return;
                }
                order[pos - 1]
            }
            _ => {
                if idx == 0 {
                    return;
                }
                idx - 1
            }
        };
        let Some(prev) = queue.get(prev_idx).cloned() else { return };
        self.reset_prefetch();
        this.current_index.set(Some(prev_idx));
        self.load_and_play(prev, prev_idx);
    }

    pub fn seek_to(&self, seconds: f64) {
        let mut this = *self;
        this.position.set(seconds);
        if this.resume_at.peek().is_some() {
            this.resume_at.set(Some(seconds));
        }
        cmd("seek", json!({"t": seconds}));
        push_now_playing(this.now_playing());
    }

    /// Brackets a seek-bar drag so the engine's time ticks don't fight it.
    pub fn set_seeking(&self, seeking: bool) {
        cmd("seeking", json!({"on": seeking}));
    }

    pub fn set_volume(&self, v: f32) {
        let mut this = *self;
        this.volume.set(v.clamp(0.0, 1.0));
        // Moving the slider at all means "I want to hear this level".
        this.muted.set(false);
    }

    pub fn toggle_mute(&self) {
        let mut this = *self;
        let m = *this.muted.peek();
        this.muted.set(!m);
    }

    /// Stops playback and clears the queue entirely - hides the player bar.
    pub fn stop(&self) {
        let mut this = *self;
        let t = *this.load_token.peek() + 1;
        this.load_token.set(t);
        self.reset_prefetch();
        cmd("stop", json!({}));
        this.shuffle_order.set(None);
        this.queue.set(vec![]);
        this.current_index.set(None);
        this.resume_at.set(None);
        this.is_playing.set(false);
        this.loading.set(false);
        this.error.set(None);
        this.position.set(0.0);
        this.duration.set(0.0);
        this.waveform.set(vec![]);
        this.analysis.set(None);
    }
}
