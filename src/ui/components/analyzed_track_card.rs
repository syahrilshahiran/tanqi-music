use crate::ui::components::analyzed_card::AnalyzedCard;
use app_lib::models::LyricsAnalysis;
use dioxus::prelude::*;

#[component]
pub fn AnalyzedTrackCard(analysis: LyricsAnalysis, on_open: EventHandler<LyricsAnalysis>) -> Element {
    let vibe = analysis.vibe_summary.clone();
    let a = analysis.clone();
    rsx! {
        AnalyzedCard {
            thumbnail_url: analysis.thumbnail_url.clone(),
            title: analysis.title.clone(),
            artist: analysis.artist.clone(),
            created_at: analysis.created_at.clone(),
            on_open: move |_| on_open.call(a.clone()),
            meta: rsx! {
                if !vibe.is_empty() {
                    p { class: "analyzed-card__vibe", "{vibe}" }
                }
            },
        }
    }
}
