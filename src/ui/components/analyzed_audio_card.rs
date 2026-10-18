use crate::ui::components::{analyzed_card::AnalyzedCard, icons::InfoIcon};
use app_lib::models::AudioAnalysis;
use dioxus::prelude::*;

/// One-line summary of the suggested EQ shape, for a quick scan without
/// opening the full panel - same bass-vs-treble heuristic as the modal's
/// `eq_summary` (in `audio_analysis_modal.rs`), just condensed.
fn shape_summary(a: &AudioAnalysis) -> String {
    match app_lib::audio::verdict(&a.spectrum_db) {
        Some(v) => v.short,
        None => legacy_shape_summary(a).to_string(),
    }
}

fn legacy_shape_summary(a: &AudioAnalysis) -> &'static str {
    let diff = a.bass_db - a.treble_db;
    if diff > 15.0 {
        "Very bass-heavy"
    } else if diff > 8.0 {
        "Leans bass-heavy"
    } else if diff < -15.0 {
        "Very bright"
    } else if diff < -8.0 {
        "Leans bright"
    } else {
        "Fairly balanced"
    }
}

/// Clicking the card plays the track in the player bar; "Details" opens the
/// full analysis.
#[component]
pub fn AnalyzedAudioCard(
    analysis: AudioAnalysis,
    on_play: EventHandler<AudioAnalysis>,
    on_details: EventHandler<AudioAnalysis>,
) -> Element {
    let mut summary = shape_summary(&analysis).to_string();
    if let Some(bpm) = analysis.bpm {
        summary.push_str(&format!(" · {} BPM", bpm.round() as i64));
    }
    if let Some(key) = analysis.key.as_deref().filter(|k| !k.is_empty()) {
        summary.push_str(&format!(" · {key}"));
    }
    let (a, d) = (analysis.clone(), analysis.clone());
    rsx! {
        AnalyzedCard {
            thumbnail_url: analysis.thumbnail_url.clone(),
            title: analysis.title.clone(),
            artist: analysis.artist.clone(),
            created_at: analysis.created_at.clone(),
            on_open: move |_| on_play.call(a.clone()),
            actions: rsx! {
                button {
                    class: "analyzed-card__details",
                    title: "Open the full EQ analysis",
                    "aria-label": "Details",
                    onclick: move |e| {
                        e.stop_propagation();
                        on_details.call(d.clone());
                    },
                    InfoIcon { size: 22 }
                }
            },
            meta: rsx! {
                p { class: "analyzed-card__summary", "{summary}" }
            },
        }
    }
}
