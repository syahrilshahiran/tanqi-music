use crate::ui::{
    api,
    components::{
        icons::{CheckIcon, CloseIcon, PlusIcon, RefreshIcon, TrashIcon},
        lyrics_modal::use_escape_to_close,
    },
};
use app_lib::{commands::FolderListing, library_store::FolderInfo};
use dioxus::prelude::*;

/// Where the folder browser starts on Android: the phone's shared storage.
const BROWSE_START: &str = "/storage/emulated/0";

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The Library's folders: each with its path and how many Tracks it holds,
/// plus adding and removing. On desktop "Add folder" opens the native dialog
/// (`on_add_native`); on Android, which has none, it walks the device's
/// folders in here and hands the chosen path to `on_add_path`.
#[component]
pub fn FoldersModal(
    folders: Vec<FolderInfo>,
    busy: bool,
    /// A status or error line from the page, so it is not hidden behind this popup.
    note: Option<String>,
    /// Open straight into the folder browser (the empty Library's button).
    start_adding: bool,
    on_close: EventHandler<()>,
    on_add_native: EventHandler<()>,
    on_add_path: EventHandler<String>,
    on_remove: EventHandler<String>,
    on_rescan: EventHandler<()>,
) -> Element {
    use_escape_to_close(on_close);

    let mut browse = use_signal(|| None::<FolderListing>);
    let mut browse_error = use_signal(|| None::<String>);
    // A Remove drops the folder's Tracks from the Library, so it asks twice.
    let mut confirming = use_signal(|| None::<String>);

    let open_dir = move |path: String| {
        spawn(async move {
            match api::list_subfolders(path).await {
                Ok(listing) => {
                    browse.set(Some(listing));
                    browse_error.set(None);
                }
                Err(e) => browse_error.set(Some(e)),
            }
        });
    };
    let start_browsing = move || open_dir(BROWSE_START.to_string());
    use_hook(move || {
        if start_adding && cfg!(target_os = "android") {
            start_browsing();
        }
    });

    rsx! {
        div { class: "modal-scrim folders", onclick: move |_| on_close.call(()),
            div { class: "folders__panel", onclick: move |e| e.stop_propagation(),
                if let Some(listing) = browse() {
                    div { class: "folders__head",
                        p { class: "folders__title", "Choose a folder" }
                        button { class: "library__tool-btn", onclick: move |_| browse.set(None), "Cancel" }
                    }
                    div { class: "folders__here",
                        p { class: "folders__path", "{listing.path}" }
                        p { class: "folders__count", "{plural(listing.audio_files, \"audio file\")} here" }
                    }
                    div { class: "folders__body",
                        if let Some(parent) = listing.parent.clone() {
                            button { class: "folders__dir", onclick: move |_| open_dir(parent.clone()), "↑  Up" }
                        }
                        for (name, path) in listing.folders.clone() {
                            button {
                                key: "{path}",
                                class: "folders__dir",
                                onclick: move |_| open_dir(path.clone()),
                                "{name}"
                            }
                        }
                        if listing.folders.is_empty() {
                            p { class: "folders__empty", "No sub-folders." }
                        }
                    }
                    if let Some(e) = browse_error() {
                        p { class: "folders__error", "{e}" }
                    }
                    div { class: "folders__foot",
                        button {
                            class: "library__tool-btn",
                            disabled: busy,
                            onclick: {
                                let path = listing.path.clone();
                                move |_| {
                                    on_add_path.call(path.clone());
                                    browse.set(None);
                                }
                            },
                            CheckIcon { size: 16 }
                            "Use this folder"
                        }
                    }
                } else {
                    div { class: "folders__head",
                        p { class: "folders__title", "Library folders" }
                        button { class: "library__tool-btn btn--icon", title: "Close", "aria-label": "Close", onclick: move |_| on_close.call(()), CloseIcon { size: 18 } }
                    }
                    div { class: "folders__body",
                        if folders.is_empty() {
                            p { class: "folders__empty", "No folders yet. Add the one your music lives in." }
                        }
                        for folder in folders.clone() {
                            div { key: "{folder.path}", class: "folders__row",
                                div { class: "folders__row-main",
                                    p { class: "folders__path", "{folder.path}" }
                                    p { class: "folders__count", "{plural(folder.tracks, \"track\")}" }
                                }
                                if confirming().as_deref() == Some(folder.path.as_str()) {
                                    button {
                                        class: "folders__remove folders__remove--confirm",
                                        title: "Removes its Tracks from the Library; files on disk are untouched",
                                        onclick: {
                                            let path = folder.path.clone();
                                            move |_| {
                                                confirming.set(None);
                                                on_remove.call(path.clone());
                                            }
                                        },
                                        "Remove?"
                                    }
                                } else {
                                    button {
                                        class: "folders__remove",
                                        title: "Stop watching this folder (files on disk are untouched)",
                                        onclick: {
                                            let path = folder.path.clone();
                                            move |_| confirming.set(Some(path.clone()))
                                        },
                                        "aria-label": "Remove",
                                        TrashIcon { size: 18 }
                                    }
                                }
                            }
                        }
                    }
                    if busy {
                        p { class: "folders__note", "Scanning…" }
                    } else if let Some(note) = note {
                        p { class: "folders__note", "{note}" }
                    }
                    div { class: "folders__foot",
                        button {
                            class: "library__tool-btn",
                            disabled: busy || folders.is_empty(),
                            title: "Look for new, changed and deleted files in every folder",
                            onclick: move |_| on_rescan.call(()),
                            RefreshIcon { size: 16 }
                            "Rescan"
                        }
                        button {
                            class: "library__tool-btn",
                            disabled: busy,
                            onclick: move |_| {
                                if cfg!(target_os = "android") {
                                    start_browsing();
                                } else {
                                    on_add_native.call(());
                                }
                            },
                            PlusIcon { size: 16 }
                            "Add folder"
                        }
                    }
                }
            }
        }
    }
}
