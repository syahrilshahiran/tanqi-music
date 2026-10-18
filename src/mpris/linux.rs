//! The D-Bus half of media-key / now-playing support: an MPRIS player on its
//! own thread. Linux only.

use super::{MprisCommand, NowPlaying};
use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time};
use tokio::sync::mpsc::UnboundedReceiver;

fn loop_status_from_str(s: &str) -> LoopStatus {
    match s {
        "Track" => LoopStatus::Track,
        "Playlist" => LoopStatus::Playlist,
        _ => LoopStatus::None,
    }
}

fn loop_status_to_str(status: LoopStatus) -> &'static str {
    match status {
        LoopStatus::Track => "Track",
        LoopStatus::Playlist => "Playlist",
        LoopStatus::None => "None",
    }
}

fn metadata_for(state: &NowPlaying) -> Metadata {
    let mut builder = Metadata::builder();
    if state.has_track {
        builder = builder.title(state.title.clone()).artist([state.artist.clone()]);
        if let Some(album) = &state.album {
            builder = builder.album(album.clone());
        }
        if let Some(art) = &state.art_url {
            builder = builder.art_url(art.clone());
        }
        if let Some(ms) = state.duration_ms {
            builder = builder.length(Time::from_micros((ms * 1000.0) as i64));
        }
    }
    builder.build()
}

/// Runs the MPRIS player on its own thread, relaying D-Bus control requests
/// to `sink` and applying the state updates that arrive on `rx`.
pub(super) fn spawn(sink: Box<dyn Fn(MprisCommand) + Send + 'static>, mut rx: UnboundedReceiver<NowPlaying>) {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                log::warn!("MPRIS: couldn't start runtime, media keys unavailable: {e}");
                return;
            }
        };
        let local = tokio::task::LocalSet::new();

        local.block_on(&runtime, async move {
            let player = match Player::builder("tanqi")
                .identity("Tanqi")
                .desktop_entry("tanqi")
                .can_play(true)
                .can_pause(true)
                .can_seek(true)
                .can_control(true)
                .can_raise(true)
                .build()
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("MPRIS: couldn't claim the bus name, media keys unavailable: {e}");
                    return;
                }
            };

            // Each handler just relays the request to the webview - all the
            // real playback logic (and the queue/shuffle/repeat state it
            // depends on) lives there, not here.
            let sink = std::rc::Rc::new(sink);
            let emit = {
                let sink = sink.clone();
                move |cmd: MprisCommand| sink(cmd)
            };
            let simple = |action: &'static str, emit: &dyn Fn(MprisCommand)| {
                emit(MprisCommand { action: action.to_string(), value: None, flag: None, text: None });
            };

            macro_rules! relay {
                ($connect:ident, $action:literal) => {{
                    let emit = emit.clone();
                    player.$connect(move |_| simple($action, &emit));
                }};
            }
            relay!(connect_play_pause, "playPause");
            relay!(connect_play, "play");
            relay!(connect_pause, "pause");
            relay!(connect_next, "next");
            relay!(connect_previous, "previous");
            relay!(connect_stop, "stop");
            relay!(connect_raise, "raise");

            {
                let emit = emit.clone();
                player.connect_seek(move |_, offset| {
                    emit(MprisCommand {
                        action: "seek".to_string(),
                        value: Some(offset.as_micros() as f64 / 1_000_000.0),
                        flag: None,
                        text: None,
                    });
                });
            }
            {
                let emit = emit.clone();
                player.connect_set_position(move |_, _track, position| {
                    emit(MprisCommand {
                        action: "setPosition".to_string(),
                        value: Some(position.as_micros() as f64 / 1_000_000.0),
                        flag: None,
                        text: None,
                    });
                });
            }
            {
                let emit = emit.clone();
                player.connect_set_shuffle(move |_, shuffle| {
                    emit(MprisCommand {
                        action: "setShuffle".to_string(),
                        value: None,
                        flag: Some(shuffle),
                        text: None,
                    });
                });
            }
            {
                let emit = emit.clone();
                player.connect_set_loop_status(move |_, status| {
                    emit(MprisCommand {
                        action: "setLoopStatus".to_string(),
                        value: None,
                        flag: None,
                        text: Some(loop_status_to_str(status).to_string()),
                    });
                });
            }
            {
                let emit = emit.clone();
                player.connect_set_volume(move |_, volume| {
                    emit(MprisCommand {
                        action: "setVolume".to_string(),
                        value: Some(volume),
                        flag: None,
                        text: None,
                    });
                });
            }

            tokio::task::spawn_local(player.run());

            while let Some(state) = rx.recv().await {
                // Position is a plain read-only property on the D-Bus side
                // (no signal), so this just has to be current whenever
                // something asks - no need to announce every tick.
                player.set_position(Time::from_micros((state.position_ms * 1000.0) as i64));

                let status = if !state.has_track {
                    PlaybackStatus::Stopped
                } else if state.playing {
                    PlaybackStatus::Playing
                } else {
                    PlaybackStatus::Paused
                };

                // Each of these emits a PropertiesChanged signal, and
                // mpris-server skips ones whose value didn't actually
                // change - so pushing the whole state on every update
                // doesn't spam the bus.
                let _ = player.set_playback_status(status).await;
                let _ = player.set_metadata(metadata_for(&state)).await;
                let _ = player.set_can_go_next(state.can_go_next).await;
                let _ = player.set_can_go_previous(state.can_go_previous).await;
                let _ = player.set_can_play(state.has_track).await;
                let _ = player.set_can_pause(state.has_track).await;
                let _ = player.set_can_seek(state.has_track).await;
                let _ = player.set_shuffle(state.shuffle).await;
                let _ = player.set_loop_status(loop_status_from_str(&state.loop_status)).await;
                let _ = player.set_volume(state.volume).await;
            }
        });
    });
}
