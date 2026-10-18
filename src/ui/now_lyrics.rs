//! The lyrics of the track now playing, looked up whenever it changes, for the
//! player bar's display line and lyrics panel, plus their English
//! translation when the lyrics are in another script.

use super::{
    api,
    jobs::{translate_lyrics_job, use_jobs, wait_for_job, Jobs},
    player::Player,
};
use app_lib::lyrics_analyzer::LyricsTrack;
use app_lib::models::{LyricsInfo, LyricsTranslation, SyncedLine};
use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
pub enum LyricsState {
    /// Nothing is playing.
    Idle,
    Loading,
    Found(LyricsInfo),
    /// The service has no lyrics for it, or it is an instrumental.
    Missing { instrumental: bool },
    /// The lookup itself failed (offline, service down).
    Failed,
}

#[derive(Clone, PartialEq)]
pub enum TranslationState {
    /// The lyrics are already readable as English (Latin script), or there are none.
    NotNeeded,
    /// Non-English and not translated yet; the user can ask for it.
    Available,
    Translating,
    /// One English line per lyric line (blank where there is nothing to add),
    /// in the same order as the lyrics being shown.
    Ready(Vec<String>),
    Failed(String),
}

#[derive(Clone, Copy)]
struct ShowTranslation(Signal<bool>);

#[derive(Clone, Copy)]
pub struct NowLyrics {
    state: Signal<LyricsState>,
    translation: Signal<TranslationState>,
    show: Signal<bool>,
    jobs: Jobs,
}

impl NowLyrics {
    pub fn state(&self) -> Signal<LyricsState> {
        self.state
    }

    pub fn translation(&self) -> Signal<TranslationState> {
        self.translation
    }

    /// Whether the English line is shown under each lyric line.
    pub fn show_translation(&self) -> bool {
        (self.show)()
    }

    pub fn toggle_translation(&self) {
        let mut show = self.show;
        let now = *show.peek();
        show.set(!now);
    }

    /// Queues the playing track's lyrics for translation by the local model
    /// (the same saved translation the lyrics modal makes) and shows the
    /// result when it lands. It is a background job, so the panel can be
    /// closed meanwhile.
    pub fn translate(&self, player: Player) {
        let (mut translation, state, jobs) = (self.translation, self.state, self.jobs);
        let Some(track) = player.current_track() else { return };
        let LyricsState::Found(info) = state.peek().clone() else { return };
        translation.set(TranslationState::Translating);
        let queued = jobs.enqueue(translate_lyrics_job(LyricsTrack::from(&track), lyrics_text(&info), false));
        // On the root scope, so it outlives the panel that asked.
        dioxus::core::spawn_forever(async move {
            let result = wait_for_job(jobs, queued).await;
            // Still the same track?
            if player.current_track().map(|t| t.id) != Some(track.id.clone()) {
                return;
            }
            translation.set(match result {
                Ok(()) => match api::get_lyrics_translation(track.id.clone()).await.ok().flatten() {
                    Some(t) => TranslationState::Ready(align_translation(&shown_lines(&info), &t)),
                    None => TranslationState::Failed("the translation wasn't saved".to_string()),
                },
                Err(e) => TranslationState::Failed(e),
            });
        });
    }
}

/// The text the translation is numbered against: the plain lyrics, or the
/// timed lines joined when there are no plain ones.
fn lyrics_text(info: &LyricsInfo) -> String {
    match info.lyrics.as_deref().filter(|l| !l.trim().is_empty()) {
        Some(plain) => plain.to_string(),
        None => info.synced.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n"),
    }
}

/// The lines as the panel shows them: timed ones when there are any.
fn shown_lines(info: &LyricsInfo) -> Vec<String> {
    if info.synced.is_empty() {
        info.lyrics.as_deref().unwrap_or("").lines().map(|l| l.trim().to_string()).collect()
    } else {
        info.synced.iter().map(|l| l.text.clone()).collect()
    }
}

/// Korean (Hangul), Japanese (Hiragana/Katakana) and Chinese/Japanese (CJK
/// ideographs): the scripts a translation is worth offering for.
pub fn contains_non_latin_script(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c as u32,
            0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF)
    })
}

/// Pairs each shown line with its English. The translation numbers only the
/// non-blank lyric lines, and a timed version of the same lyrics repeats or
/// splits some, so lines are matched by text, walking forward through the
/// translation. A line whose English is the line itself gets nothing.
pub fn align_translation(shown: &[String], t: &LyricsTranslation) -> Vec<String> {
    let norm = |s: &str| s.trim().to_lowercase();
    let mut next = 0usize;
    shown
        .iter()
        .map(|line| {
            let key = norm(line);
            if key.is_empty() {
                return String::new();
            }
            let found = (next..t.lines.len()).find(|&k| norm(&t.lines[k]) == key);
            let Some(k) = found else { return String::new() };
            next = k + 1;
            let english = t.translations.get(k).map(|s| s.trim()).unwrap_or("");
            if norm(english) == key { String::new() } else { english.to_string() }
        })
        .collect()
}

