use crate::ui::{
    api,
    components::{dropdown::Dropdown, icons::{CloseIcon, DownloadIcon}},
};
use app_lib::models::ArtworkOption;
use dioxus::prelude::*;

/// Preview + download of iTunes artwork at the largest confirmed size.
#[component]
pub fn ArtworkModal(
    /// Any size variant of the artwork's URL - the initial preview and the
    /// base to probe other resolutions from.
    image_url: String,
    title: String,
    on_close: EventHandler<()>,
) -> Element {
    let mut options = use_signal(Vec::<ArtworkOption>::new);
    let mut loading = use_signal(|| true);
    let mut selected = use_signal(|| None::<ArtworkOption>);
    let mut downloading = use_signal(|| false);
    let mut download_error = use_signal(|| None::<String>);
    let mut saved = use_signal(|| false);

    let probe_url = image_url.clone();
    use_future(move || {
        let probe_url = probe_url.clone();
        async move {
            loading.set(true);
            if let Ok(sizes) = app_lib::with_metadata(move |m| async move { m.itunes_artwork_sizes(&probe_url).await }).await {
                // Prefer the largest confirmed size over the (possibly small)
                // default preview passed in.
                if let Some(last) = sizes.last() {
                    selected.set(Some(last.clone()));
                }
                options.set(sizes);
            }
            loading.set(false);
        }
    });

    let preview_url = selected().map(|s| s.url).unwrap_or_else(|| image_url.clone());

    let handle_download = {
        let preview_url = preview_url.clone();
        let title = title.clone();
        move |_| {
            downloading.set(true);
            download_error.set(None);
            saved.set(false);
            let size = selected().map(|s| s.size);
            let suggested_name = match size {
                Some(size) => format!("{title} - {size}x{size}.jpg"),
                None => format!("{title}.jpg"),
            };
            let url = preview_url.clone();
            spawn(async move {
                match api::download_itunes_artwork(url, suggested_name).await {
                    Ok(did_save) => {
                        if did_save {
                            saved.set(true);
                        }
                    }
                    Err(e) => download_error.set(Some(e)),
                }
                downloading.set(false);
            });
        }
    };

    rsx! {
        // Focused on mount so Escape reaches `onkeydown` here; stopping the
        // event keeps it from also closing a modal this one was opened from.
        div {
            class: "modal-scrim artwork-modal",
            tabindex: "0",
            onmounted: move |e| async move {
                let _ = e.set_focus(true).await;
            },
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    e.stop_propagation();
                    on_close.call(());
                }
            },
            onclick: move |e| {
                e.stop_propagation();
                on_close.call(());
            },
            div {
                class: "artwork-modal__panel",
                onclick: move |e| e.stop_propagation(),
                // Download and Close ride on the picture's corner, so they are never pushed
                // off the screen (or under the player bar) however many sizes there are.
                div { class: "artwork-modal__stage",
                    img {
                        src: "{preview_url}",
                        alt: "{title}",
                        class: "artwork-modal__img",
                    }
                    // Size picker on the picture's corner: no row of buttons to scroll.
                    div { class: "artwork-modal__size-pick",
                        if loading() {
                            span { class: "artwork-modal__note", "Checking sizes…" }
                        } else if options().is_empty() {
                            span { class: "artwork-modal__note", "Default size only" }
                        } else {
                            Dropdown {
                                value: selected().map(|o| o.size.to_string()).unwrap_or_default(),
                                options: options().iter().map(|o| (o.size.to_string(), format!("{}×{}", o.size, o.size))).collect::<Vec<_>>(),
                                on_change: move |v: String| {
                                    if let Some(o) = options().into_iter().find(|o| o.size.to_string() == v) {
                                        selected.set(Some(o));
                                    }
                                },
                            }
                        }
                    }
                    div { class: "artwork-modal__tools",
                        button {
                            onclick: handle_download,
                            disabled: downloading() || loading(),
                            class: "artwork-modal__tool",
                            title: if downloading() { "Downloading…" } else { "Download this size" },
                            "aria-label": "Download",
                            if downloading() {
                                span { class: "dots", "aria-hidden": "true",
                                    span {}
                                    span {}
                                    span {}
                                }
                            } else {
                                DownloadIcon { size: 18 }
                            }
                        }
                        button {
                            onclick: move |_| on_close.call(()),
                            class: "artwork-modal__tool",
                            title: "Close",
                            "aria-label": "Close",
                            CloseIcon { size: 18 }
                        }
                    }
                }

                if saved() {
                    span { class: "artwork-modal__saved", "Saved." }
                }
                if let Some(e) = download_error() {
                    span { class: "artwork-modal__error", "{e}" }
                }
            }
        }
    }
}
