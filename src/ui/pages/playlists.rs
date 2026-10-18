//! The Library's Playlists tab and a single playlist's page.

use app_lib::models::{Track, TrackSummary};
use dioxus::prelude::*;

use crate::ui::components::icons::{BackIcon, ChevronDownIcon, ChevronUpIcon, CloseIcon, PlayIcon};
use crate::ui::player::use_player;
use crate::ui::playlists::use_playlists;
use crate::ui::util::format_duration;

fn summary(t: &Track) -> TrackSummary {
    TrackSummary {
        id: t.id.clone(),
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        duration_ms: t.duration_ms,
        thumbnail_url: t.thumbnail_url.clone(),
    }
}

/// All playlists, plus a field to start a new one.
#[component]
pub fn PlaylistList(on_open: EventHandler<i64>) -> Element {
    let playlists = use_playlists();
    let mut name = use_signal(String::new);
    let all = playlists.all();

    let mut create = move || {
        let n = name().trim().to_string();
        if n.is_empty() {
            return;
        }
        playlists.create(n, vec![], move |id| on_open.call(id));
        name.set(String::new());
    };

    rsx! {
        div { class: "playlists__new",
            input {
                class: "library__search",
                r#type: "text",
                placeholder: "New playlist name…",
                value: "{name}",
                oninput: move |e| name.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        create();
                    }
                },
            }
            button { class: "library__tool-btn", onclick: move |_| create(), "Create" }
        }
        if all.is_empty() {
            p { class: "empty-state", "No playlists yet. Name one above, or use + on any track." }
        }
        div { class: "library__tracks",
            for p in all {
                button { key: "{p.id}", class: "library__row playlists__item", onclick: move |_| on_open.call(p.id),
                    div { class: "library__row-body",
                        p { class: "library__row-title", "{p.name}" }
                    }
                    span { class: "library__row-time", "{p.track_ids.len()}" }
                }
            }
        }
    }
}

