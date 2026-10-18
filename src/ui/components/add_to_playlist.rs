use crate::ui::{components::{dismiss::use_dismiss, icons::PlusIcon}, playlists::use_playlists};
use dioxus::prelude::*;

/// A "+" button that opens a small menu: pick a playlist to append
/// `track_ids` to, or name a new one.
#[component]
pub fn AddToPlaylist(track_ids: Vec<String>, class: Option<String>) -> Element {
    let playlists = use_playlists();
    let mut open = use_signal(|| false);
    let mut name = use_signal(String::new);
    let dom_id = use_dismiss(open);
    let class = class.unwrap_or_else(|| "library__row-info".to_string());

    let mut create = {
        let ids = track_ids.clone();
        move || {
            let n = name().trim().to_string();
            if n.is_empty() {
                return;
            }
            playlists.create(n, ids.clone(), |_| {});
            name.set(String::new());
            open.set(false);
        }
    };
    let mut create_on_enter = create.clone();

    rsx! {
        div { class: "add-to-playlist", id: "{dom_id}",
            button {
                class: "{class}",
                title: "Add to playlist",
                "aria-label": "Add to playlist",
                "aria-expanded": "{open()}",
                onclick: move |_| open.set(!open()),
                PlusIcon { size: 14 }
            }
            if open() {
                div { class: "add-to-playlist__menu",
                    for p in playlists.all() {
                        button {
                            key: "{p.id}",
                            class: "add-to-playlist__item",
                            onclick: {
                                let ids = track_ids.clone();
                                move |_| {
                                    playlists.add(p.id, ids.clone());
                                    open.set(false);
                                }
                            },
                            span { class: "truncate", "{p.name}" }
                            span { class: "add-to-playlist__count", "{p.track_ids.len()}" }
                        }
                    }
                    div { class: "add-to-playlist__new",
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
                        button { class: "dev-tag", onclick: move |_| create(), "Add" }
                    }
                }
            }
        }
    }
}
