use crate::ui::components::icons::ContrastIcon;
use dioxus::prelude::*;

/// Dark or silver, switched from the top bar. The choice lives in the webview
/// (see `assets/boot.js`), which applies it before first paint on the next
/// launch.
#[component]
pub fn ThemeToggle() -> Element {
    let mut theme = use_signal(|| "dark".to_string());
    use_future(move || async move {
        let mut ev = document::eval("dioxus.send(window.tqTheme())");
        if let Ok(current) = ev.recv::<String>().await {
            theme.set(current);
        }
    });
    let next = if theme() == "silver" { "dark" } else { "silver" };
    rsx! {
        button {
            class: "navbar__mini",
            title: "Appearance: {theme()}. Switch to {next}",
            "aria-label": "Switch appearance to {next}",
            onclick: move |_| {
                theme.set(next.to_string());
                document::eval(&format!("window.tqSetTheme('{next}')"));
            },
            ContrastIcon { size: 12 }
            if theme() == "silver" { "Silver" } else { "Dark" }
        }
    }
}
