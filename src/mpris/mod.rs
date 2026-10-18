//! MPRIS (D-Bus media player) integration, so the desktop treats Tanqi like
//! any other music player: hardware media keys work, `playerctl` controls
//! it, and GNOME/KDE show the current track in their now-playing widgets.
//!
//! Linux only: the D-Bus backend lives in `linux.rs`. On other platforms the
//! types here still exist (so the UI needs no platform checks) but updates
//! are dropped and no control requests ever arrive.
//!
//! Media keys come along for free rather than needing separate global
//! shortcut grabs - GNOME and KDE route them to whichever MPRIS player is
//! active, and on a bare WM the usual setup is `playerctl` keybindings,
//! which is also just MPRIS.
//!
//! ## Why a dedicated thread
//!
//! `mpris_server::Player` holds an `Rc` internally, so it is `!Send` and
//! cannot be parked in shared state or moved between tokio worker threads.
//! It gets its own OS thread running a current-thread runtime with a
//! `LocalSet`, and everything crosses that boundary as messages: frontend
//! state comes in over an mpsc channel, and control requests go back out on
//! the `app::mpris_events()` broadcast channel.
//!
//! ## Flow
//!
//! - Playback lives entirely in the webview (see `ui/player.rs` and
//!   `assets/engine.js`), so this side owns no audio state. The UI pushes
//!   what's playing via `update_now_playing` whenever it changes.
//! - A media key / `playerctl` / desktop-widget action arrives on D-Bus and
//!   is broadcast on `app::mpris_events()`, which the player subscribes to.

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedSender};

#[cfg(target_os = "linux")]
mod linux;

/// What the frontend reports about the current playback state. Every field
/// is pushed together rather than diffed, since the whole thing is small
/// and a partial update would risk MPRIS drifting out of sync with the
/// actual player.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub has_track: bool,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Cover art as a URL the desktop can load. A Local reference
    /// (see `local_ref`) is turned into a `file://` URI
    /// by the frontend before it gets here; anything unusable is `None`.
    pub art_url: Option<String>,
    pub duration_ms: Option<f64>,
    pub position_ms: f64,
    pub playing: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub shuffle: bool,
    /// "None" | "Playlist" | "Track", matching MPRIS's own `LoopStatus`.
    pub loop_status: String,
    /// 0-1, as the player's own slider position.
    pub volume: f64,
}

/// A control request from D-Bus, forwarded to the webview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MprisCommand {
    /// "playPause" | "play" | "pause" | "next" | "previous" | "stop" |
    /// "seek" | "setPosition" | "setShuffle" | "setLoopStatus" |
    /// "setVolume" | "raise" (Android also sends "permissionsGranted")
    pub action: String,
    /// Seconds for `seek`/`setPosition`, 0-1 for `setVolume`, unused
    /// otherwise.
    pub value: Option<f64>,
    /// Set for `setShuffle`.
    pub flag: Option<bool>,
    /// Set for `setLoopStatus` - "None" | "Playlist" | "Track".
    pub text: Option<String>,
}

/// Handle the app uses to push playback state in. Cloneable and
/// `Send`; the actual `Player` stays on its own thread.
#[derive(Clone)]
pub struct MprisHandle(UnboundedSender<NowPlaying>);

impl MprisHandle {
    pub fn update(&self, state: NowPlaying) {
        // Send failure just means the MPRIS thread isn't running (no D-Bus
        // session, or it failed to start) - playback itself doesn't care.
        let _ = self.0.send(state);
    }
}

/// Starts the MPRIS service on its own thread. Returns a handle for pushing
/// state updates, or `None` if the thread couldn't be started at all.
///
/// Never fails loudly: a machine with no D-Bus session bus (a bare TTY, some
/// containers) simply gets no media-key integration, which is a missing
/// nicety rather than a reason to stop the app from playing music.
pub fn start() -> MprisHandle {
    start_with_sink(Box::new(move |cmd| {
        let _ = crate::app::mpris_events().send(cmd);
    }))
}

/// The D-Bus half, with no app dependency: control requests go to
/// `sink` rather than straight to a webview. `start` supplies a sink that
/// forwards to the frontend; `examples/mpris_probe.rs` supplies one that
/// prints, so the service can be exercised against real `playerctl`
/// without needing to launch the whole app.
///
/// Only Linux has a backend. Elsewhere this returns a handle whose updates
/// go nowhere, exactly like a Linux machine with no session bus: playback is
/// unaffected, there are just no media-key or now-playing integrations.
pub fn start_with_sink(sink: Box<dyn Fn(MprisCommand) + Send + 'static>) -> MprisHandle {
    let (tx, rx) = mpsc::unbounded_channel::<NowPlaying>();

    #[cfg(target_os = "linux")]
    linux::spawn(sink, rx);

    // No backend: dropping the receiver makes every `update` a quiet no-op.
    #[cfg(not(target_os = "linux"))]
    drop((sink, rx));

    MprisHandle(tx)
}
