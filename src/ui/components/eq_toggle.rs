use crate::ui::{
    api,
    components::icons::{HeadphonesIcon, PlusIcon, PulseIcon},
    jobs::{audio_analysis_job, use_jobs, JobKind},
    player::use_player,
};
use dioxus::prelude::*;

/// The EQ key in the player. Analyzed track: an on/off switch for its suggested EQ. Not analyzed
/// yet: a pulse icon with a plus, and pressing it queues the analysis. While that runs it blinks.
#[component]
pub fn EqToggle() -> Element {
    let player = use_player();
    let jobs = use_jobs();
    let features = crate::ui::settings::use_features();
    // A restored (paused) track hasn't been loaded yet, so the player has no analysis for it; ask the store.
    // (A hook, so it is called before anything returns early, and only looks when Audio analysis is on.)
    let id = player.current_track().map(|t| t.id).unwrap_or_default();
    let analysis_on = features.audio;
    let saved = use_resource(use_reactive!(|(id, analysis_on)| async move {
        analysis_on && api::get_audio_analysis(id).await.ok().flatten().is_some()
    }));
    let Some(track) = player.current_track() else { return rsx! {} };

    // Audio analysis off: the EQ button is the Headphone correction's switch.
    if !features.audio {
        return rsx! { HeadphoneToggle {} };
    }
    let analyzed = (player.analysis)().is_some() || saved.read().as_ref().copied().unwrap_or(false);
    let eq_enabled = (player.eq_enabled)();
    let target = format!("eq:{}", track.id);
    let busy = jobs
        .jobs
        .read()
        .iter()
        .any(|j| j.kind == JobKind::AudioTrack && !j.status.is_finished() && j.target == target);

    if analyzed {
        return rsx! {
            button {
                onclick: move |_| player.toggle_eq(),
                title: if eq_enabled { "Suggested EQ on - click to bypass" } else { "Suggested EQ off - click to apply" },
                "aria-label": "EQ",
                "aria-pressed": "{eq_enabled}",
                class: "dev-tag",
                span { class: "tag-icon", PulseIcon { size: 18 } }
            }
        };
    }
    rsx! {
        button {
            onclick: move |_| {
                if !busy {
                    jobs.enqueue(audio_analysis_job(track.clone(), false));
                }
            },
            title: if busy { "Analyzing this track..." } else { "Not analyzed yet - click to analyze and get a suggested EQ" },
            "aria-label": if busy { "Analyzing" } else { "Analyze for EQ" },
            "aria-pressed": "false",
            class: if busy { "dev-tag eq-toggle--busy" } else { "dev-tag" },
            span { class: "tag-icon",
                PulseIcon { size: 18 }
                if !busy {
                    span { class: "tag-icon__plus", PlusIcon { size: 9 } }
                }
            }
        }
    }
}

/// The Headphone correction's switch: on when a correction is in use and applied; pressing it with none in
/// use picks the first saved one. With Audio analysis off this is the player's EQ button; with it on it sits
/// next to the suggested-EQ button, so both are one press away.
#[component]
pub fn HeadphoneToggle() -> Element {
    let player = use_player();
    if player.current_track().is_none() {
        return rsx! {};
    }
    let active = (player.correction_profile)().is_some();
    let on = active && (player.correction_enabled)();
    let saved = (player.headphones)();
    let first = saved.first().map(|h| h.id);
    let title = if active {
        if on { "Headphone correction on - click to bypass" } else { "Headphone correction off - click to apply" }
    } else if first.is_some() {
        "Use your saved headphone correction"
    } else {
        "No headphone correction saved - open the EQ panel to add one"
    };
    rsx! {
        button {
            onclick: move |_| {
                if active {
                    player.set_correction_enabled(!on);
                } else if let Some(id) = first {
                    player.select_headphone(Some(id));
                }
            },
            title: "{title}",
            "aria-label": "Headphone correction",
            "aria-pressed": "{on}",
            class: "dev-tag",
            span { class: "tag-icon", HeadphonesIcon { size: 18 } }
        }
    }
}
