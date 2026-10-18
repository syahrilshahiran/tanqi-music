//! The player as the platform's media integration sees it: MPRIS on desktop, the foreground service and
//! MediaSession on Android. What is playing goes out; control requests (media keys, lock screen) come back.

use super::*;

impl Player {
    pub(super) fn apply_mpris(&self, c: MprisCommand) {
        match c.action.as_str() {
            "playPause" => self.toggle_play_pause(),
            // MPRIS distinguishes Play/Pause from PlayPause - a blind toggle
            // would do the opposite of what some widgets ask.
            "play" => {
                if !*self.is_playing.peek() {
                    self.toggle_play_pause();
                }
            }
            "pause" => {
                if *self.is_playing.peek() {
                    self.toggle_play_pause();
                }
            }
            "next" => self.play_next(),
            "previous" => self.play_previous(),
            "stop" => self.stop(),
            "seek" => {
                if let Some(v) = c.value {
                    self.seek_to((self.get_position() + v).max(0.0));
                }
            }
            "setPosition" => {
                if let Some(v) = c.value {
                    self.seek_to(v.max(0.0));
                }
            }
            "setShuffle" => {
                if let Some(f) = c.flag {
                    if f != *self.shuffle.peek() {
                        self.toggle_shuffle();
                    }
                }
            }
            "setLoopStatus" => {
                let want = match c.text.as_deref() {
                    Some("Track") => RepeatMode::One,
                    Some("Playlist") => RepeatMode::All,
                    _ => RepeatMode::Off,
                };
                for _ in 0..3 {
                    if *self.repeat.peek() != want {
                        self.cycle_repeat();
                    }
                }
            }
            "setVolume" => {
                if let Some(v) = c.value {
                    self.set_volume(v.clamp(0.0, 1.0) as f32);
                }
            }
            "raise" => {
                crate::host::window().set_minimized(false);
                crate::host::window().set_focus();
            }
            _ => {}
        }
    }

    pub(super) fn now_playing(&self) -> NowPlaying {
        let track = self.current_track();
        let duration = *self.duration.peek();
        NowPlaying {
            has_track: track.is_some(),
            title: track.as_ref().map(|t| t.title.clone()).unwrap_or_default(),
            artist: track.as_ref().map(|t| t.artist.clone()).unwrap_or_default(),
            album: track.as_ref().and_then(|t| t.album.clone()),
            art_url: art_url_for(track.as_ref().and_then(|t| t.thumbnail_url.as_deref())),
            duration_ms: if duration > 0.0 { Some(duration * 1000.0) } else { track.as_ref().and_then(|t| t.duration_ms).map(|d| d as f64) },
            position_ms: self.get_position() * 1000.0,
            playing: *self.is_playing.peek(),
            can_go_next: self.has_next(),
            can_go_previous: self.has_previous(),
            shuffle: *self.shuffle.peek(),
            loop_status: match *self.repeat.peek() {
                RepeatMode::One => "Track",
                RepeatMode::All => "Playlist",
                RepeatMode::Off => "None",
            }
            .into(),
            volume: *self.volume.peek() as f64,
        }
    }
}

/// Bridges the player to the desktop's MPRIS service: pushes what's playing
/// out so media keys / `playerctl` / the desktop widget see it, and applies
/// control requests coming back. Mount once in the shell.
pub fn use_mpris(player: Player) {
    // Android has no MPRIS: the Kotlin side (android/MainActivity.kt) plays that
    // part. It calls `window.tqNativeCmd(cmd)` for lock-screen / notification
    // buttons, and we call `TqAndroid.nowPlaying(json)` below with the state.
    #[cfg(target_os = "android")]
    {
        let mut home = use_context::<crate::ui::HomeTick>().0;
        let mut needs_permission = use_context::<crate::ui::NeedsStoragePermission>().0;
        let open_target = crate::ui::targets::use_open_target();
        use_future(move || async move {
            let mut startup = true;
            let mut ev = document::eval(
                "window.tqNativeCmd = (c) => dioxus.send(c); dioxus.send({ action: 'appResumed' });",
            );
            while let Ok(cmd) = ev.recv::<MprisCommand>().await {
                match cmd.action.as_str() {
                    "permissionDenied" => {
                        app_lib::scanner::set_storage_readable(false);
                        needs_permission.set(true);
                    }
                    // Granted at the prompt, or in Settings while we were away. The startup Scan was held back
                    // (or refused): run it and refresh the Library.
                    "permissionsGranted" | "appResumed" => {
                        // Did a tapped notification bring us here? Go where it says.
                        let mut tv = document::eval("dioxus.send((window.TqAndroid && TqAndroid.takeTarget()) || '')");
                        if let Ok(t) = tv.recv::<String>().await {
                            if !t.is_empty() {
                                open_target.open(t);
                            }
                        }
                        let mut check = document::eval("dioxus.send(!!(window.TqAndroid && TqAndroid.hasAudioPermission()))");
                        let granted = check.recv::<bool>().await.unwrap_or(false);
                        app_lib::scanner::set_storage_readable(granted);
                        let was_missing = *needs_permission.peek();
                        needs_permission.set(!granted);
                        // The first check is the startup Scan the backend left to us.
                        let first = std::mem::replace(&mut startup, false);
                        if granted && (first || was_missing || cmd.action == "permissionsGranted") {
                            let _ = app_lib::with_store(|s| s.scan_all(&app_lib::scanner::FsScanner)).await;
                            home += 1;
                        }
                    }
                    _ => player.apply_mpris(cmd),
                }
            }
        });
    }

    use_future(move || async move {
        let mut rx = app_lib::app::mpris_events().subscribe();
        loop {
            match rx.recv().await {
                Ok(cmd) => player.apply_mpris(cmd),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // Push on every meaningful state change (position is read at push time,
    // not tracked: the platform extrapolates it while playing, and a seek pushes it afresh).
    use_effect(move || {
        let _ = (
            (player.current_index)(),
            (player.is_playing)(),
            (player.duration)(),
            (player.shuffle)(),
            (player.repeat)(),
            (player.volume)(),
            (player.queue)().len(),
        );
        push_now_playing(player.now_playing());
    });

}

/// Sends the playback state to the platform's media integration: MPRIS on
/// desktop, the foreground service and MediaSession on Android.
pub(super) fn push_now_playing(np: NowPlaying) {
    #[cfg(target_os = "android")]
    if let Ok(json) = serde_json::to_string(&np) {
        document::eval(&format!("window.TqAndroid && TqAndroid.nowPlaying(JSON.stringify({json}))"));
    }
    api::update_now_playing(np);
}

/// MPRIS wants a URL the desktop itself can load: remote thumbnails are,
/// `local:` library paths become `file://` URIs, anything else is dropped.
fn art_url_for(thumbnail: Option<&str>) -> Option<String> {
    let t = thumbnail?;
    if let Some(path) = app_lib::local_ref::decode(t) {
        return path.starts_with('/').then(|| format!("file://{}", urlencoding::encode(path).replace("%2F", "/")));
    }
    (t.starts_with("http://") || t.starts_with("https://")).then(|| t.to_string())
}

