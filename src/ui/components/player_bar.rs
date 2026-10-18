use crate::ui::{
    components::{
        device::{rim_ticks, VolumeKnob},
        eq_controls::EqControls,
        eq_toggle::{EqToggle, HeadphoneToggle},
        icons::{HeartIcon, InfoIcon, MuteIcon, PauseIcon, PlayIcon, SkipBackIcon, SkipNextIcon, SlidersIcon, StopIcon, VolumeIcon},
        play_mode_controls::PlayModeControls,
        lyrics_panel::{DisplayLyric, LyricsPanel},
        queue_panel::QueuePanel,
        song_info_modal::SongInfoModal,
        track_thumbnail::TrackThumbnail,
        waveform::Waveform,
    },
    favorites::use_favorites,
    player::use_player,
    targets::use_open_target,
};
use dioxus::prelude::*;

/// The bottom transport: the mini player's device laid flat - a disc with the
/// cover as its hub, the display, the keys, the tape strip, tags and the fader.
#[component]
pub fn PlayerBar() -> Element {
    let player = use_player();
    let mut show_eq_panel = use_signal(|| false);
    let mut show_info = use_signal(|| false);
    // Phone only: the bar is a one-line mini player until expanded to the
    // full-screen Now Playing sheet (CSS ignores this at >= 960px).
    let mut expanded = use_signal(|| false);
    let open_target = use_open_target();
    let features = crate::ui::settings::use_features();
    let favorites = use_favorites();

    // Modal scrims (`.modal-scrim`) are `position: fixed; inset: 0`, which
    // used to cover this bar - the transport was dimmed and unclickable
    // behind them, so a long AI/audio analysis left no way to stop playback
    // without closing the modal. The bar sits above the scrims (`z-index: 70`
    // against their 50) and the `player-open` flag (toggled by Shell) tells
    // them to stop short of it, so it stays both visible and usable.
    // A title or artist wider than its card scrolls while playing (phone; see zz-mobile.css):
    // measure after each change of track or of the bar's size, and mark the ones that overflow.
    use_effect(move || {
        let _ = (player.current_track().map(|t| t.id), expanded());
        document::eval(
            "setTimeout(() => document.querySelectorAll('.player-bar__display .dev-display__title, .player-bar__display .dev-display__artist').forEach(el => { el.classList.remove('marquee--long'); const inner = el.firstElementChild; const shift = inner ? inner.offsetWidth - el.clientWidth : 0; if (shift > 2) { const loop = inner.offsetWidth + 48; el.style.setProperty('--shift', loop + 'px'); el.style.setProperty('--dur', Math.max(6, loop / 30) + 's'); el.classList.add('marquee--long'); } }), 250)",
        );
    });

    let Some(track) = player.current_track() else {
        return rsx! {};
    };
    let (is_playing, loading) = ((player.is_playing)(), (player.loading)());
    let error = (player.error)();
    let waveform = (player.waveform)();
    let duration = (player.duration)();
    let has_previous = player.has_previous();
    let has_next = player.has_next();
    let (volume, muted) = ((player.volume)(), (player.muted)());
    let is_fav = favorites.contains(&track.id);
    let ticks = use_hook(rim_ticks);
    let position = (player.current_index)().map(|i| format!("{:02}", i + 1));

    rsx! {
        div {
            class: match (is_playing, expanded()) {
                (true, true) => "player-bar player-bar--playing player-bar--open",
                (true, false) => "player-bar player-bar--playing",
                (false, true) => "player-bar player-bar--open",
                (false, false) => "player-bar",
            },
            div { class: "player-bar__row",
                div { class: "player-bar__disc",
                    svg { class: "player-bar__etch", view_box: "0 0 100 100", "aria-hidden": "true",
                        g { stroke: "#d6d6d9", stroke_width: ".5", opacity: ".6",
                            for (i, (x1, y1, x2, y2)) in ticks.iter().enumerate() {
                                line { key: "{i}", x1: "{x1:.2}", y1: "{y1:.2}", x2: "{x2:.2}", y2: "{y2:.2}" }
                            }
                        }
                        // Two bold etched marks, like the mini player's: they make
                        // the turning easy to see at a glance.
                        g { stroke: "#fff", stroke_width: "1.6", stroke_linecap: "round",
                            line { x1: "74", y1: "26", x2: "84", y2: "16" }
                            line { x1: "26", y1: "74", x2: "16", y2: "84" }
                        }
                    }
                    div { class: "player-bar__hub",
                        TrackThumbnail { url: track.thumbnail_url.clone(), class: "player-bar__cover" }
                    }
                }

                div {
                    class: "dev-display player-bar__display",
                    onclick: move |_| expanded.set(true),
                    div { class: "dev-display__title", span { class: "marquee__inner", "data-text": "{track.title}", "{track.title}" } }
                    div { class: "dev-display__row",
                        span {
                            class: "dev-display__artist dev-display__artist--link",
                            title: "Open {track.artist}",
                            onclick: {
                                let artist = track.artist.clone();
                                move |e| {
                                    e.stop_propagation();
                                    expanded.set(false);
                                    open_target.open(format!("artist:{artist}"));
                                }
                            },
                            span { class: "marquee__inner", "data-text": "{track.artist}", "{track.artist}" }
                        }
                        if let Some(position) = position {
                            span { class: "dev-chip", "{position}" }
                        }
                    }
                    DisplayLyric {}
                }

                // Drawn by `assets/engine.js` (spectrum bars with peak hold).
                canvas { id: "tq-meter", class: "player-bar__meter", "aria-hidden": "true" }

                div { class: "player-bar__keys",
                    button {
                        onclick: move |_| player.play_previous(),
                        disabled: !has_previous,
                        title: "Previous",
                        "aria-label": "Previous",
                        class: "dev-key",
                        SkipBackIcon { size: 18 }
                    }
                    button {
                        onclick: move |_| player.toggle_play_pause(),
                        disabled: loading,
                        title: if is_playing { "Pause" } else { "Play" },
                        "aria-label": if is_playing { "Pause" } else { "Play" },
                        class: "dev-key dev-key--play",
                        if loading {
                            "…"
                        } else if is_playing {
                            PauseIcon { size: 18 }
                        } else {
                            PlayIcon { size: 18 }
                        }
                    }
                    button {
                        onclick: move |_| player.stop(),
                        title: "Stop",
                        "aria-label": "Stop",
                        class: "dev-key dev-key--stop",
                        StopIcon { size: 16 }
                    }
                    button {
                        onclick: move |_| player.play_next(),
                        disabled: !has_next,
                        title: "Next",
                        "aria-label": "Next",
                        class: "dev-key",
                        SkipNextIcon { size: 18 }
                    }
                }

                div { class: "player-bar__seek",
                    Waveform {
                        peaks: waveform,
                        duration,
                        on_seek: move |s| player.seek_to(s),
                        set_seeking: move |on| player.set_seeking(on),
                    }
                }

                div { class: "player-bar__tags",
                    PlayModeControls {}
                    button {
                        onclick: {
                            let id = track.id.clone();
                            move |_| favorites.toggle(id.clone())
                        },
                        title: if is_fav { "Remove from favorites" } else { "Add to favorites" },
                        "aria-label": "Favorite",
                        "aria-pressed": "{is_fav}",
                        class: "dev-tag",
                        HeartIcon { size: 14, filled: is_fav }
                    }
                    QueuePanel {}
                    LyricsPanel {}
                    button {
                        onclick: move |_| show_info.set(true),
                        title: "Track info",
                        "aria-label": "Track info",
                        class: "dev-tag",
                        InfoIcon { size: 14 }
                    }
                    button {
                        onclick: move |_| player.toggle_mute(),
                        title: if muted { "Muted - click to unmute".to_string() } else { format!("Volume {}% - click to mute", (volume * 100.0).round() as i32) },
                        "aria-label": if muted { "Unmute" } else { "Mute" },
                        "aria-pressed": "{muted}",
                        class: "dev-tag",
                        span { class: "tag-icon",
                            if muted {
                                MuteIcon { size: 18 }
                            } else {
                                VolumeIcon { size: 18 }
                            }
                        }
                    }
                    EqToggle {}
                    if features.audio {
                        HeadphoneToggle {}
                    }
                    div { class: "player-bar__eq",
                        button {
                            onclick: move |_| show_eq_panel.set(!show_eq_panel()),
                            title: "Adjust EQ",
                            "aria-label": "Adjust EQ",
                            "aria-expanded": "{show_eq_panel()}",
                            class: "dev-tag",
                            SlidersIcon { size: 14 }
                        }
                        if show_eq_panel() {
                            div { class: "player-bar__eq-panel",
                                EqControls { on_close: move |_| show_eq_panel.set(false) }
                            }
                        }
                    }
                }

                VolumeKnob {}

                button {
                    class: "player-bar__expand",
                    onclick: move |_| expanded.set(!expanded()),
                    title: if expanded() { "Back to the mini player" } else { "Open Now Playing" },
                    "aria-label": if expanded() { "Collapse Now Playing" } else { "Expand Now Playing" },
                    "aria-expanded": "{expanded()}",
                    if expanded() { "▾" } else { "▴" }
                }
            }

            if let Some(err) = error {
                div { class: "player-bar__error-wrap",
                    div { class: "player-bar__error",
                        "{err}"
                    }
                }
            }
        }
        if show_info() {
            SongInfoModal { track: track.clone(), on_close: move |_| show_info.set(false) }
        }
    }
}
