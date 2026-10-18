use crate::ui::{
    components::{dismiss::use_dismiss, icons::{CloseIcon, LyricsIcon}},
    now_lyrics::{active_line, use_now_lyrics, LyricsState, TranslationState},
    player::use_player,
};
use dioxus::prelude::*;

/// What the black display says when there is no line to show: the words are missing, not the player broken.
/// `(first line, second line)` for a two-line display.
fn display_status(state: &LyricsState) -> Option<(&'static str, &'static str)> {
    match state {
        LyricsState::Missing { instrumental: true } => Some(("♪ Instrumental", "no words, just sound")),
        LyricsState::Missing { instrumental: false } => Some(("No lyrics on file", "just the music")),
        LyricsState::Failed => Some(("Lyrics unreachable", "check your connection")),
        LyricsState::Found(info) if info.synced.is_empty() => Some(("Lyrics aren't timed", "open the panel to read")),
        _ => None,
    }
}

/// The line being sung, for the player bar's black display. Its own component
/// so the ~20 position updates a second re-render only this one line.
#[component]
pub fn DisplayLyric(class: Option<String>, with_next: Option<bool>) -> Element {
    let class = class.unwrap_or_else(|| "dev-display__lyric".to_string());
    let player = use_player();
    let lyrics = use_now_lyrics().state();
    let state = lyrics.read();
    if let Some((first, second)) = display_status(&state) {
        return rsx! {
            div { class: "{class} lyric--status", "{first}" }
            if with_next.unwrap_or(false) {
                div { class: "{class} {class}--next lyric--status", "{second}" }
            }
        };
    }
    let LyricsState::Found(info) = &*state else { return rsx! {} };
    let pos_ms = ((player.position)() * 1000.0) as i64;
    let active = active_line(&info.synced, pos_ms);
    let text = active.map(|i| info.synced[i].text.as_str()).unwrap_or("");
    // The dim second line of a two-line display: the English for the line
    // being sung when there is one (and it is switched on), else the line
    // after it (skipping blank gaps).
    let now = use_now_lyrics();
    let translation = now.translation();
    let translation = translation.read();
    let english = match (&*translation, active) {
        (TranslationState::Ready(v), Some(i)) if now.show_translation() => v.get(i).map(|s| s.as_str()).filter(|s| !s.is_empty()),
        _ => None,
    };
    let next = if with_next.unwrap_or(false) {
        english.unwrap_or_else(|| {
            let from = active.map_or(0, |i| i + 1);
            info.synced.get(from..).and_then(|rest| rest.iter().find(|l| !l.text.is_empty())).map(|l| l.text.as_str()).unwrap_or("")
        })
    } else {
        ""
    };
    // A line too long for its box scrolls across it, paced to fit the time
    // before the next line starts, instead of being cut off.
    let dur_ms = active
        .and_then(|i| info.synced.get(i + 1).map(|n| n.ms - info.synced[i].ms))
        .map(|d| ((d as f64) * 0.8) as i64)
        .unwrap_or(5000)
        .clamp(1500, 12000);
    let dom_id = format!("lyric-line-{class}");
    rsx! {
        // Keyed by the text so each new line mounts fresh and can fade in.
        div {
            key: "{text}",
            id: "{dom_id}",
            class: "{class}",
            style: "--dur: {dur_ms}ms",
            title: "{text}",
            onmounted: {
                let dom_id = dom_id.clone();
                move |_| {
                    let dom_id = dom_id.clone();
                    spawn(async move {
                        document::eval(&format!(
                            "const el = document.getElementById('{dom_id}'); if (el) {{ const shift = el.scrollWidth - el.clientWidth; if (shift > 2) {{ el.style.setProperty('--shift', shift + 'px'); el.classList.add('lyric--long'); }} }}"
                        ));
                    });
                }
            },
            "{text}"
        }
        if with_next.unwrap_or(false) {
            div { class: "{class} {class}--next", "{next}" }
        }
    }
}

/// Translation controls for a lyrics header: ask for a translation, then
/// switch the English lines on and off.
#[component]
pub fn TranslationControls() -> Element {
    let player = use_player();
    let now = use_now_lyrics();
    let state = now.translation().read().clone();
    // Translating is an AI feature: with it off, only an already saved translation can be shown or hidden.
    let ai_on = crate::ui::settings::use_features().lyrics_ai;
    match state {
        TranslationState::Available | TranslationState::Failed(_) if !ai_on => rsx! {},
        TranslationState::Translating if !ai_on => rsx! {},
        TranslationState::NotNeeded => rsx! {},
        TranslationState::Available => rsx! {
            button {
                class: "dev-tag",
                title: "Translate these lyrics to English with the AI model (saved afterwards)",
                onclick: move |_| now.translate(player),
                "[AI] Translate"
            }
        },
        TranslationState::Translating => rsx! {
            button { class: "dev-tag", disabled: true, "Translating…" }
        },
        TranslationState::Failed(e) => rsx! {
            button { class: "dev-tag", title: "{e}", onclick: move |_| now.translate(player), "Retry translate" }
        },
        TranslationState::Ready(_) => rsx! {
            button {
                class: "dev-tag",
                title: "Show the English under each line",
                "aria-pressed": "{now.show_translation()}",
                onclick: move |_| now.toggle_translation(),
                "English"
            }
        },
    }
}

