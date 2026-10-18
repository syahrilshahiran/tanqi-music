use crate::ui::{api, settings::save_settings, settings::use_settings};
use app_lib::models::AppSettings;
use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
enum Phase {
    Checking,
    Hidden,
    Picking(AppSettings),
}

/// First-run welcome, mounted once at the root: asks for a music folder to
/// scan. Shows only when `onboarding_completed` is false AND the library is
/// still empty.
#[component]
pub fn OnboardingGate() -> Element {
    let mut phase = use_signal(|| Phase::Checking);
    let mut home = use_context::<crate::ui::HomeTick>().0;
    use_future(move || async move {
        let Ok(settings) = api::get_settings().await else {
            phase.set(Phase::Hidden);
            return;
        };
        if settings.onboarding_completed {
            phase.set(Phase::Hidden);
            return;
        }
        let tracks = app_lib::with_store(|s| s.track_count()).await.unwrap_or(0);
        if tracks > 0 {
            // A library that already exists predates the optional features: keep them on.
            let mut done = settings.clone();
            done.onboarding_completed = true;
            done.audio_analysis_enabled = true;
            done.lyrics_ai_enabled = true;
            let _ = api::set_settings(done).await;
            phase.set(Phase::Hidden);
            return;
        }
        phase.set(Phase::Picking(settings));
    });

    match phase() {
        Phase::Picking(initial) => rsx! { FolderPicker { initial, on_done: move |_| {
            phase.set(Phase::Hidden);
            // The Library page loaded before the first folder existed.
            home += 1;
        } } },
        _ => rsx! {},
    }
}

#[component]
fn FolderPicker(initial: AppSettings, on_done: EventHandler<()>) -> Element {
    let shared = use_settings();
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    // The optional features, ticked here and off until chosen: the main thing is a plain music player.
    let mut audio_on = use_signal(|| initial.audio_analysis_enabled);
    let mut lyrics_on = use_signal(|| initial.lyrics_ai_enabled);

    // Marks onboarding finished whether the user imported or skipped, so it
    // never reappears.
    let finish = {
        let initial = initial.clone();
        move || {
            let mut next = initial.clone();
            next.onboarding_completed = true;
            next.audio_analysis_enabled = *audio_on.peek();
            next.lyrics_ai_enabled = *lyrics_on.peek();
            spawn(async move {
                match save_settings(shared, next).await {
                    Ok(()) => on_done.call(()),
                    Err(e) => {
                        error.set(Some(e));
                        busy.set(false);
                    }
                }
            });
        }
    };

    let pick = {
        let finish = finish.clone();
        move |_| {
            busy.set(true);
            error.set(None);
            let finish = finish.clone();
            spawn(async move {
                match api::pick_local_audio_folder().await {
                    Ok(None) => busy.set(false), // cancelled
                    Ok(Some(folder)) => match app_lib::with_store(move |s| s.add_folder(&folder, &app_lib::scanner::FsScanner)).await {
                        Ok(_) => finish(),
                        Err(e) => {
                            error.set(Some(e));
                            busy.set(false);
                        }
                    },
                    Err(e) => {
                        error.set(Some(e));
                        busy.set(false);
                    }
                }
            });
        }
    };

    rsx! {
        div { class: "modal-scrim onboarding",
            div { class: "onboarding__dialog",
                div { class: "onboarding__top",
                    span { class: "onboarding__label", "TQ-1" }
                    div { class: "dev-display",
                        div { class: "dev-display__title",
                            if busy() { "Scanning…" } else { "No music yet" }
                        }
                        div { class: "dev-display__row",
                            span { class: "dev-display__artist",
                                if busy() { "Reading tags from your files" } else { "Waiting for a folder" }
                            }
                        }
                    }
                }
                // The tape strip: idle until a scan runs, then it sweeps.
                div { class: if busy() { "onboarding__tape onboarding__tape--busy" } else { "onboarding__tape" },
                    span { class: "onboarding__tape-head" }
                }
                h1 { class: "onboarding__title", "Welcome to Tanqi" }
                p { class: "onboarding__lead",
                    "Pick the folder your music lives in. It's scanned recursively and your library is built from the tags in your files."
                }
                p { class: "onboarding__extras-title", "Optional extras" }
                label { class: "onboarding__extra",
                    input {
                        r#type: "checkbox",
                        checked: audio_on(),
                        disabled: busy(),
                        onchange: move |e| audio_on.set(e.checked()),
                    }
                    span {
                        span { class: "onboarding__extra-name", "Audio analysis and suggested EQ" }
                        span { class: "onboarding__extra-hint", "Measures each song's loudness and tone, suggests an EQ and levels playback." }
                    }
                }
                label { class: "onboarding__extra",
                    input {
                        r#type: "checkbox",
                        checked: lyrics_on(),
                        disabled: busy(),
                        onchange: move |e| lyrics_on.set(e.checked()),
                    }
                    span {
                        span { class: "onboarding__extra-name", "AI lyrics analysis" }
                        span { class: "onboarding__extra-hint", "Synced lyrics are looked up online either way. This adds the [AI] deep dive and translation, which use your own Ollama, or a provider you give a key to in Settings. You can change these in Settings." }
                    }
                }
                if let Some(e) = error() {
                    p { class: "onboarding__error", "{e}" }
                }
                button { onclick: pick, disabled: busy(), class: "onboarding__continue",
                    if busy() { "Scanning…" } else { "Choose music folder" }
                }
                button { onclick: move |_| finish(), disabled: busy(), class: "onboarding__skip", "Skip for now" }
            }
        }
    }
}
