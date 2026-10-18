use crate::ui::components::{dismiss::use_dismiss, icons::ChevronDownIcon};
use dioxus::prelude::*;

/// A select in the device's own style (the native one opens a system popup
/// that can't be skinned). `options` are `(value, label)` pairs.
#[component]
pub fn Dropdown(
    options: Vec<(String, String)>,
    value: String,
    on_change: EventHandler<String>,
    placeholder: Option<String>,
    class: Option<String>,
) -> Element {
    let mut open = use_signal(|| false);
    let dom_id = use_dismiss(open);
    let current = options
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, l)| l.clone())
        .or(placeholder)
        .unwrap_or_default();
    let class = class.unwrap_or_default();

    rsx! {
        div { id: "{dom_id}", class: "dropdown {class}",
            button {
                class: "dropdown__button",
                "aria-haspopup": "listbox",
                "aria-expanded": "{open()}",
                onclick: move |_| open.set(!open()),
                span { class: "truncate", "{current}" }
                ChevronDownIcon { size: 12 }
            }
            if open() {
                div { class: "dropdown__menu", role: "listbox",
                    for (v, label) in options.iter() {
                        button {
                            key: "{v}",
                            class: if *v == value { "dropdown__item dropdown__item--selected" } else { "dropdown__item" },
                            role: "option",
                            "aria-selected": "{*v == value}",
                            onclick: {
                                let v = v.clone();
                                move |_| {
                                    on_change.call(v.clone());
                                    open.set(false);
                                }
                            },
                            span { class: "truncate", "{label}" }
                        }
                    }
                }
            }
        }
    }
}
