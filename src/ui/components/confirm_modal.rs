use crate::ui::components::lyrics_modal::use_escape_to_close;
use dioxus::prelude::*;

/// "Are you sure?" for something heavy. A centred dialog on desktop and a
/// bottom sheet on a phone (the shared `.modal-scrim` rules do that).
/// Clicking outside, Escape and Cancel all cancel.
#[component]
pub fn ConfirmModal(
    title: String,
    /// One paragraph each.
    lines: Vec<String>,
    confirm_label: String,
    /// Style the confirm button as destructive.
    danger: Option<bool>,
    on_confirm: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    use_escape_to_close(on_cancel);

    rsx! {
        div { class: "modal-scrim confirm", onclick: move |_| on_cancel.call(()),
            div {
                class: "confirm__panel",
                role: "alertdialog",
                "aria-label": "{title}",
                onclick: move |e| e.stop_propagation(),
                p { class: "confirm__title", "{title}" }
                for (i, line) in lines.iter().enumerate() {
                    p { key: "{i}", class: "confirm__line", "{line}" }
                }
                div { class: "confirm__actions",
                    button { class: "library__tool-btn", onclick: move |_| on_cancel.call(()), "Cancel" }
                    button {
                        class: if danger.unwrap_or(false) { "btn-solid confirm__go confirm__go--danger" } else { "btn-solid confirm__go" },
                        onclick: move |_| on_confirm.call(()),
                        "{confirm_label}"
                    }
                }
            }
        }
    }
}
