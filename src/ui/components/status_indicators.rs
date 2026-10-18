use crate::ui::{
    components::job_queue_indicator::JobQueueIndicator,
    player::use_player,
};
use dioxus::prelude::*;

/// Floating stack for background status: the shared job queue (bulk
/// audio/lyrics analysis, cover fetches). It renders nothing while idle, so
/// this is usually invisible.
///
/// Fixed-positioned rather than sitting in the nav bar's flex row, so a job
/// starting can't shove every nav link left: they're outside normal flow and
/// the header stays put whether a job is running or not.
///
/// Bottom-right rather than top: the nav bar is the busiest strip in the app
/// and a status chip parked under it competes with it, while the
/// bottom-right corner is empty on every page. Lifted clear of `PlayerBar`
/// when a track is loaded, since that's fixed to the bottom too.
#[component]
pub fn StatusIndicators() -> Element {
    let player = use_player();
    let has_track = player.current_track().is_some();

    rsx! {
        div { class: if has_track { "status-indicators status-indicators--above-player" } else { "status-indicators" },
            JobQueueIndicator {}
        }
    }
}
