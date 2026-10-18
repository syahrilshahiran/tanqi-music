use crate::ui::{
    compact::use_compact,
    components::{
        device::{rim_ticks, VolumeKnob},
        eq_controls::EqControls,
        eq_toggle::{EqToggle, HeadphoneToggle},
        lyrics_panel::{DisplayLyric, LyricsBody, TranslationControls},
        icons::{CloseIcon, ExpandIcon, HeartIcon, LyricsIcon, MinimizeIcon, QueueIcon, MuteIcon, PauseIcon, PlayIcon, RepeatIcon, ShuffleIcon, SkipBackIcon, SkipNextIcon, SlidersIcon, StopIcon, VolumeIcon},
        queue_panel::QueueList,
        track_thumbnail::TrackThumbnail,
        waveform::Waveform,
    },
    player::{use_player, RepeatMode},
};
use dioxus::prelude::*;

/// `Waveform` renders one bar per peak - fine at PlayerBar's ~1000px+ width,
/// but 100 razor-thin bars packed into this widget's ~150px-wide track read
/// as a blurry smear crowding the duration label next to it. Downsampling
/// (max-of-bucket, so a loud transient still shows) to a count that
/// actually fits keeps each bar visibly distinct instead.
fn resample_peaks(peaks: &[f32], target_count: usize) -> Vec<f32> {
    if peaks.len() <= target_count {
        return peaks.to_vec();
    }
    (0..target_count)
        .map(|i| {
            let start = i * peaks.len() / target_count;
            let end = (start + 1).max((i + 1) * peaks.len() / target_count);
            peaks[start..end.min(peaks.len())].iter().cloned().fold(0.0, f32::max)
        })
        .collect()
}