/// A button and the popover it opens above the bar: the full lyrics, with
/// the current line lit and kept in view when they are timed.
#[component]
pub fn LyricsPanel() -> Element {
    let mut open = use_signal(|| false);
    let dom_id = use_dismiss(open);
    rsx! {
        div { id: "{dom_id}", class: "player-bar__lyrics",
            button {
                onclick: move |_| open.set(!open()),
                title: "Lyrics",
                "aria-label": "Lyrics",
                "aria-expanded": "{open()}",
                class: "dev-tag",
                LyricsIcon { size: 14 }
            }
            if open() {
                div { class: "lyrics-panel",
                    div { class: "lyrics-panel__head",
                        span { class: "section-label", "Lyrics" }
                        TranslationControls {}
                        button {
                            class: "dev-tag",
                            title: "Close",
                            "aria-label": "Close lyrics",
                            onclick: move |_| open.set(false),
                            CloseIcon { size: 12 }
                        }
                    }
                    LyricsBody {}
                }
            }
        }
    }
}

#[component]
pub fn LyricsBody() -> Element {
    let player = use_player();
    let lyrics = use_now_lyrics().state();
    let state = lyrics.read().clone();
    let now = use_now_lyrics();
    let english: Vec<String> = match (&*now.translation().read(), now.show_translation()) {
        (TranslationState::Ready(v), true) => v.clone(),
        _ => Vec::new(),
    };

    match state {
        LyricsState::Idle | LyricsState::Loading => rsx! { p { class: "lyrics-panel__note", "Looking up lyrics…" } },
        LyricsState::Missing { instrumental: true } => rsx! { p { class: "lyrics-panel__note", "Instrumental - no words here, just the music." } },
        LyricsState::Missing { .. } => rsx! { p { class: "lyrics-panel__note", "No lyrics on file for this track - the music speaks for itself." } },
        LyricsState::Failed => rsx! { p { class: "lyrics-panel__note", "Couldn't reach the lyrics service. Close and reopen to try again later." } },
        LyricsState::Found(info) if info.synced.is_empty() => {
            let text = info.lyrics.unwrap_or_default();
            rsx! {
                p { class: "lyrics-panel__note", "No timing available for this track, so it can't follow along." }
                div { class: "lyrics-panel__list",
                    for (i, l) in text.lines().enumerate() {
                        div { key: "{i}", class: "lyrics-panel__line lyrics-panel__line--plain",
                            "{l}"
                            if let Some(en) = english.get(i).filter(|e| !e.is_empty()) {
                                span { class: "lyrics-panel__trans", "{en}" }
                            }
                        }
                    }
                }
            }
        }
        LyricsState::Found(info) => {
            let lines = info.synced;
            // Notifies only when the current line changes, not on every tick.
            let active = use_memo({
                let lines = lines.clone();
                move || active_line(&lines, ((player.position)() * 1000.0) as i64)
            });
            // Keep the current line centred. A new line eases into place; showing
            // or hiding the English changes every row's height, so then jump
            // straight back instead of crawling there.
            let mut last = use_signal(|| None::<usize>);
            let now = use_now_lyrics();
            use_effect(move || {
                let _ = now.show_translation();
                let current = active();
                if current.is_some() {
                    let behavior = if *last.peek() == current { "auto" } else { "smooth" };
                    last.set(current);
                    document::eval(&format!("document.getElementById('lyric-active')?.scrollIntoView({{ block: 'center', behavior: '{behavior}' }})"));
                }
            });
            rsx! {
                div { class: "lyrics-panel__list",
                    for (i, l) in lines.iter().enumerate() {
                        button {
                            key: "{i}",
                            id: if active() == Some(i) { "lyric-active" } else { "" },
                            class: if active() == Some(i) { "lyrics-panel__line lyrics-panel__line--active" } else { "lyrics-panel__line" },
                            title: "Jump to this line",
                            onclick: {
                                let ms = l.ms;
                                move |_| player.seek_to(ms as f64 / 1000.0)
                            },
                            if l.text.is_empty() { "♪" } else { "{l.text}" }
                            if let Some(en) = english.get(i).filter(|e| !e.is_empty()) {
                                span { class: "lyrics-panel__trans", "{en}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
