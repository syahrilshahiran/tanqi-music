use crate::ui::{
    components::{
        icons::{HeartIcon, InfoIcon, PlusIcon, QueueIcon, SkipNextIcon},
        lyrics_modal::use_escape_to_close,
        track_thumbnail::TrackThumbnail,
    },
    favorites::use_favorites,
    playlists::use_playlists,
    player::use_player,
};
use app_lib::models::TrackSummary;
use dioxus::prelude::*;

/// A Track's actions as a bottom sheet (the phone's replacement for the
/// hover buttons on a row): play next, add to the queue, add to a Playlist,
/// Track info, and the heart.
#[component]
pub fn TrackActionsSheet(track: TrackSummary, on_info: EventHandler<TrackSummary>, on_close: EventHandler<()>) -> Element {
    let player = use_player();
    let favorites = use_favorites();
    let playlists = use_playlists();
    let mut picking = use_signal(|| false);
    let mut name = use_signal(String::new);
    use_escape_to_close(on_close);

    let is_fav = favorites.contains(&track.id);
    let (fav_id, id_for_new) = (track.id.clone(), track.id.clone());
    let (next_t, queue_t, info_t) = (track.clone(), track.clone(), track.clone());
    let create = move || {
        let n = name().trim().to_string();
        if !n.is_empty() {
            playlists.create(n, vec![id_for_new.clone()], |_| {});
            on_close.call(());
        }
    };

    let create_on_enter = create.clone();

    rsx! {
        div { class: "modal-scrim track-sheet", onclick: move |_| on_close.call(()),
            div { class: "track-sheet__panel", onclick: move |e| e.stop_propagation(),
                div { class: "track-sheet__head",
                    TrackThumbnail { url: track.thumbnail_url.clone(), class: "track-sheet__thumb" }
                    div { class: "track-sheet__titles",
                        p { class: "track-sheet__title truncate", "{track.title}" }
                        p { class: "track-sheet__artist truncate", "{track.artist}" }
                    }
                    button {
                        class: "track-sheet__heart",
                        title: if is_fav { "Remove from favorites" } else { "Add to favorites" },
                        "aria-label": "Favorite",
                        "aria-pressed": "{is_fav}",
                        onclick: move |_| favorites.toggle(fav_id.clone()),
                        HeartIcon { size: 20, filled: is_fav }
                    }
                }

                if picking() {
                    div { class: "track-sheet__list",
                        button { class: "track-sheet__back", onclick: move |_| picking.set(false), "‹  Add to playlist" }
                        for p in playlists.all() {
                            button {
                                key: "{p.id}",
                                class: "track-sheet__row",
                                onclick: {
                                    let id = track.id.clone();
                                    move |_| {
                                        playlists.add(p.id, vec![id.clone()]);
                                        on_close.call(());
                                    }
                                },
                                span { class: "truncate", "{p.name}" }
                                span { class: "track-sheet__count", "{p.track_ids.len()}" }
                            }
                        }
                        div { class: "track-sheet__new",
                            input {
                                r#type: "text",
                                placeholder: "New playlist…",
                                value: "{name}",
                                oninput: move |e| name.set(e.value()),
                                onkeydown: move |e| {
                                    if e.key() == Key::Enter {
                                        create_on_enter();
                                    }
                                },
                            }
                            button { class: "library__tool-btn", onclick: move |_| create(), "Add" }
                        }
                    }
                } else {
                    div { class: "track-sheet__tiles",
                        button {
                            class: "track-sheet__tile",
                            onclick: move |_| {
                                player.play_after_current(next_t.clone());
                                on_close.call(());
                            },
                            SkipNextIcon { size: 20 }
                            "Play next"
                        }
                        button {
                            class: "track-sheet__tile",
                            onclick: move |_| {
                                player.enqueue(queue_t.clone());
                                on_close.call(());
                            },
                            QueueIcon { size: 20 }
                            "Add to queue"
                        }
                        button { class: "track-sheet__tile", onclick: move |_| picking.set(true),
                            PlusIcon { size: 20 }
                            "Add to playlist"
                        }
                        button {
                            class: "track-sheet__tile",
                            onclick: move |_| {
                                on_info.call(info_t.clone());
                                on_close.call(());
                            },
                            InfoIcon { size: 20 }
                            "Track info"
                        }
                    }
                }
            }
        }
    }
}