/// Provide once, above the shell, after the player.
pub fn use_now_lyrics_provider(player: Player) -> NowLyrics {
    let mut state = use_context_provider(|| Signal::new(LyricsState::Idle));
    let mut translation = use_context_provider(|| Signal::new(TranslationState::NotNeeded));
    let show = use_context_provider(|| ShowTranslation(Signal::new(true))).0;
    // Queue edits re-run the resource below; only a different track refetches.
    let mut last_id = use_signal(|| None::<String>);
    use_resource(move || async move {
        let track = player.current_track();
        let id = track.as_ref().map(|t| t.id.clone());
        if *last_id.peek() == id {
            return;
        }
        last_id.set(id);
        // The lookup runs as its own task: this resource re-runs on every queue edit, and a re-run cancels
        // whatever it was awaiting - which, with `last_id` already set, would leave the state at Loading for good.
        spawn(async move {
            translation.set(TranslationState::NotNeeded);
            let Some(track) = track else {
                state.set(LyricsState::Idle);
                return;
            };
            state.set(LyricsState::Loading);
            let id = track.id.clone();
            let found = app_lib::with_metadata(move |m| async move {
                m.lyrics(&track.title, &track.artist, track.duration_ms).await
            })
            .await;
            // The track may have changed while the lookup ran.
            if last_id.peek().as_deref() != Some(id.as_str()) {
                return;
            }
            let next = match found {
                Ok(Some(info)) if !info.synced.is_empty() || info.lyrics.as_deref().is_some_and(|l| !l.trim().is_empty()) => {
                    LyricsState::Found(info)
                }
                Ok(Some(info)) => LyricsState::Missing { instrumental: info.instrumental },
                Ok(None) => LyricsState::Missing { instrumental: false },
                Err(_) => LyricsState::Failed,
            };
            if let LyricsState::Found(info) = &next {
                if contains_non_latin_script(&lyrics_text(info)) {
                    // A translation saved earlier shows straight away; a new one
                    // waits for the user to ask (opening a track never runs the model).
                    let saved = api::get_lyrics_translation(id.clone()).await.ok().flatten();
                    if last_id.peek().as_deref() != Some(id.as_str()) {
                        return;
                    }
                    translation.set(match saved {
                        Some(t) => TranslationState::Ready(align_translation(&shown_lines(info), &t)),
                        None => TranslationState::Available,
                    });
                }
            }
            state.set(next);
        });
    });
    NowLyrics { state, translation, show, jobs: use_jobs() }
}

pub fn use_now_lyrics() -> NowLyrics {
    NowLyrics {
        state: use_context::<Signal<LyricsState>>(),
        translation: use_context::<Signal<TranslationState>>(),
        show: use_context::<ShowTranslation>().0,
        jobs: use_jobs(),
    }
}

/// Index of the line being sung at `pos_ms`: the last one that has started.
pub fn active_line(lines: &[SyncedLine], pos_ms: i64) -> Option<usize> {
    lines.partition_point(|l| l.ms <= pos_ms).checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ms: i64) -> SyncedLine {
        SyncedLine { ms, text: String::new() }
    }

    fn translation(lines: &[&str], english: &[&str]) -> LyricsTranslation {
        LyricsTranslation {
            track_id: "t".into(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            translations: english.iter().map(|s| s.to_string()).collect(),
            model: "m".into(),
            created_at: String::new(),
        }
    }

    #[test]
    fn active_line_is_the_last_one_that_has_started() {
        let lines = [line(1000), line(5000), line(9000)];
        assert_eq!(active_line(&lines, 0), None);
        assert_eq!(active_line(&lines, 1000), Some(0));
        assert_eq!(active_line(&lines, 6000), Some(1));
        assert_eq!(active_line(&lines, 999_999), Some(2));
        assert_eq!(active_line(&[], 5), None);
    }

    #[test]
    fn translation_lines_pair_up_with_timed_lines_by_text() {
        let t = translation(&["안녕", "Hello there", "사랑해"], &["Hi", "Hello there", "I love you"]);
        // The timed version has a blank gap and a repeated chorus line.
        let shown: Vec<String> = ["안녕", "", "Hello there", "사랑해", "사랑해"].iter().map(|s| s.to_string()).collect();
        assert_eq!(align_translation(&shown, &t), vec!["Hi", "", "", "I love you", ""]);
    }

    #[test]
    fn detects_scripts_worth_translating() {
        assert!(contains_non_latin_script("Don't mind, 별별별"));
        assert!(!contains_non_latin_script("Is this the real life?"));
    }
}
