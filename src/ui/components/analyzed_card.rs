use crate::ui::components::track_thumbnail::TrackThumbnail;
use crate::ui::util::time_ago;
use dioxus::prelude::*;

/// Shared shell for a card summarizing one saved analysis (audio EQ or
/// lyrics) - thumbnail, title/artist and a type-specific `meta` line.
#[component]
pub fn AnalyzedCard(
    thumbnail_url: Option<String>,
    title: String,
    artist: String,
    created_at: String,
    on_open: EventHandler<()>,
    /// The one line that differs between analysis types (EQ's shape/BPM
    /// summary vs lyrics' vibe summary) - everything else about the card is
    /// shared.
    meta: Option<Element>,
    /// Extra controls, shown as icons in the card's top-right corner (clicks on them must not reach `on_open`).
    actions: Option<Element>,
) -> Element {
    let has_actions = actions.is_some();
    rsx! {
        div {
            onclick: move |_| on_open.call(()),
            class: "analyzed-card",
            TrackThumbnail { url: thumbnail_url, class: "analyzed-card__thumb" }
            if actions.is_some() {
                div { class: "analyzed-card__actions", {actions} }
            }
            div { class: if has_actions { "analyzed-card__body analyzed-card__body--actions" } else { "analyzed-card__body" },
                p { class: "analyzed-card__title", "{title}" }
                p { class: "analyzed-card__artist", "{artist}" }
                {meta}
                div { class: "analyzed-card__footer",
                    span { class: "analyzed-card__time", "{time_ago(&created_at)}" }
                }
            }
        }
    }
}
