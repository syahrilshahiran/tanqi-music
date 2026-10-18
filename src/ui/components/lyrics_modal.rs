use crate::ui::{
    api,
    components::icons::CloseIcon,
    jobs::{deep_dive_job, translate_lyrics_job, use_jobs, wait_for_job},
    settings::use_settings,
    util::time_ago,
};
use app_lib::lyrics_analyzer::LyricsTrack;
use app_lib::models::{LyricsAnalysis, LyricsAnnotation, LyricsTranslation, TrackSummary};
use dioxus::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Escape-to-close for a modal (the TSX's `window` keydown listener). The
/// listener is removed again when the component unmounts. Nested modals each
/// register their own, so Escape closes the whole stack like before.
pub fn use_escape_to_close(on_close: EventHandler<()>) {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = use_hook(|| NEXT_ID.fetch_add(1, Ordering::Relaxed));
    use_future(move || async move {
        let mut ev = document::eval(&format!(
            r#"
            window.__tqEsc = window.__tqEsc || {{}};
            const h = (e) => {{ if (e.key === "Escape") dioxus.send(1); }};
            window.__tqEsc[{id}] = h;
            window.addEventListener("keydown", h);
            await new Promise(() => {{}});
            "#
        ));
        while ev.recv::<i32>().await.is_ok() {
            on_close.call(());
        }
    });
    use_drop(move || {
        document::eval(&format!(
            "if (window.__tqEsc && window.__tqEsc[{id}]) {{ window.removeEventListener('keydown', window.__tqEsc[{id}]); delete window.__tqEsc[{id}]; }}"
        ));
    });
}

/// Korean (Hangul), Japanese (Hiragana/Katakana), and Chinese/Japanese
/// (CJK ideographs) - the scripts a hover-translation is worth offering for.
/// A lyric that's already in English (or any other Latin-script language)
/// never matches, so the translate button simply doesn't appear for it.
fn contains_non_latin_script(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c as u32,
            0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF)
    })
}

#[derive(Clone, PartialEq)]
struct LineGroup {
    key: String,
    lines: Vec<(usize, String)>,
    annotated: bool,
}

/// Same splitting rule the backend uses to number lines for the model
/// (blank lines dropped) - used here just to show plain lyrics before any
/// analysis has run; once `analysis` loads, its own `lines` take over as the
/// authoritative numbering.
fn plain_numbered_lines(lyrics: &str) -> Vec<String> {
    lyrics.split('\n').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// Groups consecutive lines by whether an annotation covers them, purely
/// to decide the highlight - the annotations themselves are listed in full
/// in the right-hand column, no click needed to see what they say. Before
/// analysis exists, every line is its own unannotated group.
fn group_lines(display_lines: &[String], annotations: &[LyricsAnnotation]) -> Vec<LineGroup> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < display_lines.len() {
        let line_number = (i + 1) as i64;
        if let Some(a) = annotations.iter().find(|a| line_number >= a.start_line && line_number <= a.end_line) {
            let mut lines = Vec::new();
            while i < display_lines.len() && (i + 1) as i64 <= a.end_line {
                lines.push((i + 1, display_lines[i].clone()));
                i += 1;
            }
            result.push(LineGroup { key: format!("{}-{}", a.start_line, a.end_line), lines, annotated: true });
        } else {
            result.push(LineGroup { key: format!("line-{}", i + 1), lines: vec![(i + 1, display_lines[i].clone())], annotated: false });
            i += 1;
        }
    }
    result
}

fn scroll_into_view(dom_id: &str) {
    let id = serde_json::to_string(dom_id).unwrap_or_default();
    document::eval(&format!("document.getElementById({id})?.scrollIntoView({{ behavior: 'smooth', block: 'center' }})"));
}

