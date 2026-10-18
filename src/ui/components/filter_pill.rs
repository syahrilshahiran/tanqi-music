use dioxus::prelude::*;

#[component]
pub fn FilterPill(label: String, active: bool, on_click: EventHandler<()>) -> Element {
    let cls = if active { "filter-pill--active" } else { "" };
    rsx! {
        button {
            onclick: move |_| on_click.call(()),
            class: "filter-pill {cls}",
            "{label}"
        }
    }
}