/// Replaces the entire app shell while compact mode is on "music" (see
/// Shell) - a small always-on-top mini player. Playback itself lives in the
/// player context, above the shell, so it stays mounted the whole time -
/// this only swaps what's rendered around it, the same track keeps playing
/// across the switch.
///
/// Styled as a small matte-black device (see compact-music-widget.css): a
/// readout, a platter whose etched marks turn while playing with the cover as
/// its hub, a tape strip for seeking, and a row of keys. It is always on
/// screen: with nothing loaded the same device just sits idle.
#[component]
pub fn CompactMusicWidget() -> Element {
    let compact = use_compact();
    let player = use_player();
    let mut show_eq_panel = use_signal(|| false);
    let mut show_lyrics = use_signal(|| false);
    let mut show_queue = use_signal(|| false);
    let favorites = crate::ui::favorites::use_favorites();
    let features = crate::ui::settings::use_features();

    let compact_peaks = use_memo(move || resample_peaks(&(player.waveform)(), 32));
    let ticks = use_hook(rim_ticks);

    let track = player.current_track();
    let (is_playing, loading, duration) = ((player.is_playing)(), (player.loading)(), (player.duration)());
    let (shuffle, repeat) = ((player.shuffle)(), (player.repeat)());
    let (volume, muted) = ((player.volume)(), (player.muted)());
    let has_previous = player.has_previous();
    let has_next = player.has_next();
    let has_track = track.is_some();
    let is_fav = track.as_ref().is_some_and(|t| favorites.contains(&t.id));
    let fav_id = track.as_ref().map(|t| t.id.clone());
    let queue_len = (player.queue)().len();

    let position = (player.current_index)().map(|i| format!("{:02}", i + 1));
    let (title, artist, album, cover) = match &track {
        Some(t) => (t.title.clone(), t.artist.clone(), t.album.clone().unwrap_or_default(), t.thumbnail_url.clone()),
        None => ("Nothing playing".to_string(), String::new(), String::new(), None),
    };
    let repeat_title = match repeat {
        RepeatMode::Off => "Repeat off - click to repeat the whole queue",
        RepeatMode::All => "Repeating the queue - click to repeat this track",
        RepeatMode::One => "Repeating this track - click to turn repeat off",
    };
    let root_class = match (has_track, is_playing) {
        (false, _) => "cmw cmw--idle",
        (true, true) => "cmw cmw--playing",
        (true, false) => "cmw",
    };

    rsx! {
        div {
            class: "{root_class}",
            // No window frame in this mode, so the body is the drag handle;
            // the controls below stop the event so using them doesn't move it.
            onmousedown: move |_| crate::host::window().drag_window().unwrap_or(()),
            div { class: "cmw__device",
                // Model label and the little display.
                div { class: "cmw__top",
                    span { class: "cmw__label", "TQ-1" }
                    div { class: "dev-display",
                        div { class: "dev-display__title", "{title}" }
                        div { class: "dev-display__row",
                            span { class: "dev-display__artist", "{artist}" }
                            if let Some(position) = position {
                                span { class: "dev-chip", "{position}" }
                            }
                        }
                    }
                }

                // The platter. Its etched layer turns while playing; the cover
                // is the hub and stays upright. The marks are static.
                div { class: "cmw__stage",
                    if show_lyrics() {
                        div { class: "cmw__lyrics", onmousedown: |e| e.stop_propagation(),
                            div { class: "lyrics-panel__head",
                                span { class: "section-label", "Lyrics" }
                                TranslationControls {}
                                button {
                                    class: "dev-tag",
                                    title: "Back to the platter",
                                    "aria-label": "Close lyrics",
                                    onclick: move |_| show_lyrics.set(false),
                                    CloseIcon { size: 12 }
                                }
                            }
                            LyricsBody {}
                        }
                    }
                    div { class: "cmw__platter",
                        svg { class: "cmw__etch", view_box: "0 0 100 100", "aria-hidden": "true",
                            g { stroke: "#d6d6d9", stroke_width: ".22", opacity: ".35",
                                for (i, (x1, y1, x2, y2)) in ticks.iter().enumerate() {
                                    line { key: "{i}", x1: "{x1:.2}", y1: "{y1:.2}", x2: "{x2:.2}", y2: "{y2:.2}" }
                                }
                            }
                            circle { cx: "50", cy: "50", r: "25.2", fill: "none", stroke: "#2c2c2f", stroke_width: ".5" }
                            g { stroke: "#d6d6d9", stroke_width: ".32", stroke_linecap: "round", opacity: ".8",
                                line { x1: "68.8", y1: "31.2", x2: "83.5", y2: "16.5" }
                                line { x1: "31.2", y1: "68.8", x2: "16.5", y2: "83.5" }
                            }
                        }
                        div { class: "cmw__hub",
                            if has_track {
                                TrackThumbnail { url: cover, class: "cmw__cover-img" }
                            }
                        }
                        span { class: "cmw__mark", "{album}" }
                        div { class: "cmw__marks", onmousedown: |e| e.stop_propagation(),
                        button {
                            onclick: move |_| show_lyrics.set(true),
                            title: "Show the lyrics",
                            "aria-label": "Lyrics",
                            class: "cmw__mark-btn",
                            span { class: "tag-icon", LyricsIcon { size: 15 } }
                        }
                            button {
                                onclick: move |_| player.toggle_shuffle(),
                                title: if shuffle { "Shuffle on - click to play in order" } else { "Shuffle off - click to shuffle" },
                                "aria-label": "Shuffle",
                                "aria-pressed": "{shuffle}",
                                class: "cmw__mark-btn",
                                span { class: "tag-icon", ShuffleIcon { size: 15 } }
                            }
                            button {
                                onclick: move |_| player.cycle_repeat(),
                                title: "{repeat_title}",
                                "aria-label": "Repeat",
                                "aria-pressed": "{repeat != RepeatMode::Off}",
                                class: "cmw__mark-btn",
                                span { class: "tag-icon",
                                    RepeatIcon { size: 15 }
                                    if repeat == RepeatMode::One {
                                        span { class: "tag-icon__one", "1" }
                                    }
                                }
                            }
                        }
                    }
                    button {
                        onmousedown: |e| e.stop_propagation(),
                        onclick: move |_| compact.exit(),
                        title: "Back to full app",
                        "aria-label": "Back to full app",
                        class: "cmw__home",
                        ExpandIcon { size: 13 }
                    }
                    button {
                        onmousedown: |e| e.stop_propagation(),
                        onclick: move |_| crate::host::window().set_minimized(true),
                        title: "Minimize - it comes back from the taskbar or a media key",
                        "aria-label": "Minimize",
                        class: "cmw__home cmw__minimize",
                        MinimizeIcon { size: 13 }
                    }
                    button {
                        onmousedown: |e| e.stop_propagation(),
                        onclick: move |_| crate::host::window().close(),
                        title: "Close",
                        "aria-label": "Close",
                        class: "cmw__home cmw__close",
                        CloseIcon { size: 13 }
                    }
                }

                // The current line, in a row of its own under the disc.
                div { class: "cmw__lyric-row", DisplayLyric { class: "cmw__lyric".to_string(), with_next: true } }

                // EQ and volume.
                div { class: "cmw__tools", onmousedown: |e| e.stop_propagation(),
                    div { class: "cmw__group",
                        EqToggle {}
                        if features.audio {
                            HeadphoneToggle {}
                        }
                        button {
                            onclick: move |_| show_eq_panel.set(true),
                            title: "Adjust EQ",
                            "aria-label": "Adjust EQ",
                            "aria-expanded": "{show_eq_panel()}",
                            class: "dev-tag",
                            SlidersIcon { size: 14 }
                        }
                    }
                    div { class: "cmw__group",
                    button {
                        onclick: move |_| {
                            if let Some(id) = fav_id.clone() {
                                favorites.toggle(id);
                            }
                        },
                        disabled: !has_track,
                        title: if is_fav { "Remove from favorites" } else { "Add to favorites" },
                        "aria-label": "Favorite",
                        "aria-pressed": "{is_fav}",
                        class: "dev-tag",
                        HeartIcon { size: 14, filled: is_fav }
                    }
                    button {
                        onclick: move |_| show_queue.set(true),
                        title: "Queue ({queue_len})",
                        "aria-label": "Queue",
                        "aria-expanded": "{show_queue()}",
                        class: "dev-tag",
                        QueueIcon { size: 14 }
                    }
                    button {
                        onclick: move |_| player.toggle_mute(),
                        title: if muted { "Muted - click to unmute".to_string() } else { format!("Volume {}% - click to mute", (volume * 100.0).round() as i32) },
                        "aria-label": if muted { "Unmute" } else { "Mute" },
                        "aria-pressed": "{muted}",
                        class: "dev-tag",
                        span { class: "tag-icon",
                            if muted {
                                MuteIcon { size: 15 }
                            } else {
                                VolumeIcon { size: 15 }
                            }
                        }
                    }
                    }
                }

                // The tape strip: the waveform, which is also the seek bar.
                div { class: "cmw__seek", onmousedown: |e| e.stop_propagation(),
                    Waveform {
                        peaks: compact_peaks(),
                        duration,
                        on_seek: move |s| player.seek_to(s),
                        set_seeking: move |on| player.set_seeking(on),
                    }
                }

                // The keys, and the volume fader beside them.
                div { class: "cmw__deck", onmousedown: |e| e.stop_propagation(),
                    div { class: "cmw__keys",
                        button {
                            onclick: move |_| player.play_previous(),
                            disabled: !has_previous,
                            title: "Previous",
                            "aria-label": "Previous",
                            class: "dev-key",
                            SkipBackIcon { size: 20 }
                        }
                        button {
                            onclick: move |_| player.toggle_play_pause(),
                            disabled: loading || !has_track,
                            title: if is_playing { "Pause" } else { "Play" },
                            "aria-label": if is_playing { "Pause" } else { "Play" },
                            class: "dev-key",
                            if loading {
                                "…"
                            } else if is_playing {
                                PauseIcon { size: 20 }
                            } else {
                                PlayIcon { size: 20 }
                            }
                        }
                        button {
                            onclick: move |_| player.stop(),
                            disabled: !has_track,
                            title: "Stop",
                            "aria-label": "Stop",
                            class: "dev-key",
                            StopIcon { size: 18 }
                        }
                        button {
                            onclick: move |_| player.play_next(),
                            disabled: !has_next,
                            title: "Next",
                            "aria-label": "Next",
                            class: "dev-key",
                            SkipNextIcon { size: 20 }
                        }
                    }
                    VolumeKnob {}
                }
            }

            if show_queue() {
                div {
                    class: "modal-scrim cmw__scrim",
                    onmousedown: |e| e.stop_propagation(),
                    onclick: move |_| show_queue.set(false),
                    div { class: "queue-panel cmw__queue", onclick: move |e| e.stop_propagation(),
                        QueueList { on_close: move |_| show_queue.set(false) }
                    }
                }
            }

            if show_eq_panel() {
                div {
                    class: "modal-scrim cmw__scrim",
                    onmousedown: |e| e.stop_propagation(),
                    onclick: move |_| show_eq_panel.set(false),
                    div { onclick: move |e| e.stop_propagation(),
                        EqControls { on_close: move |_| show_eq_panel.set(false) }
                    }
                }
            }
        }
    }
}