#[component]
pub fn LyricsModal(
    track: TrackSummary,
    lyrics: String,
    /// Pre-loaded analysis (e.g. reopened from the Topics tab's Music
    /// section) - skips the initial fetch and opens straight into the
    /// analysis view instead of requiring another "[AI] Deep dive" click.
    initial_analysis: Option<LyricsAnalysis>,
    on_close: EventHandler<()>,
) -> Element {
    let mut show_analysis = use_signal(|| initial_analysis.is_some());
    let mut analysis = use_signal(|| initial_analysis.clone());
    let mut loading = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let settings = use_settings();
    let ai_model = settings.read().ai_model.clone();
    let ai_model = if ai_model.is_empty() { None } else { Some(ai_model) };
    let ai_provider = settings.read().ai_provider;
    let ai_remote = ai_provider.needs_key();
    // The deep dive and the translation are AI features; the lyrics themselves are always there.
    let ai_on = crate::ui::settings::use_features().lyrics_ai;

    // Translation is line-by-line English, offered only for non-Latin-script
    // lyrics (Korean/Japanese/Chinese) - unlike the deep dive, a cached
    // translation loads silently on open (it's cheap and near-instant to
    // read), but a fresh one still requires the button so opening this modal
    // never triggers Ollama on its own.
    let is_non_english = use_memo(use_reactive!(|lyrics| contains_non_latin_script(&lyrics)));
    let mut translation = use_signal(|| None::<LyricsTranslation>);
    let mut translating = use_signal(|| false);
    let mut translate_error = use_signal(|| None::<String>);

    {
        let id = track.id.clone();
        use_future(move || {
            let id = id.clone();
            async move {
                if !is_non_english() {
                    return;
                }
                if let Ok(Some(t)) = api::get_lyrics_translation(id).await {
                    translation.set(Some(t));
                }
            }
        });
    }

    let jobs = use_jobs();
    // False once this modal has been closed; its signals are gone with it.
    let open_flag = use_hook(|| std::rc::Rc::new(std::cell::Cell::new(true)));
    use_drop({
        let open_flag = open_flag.clone();
        move || open_flag.set(false)
    });

    let start_translate = {
        let open_flag = open_flag.clone();
        let track = track.clone();
        let lyrics = lyrics.clone();
        use_callback(move |force: bool| {
            let (track, lyrics) = (track.clone(), lyrics.clone());
            translating.set(true);
            translate_error.set(None);
            // Queued like the other background jobs, so closing this window
            // doesn't stop it (and it waits its turn behind any other Ollama
            // work). The waiting runs on the root scope for the same reason.
            let id = track.id.clone();
            let open_flag = open_flag.clone();
            let queued = jobs.enqueue(translate_lyrics_job(LyricsTrack::from(&track), lyrics, force));
            dioxus::core::spawn_forever(async move {
                let result = wait_for_job(jobs, queued).await;
                // This window may be gone by now; only touch it if it is still open.
                if open_flag.get() {
                    match result {
                        Ok(()) => {
                            if let Ok(Some(t)) = api::get_lyrics_translation(id).await {
                                translation.set(Some(t));
                            }
                        }
                        Err(e) => translate_error.set(Some(e)),
                    }
                    translating.set(false);
                }
            });
        })
    };

    // Hover tooltip for a translated line - position is tracked in the lyrics
    // column's own local coordinates (mouse position minus the column's
    // scroll-aware bounding box) so the tooltip scrolls along with the line
    // it belongs to instead of staying fixed to the viewport.
    let mut lyrics_column = use_signal(|| None::<std::rc::Rc<MountedData>>);
    let mut translate_tooltip = use_signal(|| None::<(usize, f64, f64)>);
    let mut hover_line = use_signal(|| None::<usize>);
    let mut show_translate_tooltip = move |client_x: f64, client_y: f64, line_number: usize| {
        hover_line.set(Some(line_number));
        let Some(container) = lyrics_column.peek().clone() else { return };
        spawn(async move {
            let (Ok(rect), Ok(scroll)) = (container.get_client_rect().await, container.get_scroll_offset().await) else { return };
            // The pointer may have left the line while the geometry was fetched.
            if *hover_line.peek() == Some(line_number) {
                translate_tooltip.set(Some((line_number, client_x - rect.origin.x + scroll.x, client_y - rect.origin.y + scroll.y)));
            }
        });
    };

    // Hovering an annotated group (in either column) highlights its match in
    // the other; clicking "pins" that highlight so it survives the mouse
    // moving away - clicking the same one again (or elsewhere) unpins it.
    let mut hovered_key = use_signal(|| None::<String>);
    let mut pinned_key = use_signal(|| None::<String>);
    let active_key = pinned_key().or_else(|| hovered_key());

    // Only the column you *didn't* click scrolls - the one you clicked is
    // already where your cursor is, so leave it put and bring its counterpart
    // into view instead of scrolling both (or the one you're looking at).
    let mut toggle_pin = move |key: String, from_lyrics: bool| {
        let next = if pinned_key.peek().as_deref() == Some(key.as_str()) { None } else { Some(key) };
        pinned_key.set(next.clone());
        if let Some(next) = next {
            scroll_into_view(&format!("{}-{next}", if from_lyrics { "lyr-ann" } else { "lyr-grp" }));
        }
    };

    let run = {
        let open_flag = open_flag.clone();
        let track = track.clone();
        let lyrics = lyrics.clone();
        use_callback(move |force: bool| {
            let (track, lyrics) = (track.clone(), lyrics.clone());
            loading.set(true);
            error.set(None);
            // A queued job, like the translation: closing this window doesn't
            // stop it, and it waits its turn behind other local-model work.
            let id = track.id.clone();
            let open_flag = open_flag.clone();
            let queued = jobs.enqueue(deep_dive_job(LyricsTrack::from(&track), lyrics, force));
            dioxus::core::spawn_forever(async move {
                let result = wait_for_job(jobs, queued).await;
                // This window may be gone by now; only touch it if it is still open.
                if open_flag.get() {
                    match result {
                        Ok(()) => match api::get_lyrics_analysis(id).await {
                            Ok(Some(saved)) => analysis.set(Some(saved)),
                            Ok(None) => error.set(Some("the deep dive finished but wasn't saved".to_string())),
                            Err(e) => error.set(Some(e)),
                        },
                        Err(e) => error.set(Some(e)),
                    }
                    loading.set(false);
                }
            });
        })
    };

    // Lyrics render immediately for free. "Deep dive" only opens the side
    // card (showing a saved analysis if there is one); the AI itself starts
    // from the card's own Run button, so opening this never triggers Ollama.
    let open_deep_dive = {
        let id = track.id.clone();
        use_callback(move |_: ()| {
            let id = id.clone();
            show_analysis.set(true);
            error.set(None);
            spawn(async move {
                match api::get_lyrics_analysis(id).await {
                    Ok(Some(cached)) => analysis.set(Some(cached)),
                    Ok(None) => {}
                    Err(e) => error.set(Some(e)),
                }
            });
        })
    };

    use_escape_to_close(on_close);
    // Phone: Lyrics and Analysis are tabs of one full-screen column instead of two squeezed side by side.
    let mut tab_analysis = use_signal(|| false);

    let current = analysis();
    let display_lines = match &current {
        Some(a) => a.lines.clone(),
        None => plain_numbered_lines(&lyrics),
    };
    let annotations = current.as_ref().map(|a| a.annotations.clone()).unwrap_or_default();
    let groups = group_lines(&display_lines, &annotations);
    let sorted_annotations = {
        let mut v = annotations;
        v.sort_by_key(|a| a.start_line);
        v
    };
    let translation_now = translation();
    let tooltip_now = translate_tooltip();
    let is_loading = loading();
    let show_an = show_analysis();

    rsx! {
        div {
            class: "modal-scrim lyrics-modal",
            // A click outside the panel does not close it (it holds a long read and an analysis in progress):
            // only the close button or Escape does. It is opened from within SongInfoModal's own backdrop, so
            // the click must not bubble up and close that one either.
            onclick: move |e| e.stop_propagation(),
            div {
                class: if show_an { "lyrics-modal__panel lyrics-modal__panel--wide" } else { "lyrics-modal__panel" },
                onclick: move |e| e.stop_propagation(),
                div { class: "lyrics-modal__header",
                    div { class: "lyrics-modal__heading",
                        p { class: "lyrics-modal__kicker", "Lyrics" }
                        p { class: "lyrics-modal__title",
                            "{track.title} "
                            span { class: "lyrics-modal__artist", "· {track.artist}" }
                        }
                    }
                    button {
                        onclick: move |_| on_close.call(()),
                        class: "lyrics-modal__btn btn--icon",
                        title: "Close",
                        "aria-label": "Close",
                        CloseIcon { size: 18 }
                    }
                }

                if show_an {
                    div { class: "lyrics-modal__tabs", role: "tablist",
                        button {
                            class: "lyrics-modal__tab",
                            "aria-pressed": "{!tab_analysis()}",
                            onclick: move |_| tab_analysis.set(false),
                            "Lyrics"
                        }
                        button {
                            class: "lyrics-modal__tab",
                            "aria-pressed": "{tab_analysis()}",
                            onclick: move |_| tab_analysis.set(true),
                            "Analysis"
                        }
                    }
                }

                div { class: "lyrics-modal__content",
                    if let Some(err) = error() {
                        div { class: "alert-error lyrics-modal__error",
                            "{err}"
                        }
                    }

                    div {
                        class: match (show_an, tab_analysis()) {
                            (true, true) => "lyrics-modal__columns lyrics-modal__columns--split lyrics-modal__columns--analysis",
                            (true, false) => "lyrics-modal__columns lyrics-modal__columns--split lyrics-modal__columns--lyrics",
                            _ => "lyrics-modal__columns",
                        },
                        div {
                            onmounted: move |e| lyrics_column.set(Some(e.data())),
                            class: "lyrics-modal__lyrics",
                            if is_non_english() && ai_on {
                                div { class: "lyrics-modal__translate-bar",
                                    span { class: "truncate",
                                        if translation_now.is_some() {
                                            "Hover a dotted line for its English translation"
                                        } else if let Some(e) = translate_error() {
                                            "{e}"
                                        } else {
                                            "Non-English lyrics detected"
                                        }
                                    }
                                    button {
                                        onclick: move |_| start_translate.call(translation.peek().is_some()),
                                        disabled: translating(),
                                        class: "lyrics-modal__btn lyrics-modal__btn--compact",
                                        if translating() {
                                            "Queued - you can close this"
                                        } else if translation_now.is_some() {
                                            "Re-translate"
                                        } else {
                                            "[AI] Translate"
                                        }
                                    }
                                }
                            }
                            div { class: "lyrics-modal__lines",
                                for group in groups.iter() {
                                    {
                                        let annotated = group.annotated;
                                        let is_active = annotated && active_key.as_deref() == Some(group.key.as_str());
                                        let (k_enter, k_click) = (group.key.clone(), group.key.clone());
                                        rsx! {
                                            div {
                                                key: "{group.key}",
                                                id: "lyr-grp-{group.key}",
                                                onmouseenter: move |_| {
                                                    if annotated {
                                                        hovered_key.set(Some(k_enter.clone()));
                                                    }
                                                },
                                                onmouseleave: move |_| hovered_key.set(None),
                                                onclick: move |_| {
                                                    if annotated {
                                                        toggle_pin(k_click.clone(), true);
                                                    }
                                                },
                                                class: if !annotated {
                                                    "lyrics-modal__group"
                                                } else if is_active {
                                                    "lyrics-modal__mark lyrics-modal__mark--active"
                                                } else {
                                                    "lyrics-modal__mark"
                                                },
                                                for (number, text) in group.lines.iter() {
                                                    {
                                                        let number = *number;
                                                        let translated = translation_now
                                                            .as_ref()
                                                            .and_then(|t| t.translations.get(number - 1))
                                                            .map(|s| s.trim().to_string())
                                                            .unwrap_or_default();
                                                        let has_translation = !translated.is_empty() && translated != text.trim();
                                                        rsx! {
                                                            p {
                                                                key: "{number}",
                                                                class: "lyrics-modal__line",
                                                                onmousemove: move |e| {
                                                                    if has_translation {
                                                                        let c = e.client_coordinates();
                                                                        show_translate_tooltip(c.x, c.y, number);
                                                                    }
                                                                },
                                                                onmouseleave: move |_| {
                                                                    if has_translation {
                                                                        if *hover_line.peek() == Some(number) {
                                                                            hover_line.set(None);
                                                                        }
                                                                        let cur = *translate_tooltip.peek();
                                                                        if matches!(cur, Some((n, _, _)) if n == number) {
                                                                            translate_tooltip.set(None);
                                                                        }
                                                                    }
                                                                },
                                                                span { class: "lyrics-modal__num", "{number}" }
                                                                span {
                                                                    class: if has_translation { "lyrics-modal__text--translated" } else { "" },
                                                                    "{text}"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if let (Some((line, x, y)), Some(t)) = (tooltip_now, translation_now.as_ref()) {
                                if let Some(text) = t.translations.get(line - 1).filter(|s| !s.is_empty()) {
                                    div {
                                        class: "lyrics-modal__tooltip",
                                        style: "left: {x + 14.0}px; top: {y + 14.0}px",
                                        "{text}"
                                    }
                                }
                            }
                        }

                        if show_an {
                            div { class: "lyrics-modal__analysis",
                                if is_loading {
                                    div { class: "lyrics-modal__loading",
                                        span {
                                            "Queued for {ai_model.clone().unwrap_or_else(|| \"the AI model\".to_string())}"
                                            " - you can close this; it keeps running and you'll get a notification"
                                        }
                                        span { class: "lyrics-modal__bounce", "aria-hidden": "true",
                                            span {}
                                            span {}
                                            span {}
                                        }
                                    }
                                } else if current.is_none() {
                                    div { class: "lyrics-modal__notice",
                                        h3 { class: "section-label lyrics-modal__heading-label", "[AI] Deep dive" }
                                        p { class: "lyrics-modal__vibe",
                                            if ai_remote {
                                                "This sends this song's lyrics to {ai_provider.label()} ({ai_model.clone().unwrap_or_default()}), which writes a vibe summary, topics, entities and notes on the lines worth explaining. Only the title, artist and lyrics are sent, and they leave your machine."
                                            } else {
                                                "This reads the lyrics with {ai_model.clone().unwrap_or_else(|| \"your local model\".to_string())} on your own machine and writes a vibe summary, topics, entities and notes on the lines worth explaining. Nothing leaves your own machine or network."
                                            }
                                        }
                                        p { class: "lyrics-modal__none",
                                            if ai_remote {
                                                "It can take a few seconds to a minute, and it may use your provider's credit or quota. The result is saved, so it only needs to run once per track."
                                            } else {
                                                "It can take a minute or more, and it uses your GPU or CPU while it runs. The result is saved, so it only needs to run once per track."
                                            }
                                        }
                                        button {
                                            onclick: move |_| run.call(false),
                                            class: "lyrics-modal__btn lyrics-modal__btn--wide lyrics-modal__run",
                                            "[AI] Run deep dive"
                                        }
                                    }
                                } else if let Some(a) = current.as_ref() {
                                    h3 { class: "section-label lyrics-modal__heading-label", "Vibe" }
                                    p { class: "lyrics-modal__vibe", "{a.vibe_summary}" }

                                    if !a.topics.is_empty() {
                                        div { class: "lyrics-modal__block",
                                            h3 { class: "section-label lyrics-modal__heading-label lyrics-modal__heading-label--md", "Topics" }
                                            div { class: "lyrics-modal__tags",
                                                for topic in a.topics.iter() {
                                                    span { key: "{topic}", class: "lyrics-modal__tag", "{topic}" }
                                                }
                                            }
                                        }
                                    }

                                    if !a.entities.is_empty() {
                                        div { class: "lyrics-modal__block lyrics-modal__block--lg",
                                            h3 { class: "section-label lyrics-modal__heading-label lyrics-modal__heading-label--md", "Entities" }
                                            div { class: "lyrics-modal__tags",
                                                for entity in a.entities.iter() {
                                                    span { key: "{entity}", class: "lyrics-modal__tag", "{entity}" }
                                                }
                                            }
                                        }
                                    }

                                    h3 { class: "section-label lyrics-modal__heading-label lyrics-modal__heading-label--lg", "Annotations" }
                                    if sorted_annotations.is_empty() {
                                        p { class: "lyrics-modal__none", "Nothing flagged as worth explaining for this track." }
                                    } else {
                                        div { class: "lyrics-modal__annotations",
                                            for ann in sorted_annotations.iter() {
                                                {
                                                    let key = format!("{}-{}", ann.start_line, ann.end_line);
                                                    let is_active = active_key.as_deref() == Some(key.as_str());
                                                    let (k_enter, k_click) = (key.clone(), key.clone());
                                                    let label = if ann.start_line == ann.end_line {
                                                        format!("Line {}", ann.start_line)
                                                    } else {
                                                        format!("Lines {}-{}", ann.start_line, ann.end_line)
                                                    };
                                                    rsx! {
                                                        div {
                                                            key: "{key}",
                                                            id: "lyr-ann-{key}",
                                                            onmouseenter: move |_| hovered_key.set(Some(k_enter.clone())),
                                                            onmouseleave: move |_| hovered_key.set(None),
                                                            onclick: move |_| toggle_pin(k_click.clone(), false),
                                                            class: if is_active { "lyrics-modal__mark lyrics-modal__mark--active" } else { "lyrics-modal__mark" },
                                                            p { class: "lyrics-modal__ann-label", "{label}" }
                                                            p { class: "lyrics-modal__ann-text", "{ann.meaning}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                div { class: "lyrics-modal__footer",
                    if let Some(a) = current.as_ref() {
                        span { class: "truncate", "{a.model} · {time_ago(&a.created_at)}" }
                        button {
                            onclick: move |_| run.call(true),
                            disabled: is_loading,
                            class: "lyrics-modal__btn lyrics-modal__btn--compact",
                            "Re-analyze"
                        }
                    } else if ai_on {
                        span { class: "truncate",
                            if let Some(m) = ai_model.as_ref() {
                                "Uses {m}"
                            }
                        }
                        if !show_an {
                            button {
                                onclick: move |_| open_deep_dive.call(()),
                                class: "lyrics-modal__btn lyrics-modal__btn--wide",
                                "[AI] Deep dive"
                            }
                        }
                    }
                }
            }
        }
    }
}