/// One playlist: play it, rename it, reorder or drop its tracks.
#[component]
pub fn PlaylistPage(id: i64, tracks: Vec<Track>, on_back: EventHandler<()>) -> Element {
    let player = use_player();
    let playlists = use_playlists();
    let mut confirm_delete = use_signal(|| false);
    let mut editing = use_signal(|| false);
    let mut draft = use_signal(String::new);
    let mut commit = move || {
        let n = draft().trim().to_string();
        editing.set(false);
        if !n.is_empty() {
            playlists.rename(id, n);
        }
    };

    let Some(playlist) = playlists.get(id) else {
        return rsx! {
            button { class: "library__back", onclick: move |_| on_back.call(()),
                BackIcon { size: 12 }
                "Back" }
            p { class: "text-muted-sm", "That playlist no longer exists." }
        };
    };

    // (position in the playlist, track); ids with no file in the library are
    // kept in the playlist but not shown or played.
    let rows: Vec<(usize, Track)> = playlist
        .track_ids
        .iter()
        .enumerate()
        .filter_map(|(pos, tid)| tracks.iter().find(|t| &t.id == tid).map(|t| (pos, t.clone())))
        .collect();
    let queue: Vec<TrackSummary> = rows.iter().map(|(_, t)| summary(t)).collect();
    let hidden = playlist.track_ids.len() - rows.len();
    let last_pos = playlist.track_ids.len().saturating_sub(1);
    let current = player.current_track().map(|t| t.id);
    let track_label = format!("{} track{}", rows.len(), if rows.len() == 1 { "" } else { "s" });

    rsx! {
        button { class: "library__back", onclick: move |_| on_back.call(()),
            BackIcon { size: 12 }
            "Back" }
        div { class: "playlists__head",
            if editing() {
                input {
                    class: "playlists__name",
                    r#type: "text",
                    value: "{draft}",
                    "aria-label": "Playlist name",
                    onmounted: move |e| {
                        spawn(async move {
                            let _ = e.set_focus(true).await;
                        });
                    },
                    oninput: move |e| draft.set(e.value()),
                    onkeydown: move |e| match e.key() {
                        Key::Enter => {
                            commit();
                        }
                        Key::Escape => editing.set(false),
                        _ => {}
                    },
                    onblur: move |_| {
                        if editing() {
                            commit();
                        }
                    },
                }
            } else {
                h2 {
                    class: "library__hero-title playlists__title",
                    title: "Double-click to rename",
                    ondoubleclick: {
                        let name = playlist.name.clone();
                        move |_| {
                            draft.set(name.clone());
                            editing.set(true);
                        }
                    },
                    "{playlist.name}"
                }
            }
            p { class: "text-muted-xs",
                "{track_label}"
                if hidden > 0 {
                    " · {hidden} missing from the library"
                }
            }
            div { class: "playlists__actions",
                button {
                    class: "btn-solid library__play-all",
                    disabled: queue.is_empty(),
                    onclick: {
                        let queue = queue.clone();
                        move |_| {
                            if let Some(first) = queue.first() {
                                player.play_track(first.clone(), Some(queue.clone()));
                            }
                        }
                    },
                    PlayIcon { size: 12 }
                    "Play"
                }
                button {
                    class: if confirm_delete() { "library__tool-btn playlists__delete--confirm" } else { "library__tool-btn" },
                    onclick: move |_| {
                        if confirm_delete() {
                            playlists.delete(id);
                            on_back.call(());
                        } else {
                            confirm_delete.set(true);
                        }
                    },
                    onmouseleave: move |_| confirm_delete.set(false),
                    if confirm_delete() { "Click again to delete" } else { "Delete playlist" }
                }
            }
        }
        if rows.is_empty() {
            p { class: "empty-state", "Empty. Add tracks with + or ⋯ on any track." }
        }
        div { class: "library__tracks",
            for (n, (pos, t)) in rows.iter().enumerate() {
                {
                    let (pos, s) = (*pos, summary(t));
                    let is_current = current.as_deref() == Some(t.id.as_str());
                    let queue = queue.clone();
                    let (play_s, play_q) = (s.clone(), queue.clone());
                    // A tap plays on a touch screen, where there is no double-click.
                    let (tap_s, tap_q) = (s.clone(), queue.clone());
                    rsx! {
                        div {
                            key: "{pos}-{t.id}",
                            class: if is_current { "library__row library__row--current" } else { "library__row" },
                            ondoubleclick: move |_| player.play_track(play_s.clone(), Some(play_q.clone())),
                            span { class: "library__row-num", "{n + 1}" }
                            div {
                                class: "library__row-body",
                                onclick: move |_| player.play_track(tap_s.clone(), Some(tap_q.clone())),
                                p { class: "library__row-title", "{t.title}" }
                                p { class: "library__row-sub", "{t.artist}" }
                            }
                            if let Some(ms) = t.duration_ms {
                                span { class: "library__row-time", "{format_duration(ms)}" }
                            }
                            button {
                                class: "library__row-info",
                                title: "Move up",
                                "aria-label": "Move up",
                                disabled: pos == 0,
                                onclick: move |_| playlists.move_track(id, pos, -1),
                                ChevronUpIcon { size: 14 }
                            }
                            button {
                                class: "library__row-info",
                                title: "Move down",
                                "aria-label": "Move down",
                                disabled: pos == last_pos,
                                onclick: move |_| playlists.move_track(id, pos, 1),
                                ChevronDownIcon { size: 14 }
                            }
                            button {
                                class: "library__row-info",
                                title: "Remove from playlist",
                                "aria-label": "Remove from playlist",
                                onclick: move |_| playlists.remove(id, pos),
                                CloseIcon { size: 14 }
                            }
                            button {
                                class: "library__row-play",
                                title: "Play",
                                "aria-label": "Play",
                                onclick: move |_| player.play_track(s.clone(), Some(queue.clone())),
                                PlayIcon { size: 11 }
                            }
                        }
                    }
                }
            }
        }
    }
}
