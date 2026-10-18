//! Dioxus UI: router, providers and the app shell. Pages live in `pages/`,
//! shared pieces in `components/`.

pub mod api;
pub mod compact;
pub mod components;
pub mod correction;
pub mod engine;
pub mod favorites;
pub mod jobs;
pub mod now_lyrics;
pub mod pages;
pub mod play_queue;
pub mod player;
pub mod playlists;
pub mod settings;
pub mod suggestion;
pub mod targets;
pub mod util;

use compact::{use_compact, use_compact_provider, CompactWidget};
use components::{
    compact_music_widget::CompactMusicWidget, navbar::NavBar, onboarding_gate::OnboardingGate,
    player_bar::PlayerBar, status_indicators::StatusIndicators,
};
use dioxus::prelude::*;
use pages::{eq::Eq, library::Library, lyrics::Lyrics, settings::Settings};
use player::{use_mpris, use_player, use_player_provider};

/// Bumped when the user clicks the wordmark: pages that have drill-down state
/// (the Library's artist/album pages) listen and return to their top level.
#[derive(Clone, Copy)]
pub struct HomeTick(pub Signal<u32>);

/// Android only: true while the app cannot read the user's music because the
/// permission was refused. The Library page explains and links to Settings.
#[derive(Clone, Copy)]
pub struct NeedsStoragePermission(pub Signal<bool>);

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        #[route("/")]
        Library {},
        #[route("/eq")]
        Eq {},
        #[route("/lyrics")]
        Lyrics {},
        #[route("/settings")]
        Settings {},
}

#[component]
pub fn App() -> Element {
    // All above the router, not inside any one page: a local-file analysis
    // kicked off from the EQ tab, a bulk library analysis kicked off from
    // Library, or a song playing from Library/Music, all keep running (and
    // stay controllable) when the user navigates elsewhere.
    let settings = settings::use_settings_provider();
    use_context_provider(|| HomeTick(Signal::new(0)));
    targets::use_open_target_provider();
    use_context_provider(|| NeedsStoragePermission(Signal::new(false)));
    favorites::use_favorites_provider();
    playlists::use_playlists_provider();
    jobs::use_jobs_provider();
    use_compact_provider();
    let player = use_player_provider(settings);
    use_mpris(player);
    now_lyrics::use_now_lyrics_provider(player);
    rsx! { Router::<Route> {} }
}

/// Swaps the whole app shell for a compact widget while compact mode is on;
/// the routed page unmounts along with the chrome. The player lives above
/// this, so switching never interrupts playback.
#[component]
fn Shell() -> Element {
    let compact = use_compact();
    let player = use_player();

    // Dev aid for screenshots: `TANQI_START_ROUTE=/eq TANQI_THEME=silver cargo run`
    // opens straight onto a page in a theme, without touching saved settings.
    use_hook(|| {
        if let Some(route) = std::env::var("TANQI_START_ROUTE").ok().and_then(|r| r.parse::<Route>().ok()) {
            navigator().push(route);
        }
        if std::env::var("TANQI_MINI").is_ok() {
            compact.enter(CompactWidget::Music);
        }
        // `TANQI_EVAL='window.scrollTo(0, 900)'`: run a snippet a few seconds in.
        if let Ok(js) = std::env::var("TANQI_EVAL") {
            document::eval(&format!("setTimeout(() => {{ {js} }}, 4000)"));
        }
        if let Ok(theme) = std::env::var("TANQI_THEME") {
            document::eval(&format!("document.documentElement.classList.toggle('silver', {})", theme == "silver"));
        }
    });

    // A clicked notification: go to its page (a track's analysis window is opened by the page itself).
    let open_target = targets::use_open_target();
    use_effect(move || {
        let Some(t) = (open_target.0)() else { return };
        navigator().push(targets::route_for(&t));
        if !t.contains(':') {
            open_target.clear();
        }
        #[cfg(feature = "desktop")]
        crate::host::window().set_focus();
    });
    // Desktop: a click on the system's notification pop-up arrives here (see `app::notify`).
    use_future(move || async move {
        let mut rx = app_lib::app::notification_clicks().subscribe();
        while let Ok(t) = rx.recv().await {
            open_target.open(t);
        }
    });

    // Tells the page whether the player bar is up, so modal scrims stop short
    // of it (the `player-open` class, see assets/boot.js and 00-base.css).
    let has_track = player.current_track().is_some();
    use_effect(move || {
        document::eval(&format!("tqPlayerOpen({})", has_track));
    });

    if compact.current() == Some(CompactWidget::Music) {
        return rsx! { CompactMusicWidget {} };
    }

    rsx! {
        // `app-shell--player` is the reliable "player bar is up" flag for CSS: it
        // is rendered with the shell itself, unlike the JS-set html class above.
        div { class: if has_track { "app-shell app-shell--player" } else { "app-shell" },
            OnboardingGate {}
            NavBar {}
            main { class: if has_track { "app-main app-main--player" } else { "app-main" },
                Outlet::<Route> {}
            }
            PlayerBar {}
            StatusIndicators {}
        }
    }
}
