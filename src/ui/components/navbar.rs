use crate::ui::{settings::use_features, components::{icons::{ExpandIcon, GearIcon, LibraryIcon, LyricsIcon, SlidersIcon}, theme_toggle::ThemeToggle}, compact::{use_compact, CompactWidget}, HomeTick, Route};
use dioxus::prelude::*;

fn is_active(current: &Route, link: &Route) -> bool {
    std::mem::discriminant(current) == std::mem::discriminant(link)
}

/// Top bar: wordmark, the three destinations, and the mini-player switch.
#[component]
pub fn NavBar() -> Element {
    let compact = use_compact();
    let mut home = use_context::<HomeTick>().0;
    let current = use_route::<Route>();
    let features = use_features();
    // The EQ and Lyrics pages belong to optional features.
    let links: Vec<(Route, &str)> = [(Route::Library {}, "Library"), (Route::Eq {}, "EQ"), (Route::Lyrics {}, "Lyrics"), (Route::Settings {}, "Settings")]
        .into_iter()
        .filter(|(_, label)| match *label {
            "EQ" => features.audio,
            "Lyrics" => features.lyrics_ai,
            _ => true,
        })
        .collect();

    rsx! {
        header { class: "navbar",
            div { class: "navbar__inner",
                Link {
                    class: "navbar__brand",
                    to: Route::Library {},
                    title: "Home",
                    onclick: move |_| {
                        home += 1;
                        document::eval("tqLibForget(); window.scrollTo({ top: 0, behavior: 'smooth' })");
                    },
                    "Tanqi"
                }
                nav { class: "navbar__nav",
                    for (route, label) in links.clone() {
                        Link {
                            key: "{label}",
                            class: if is_active(&current, &route) { "navbar__link navbar__link--active" } else { "navbar__link" },
                            to: route,
                            "{label}"
                        }
                    }
                }
                div { class: "navbar__right",
                    ThemeToggle {}
                    button {
                        class: "navbar__mini navbar__mini--window",
                        title: "Shrink to a small always-on-top player",
                        onclick: move |_| compact.enter(CompactWidget::Music),
                        ExpandIcon { size: 12 }
                        "Mini player"
                    }
                }
            }
        }
        // Phone: the same destinations as a bottom tab bar (the top links hide below 960px).
        nav { class: "tabbar", "aria-label": "Main",
            for (route, label) in links {
                Link {
                    key: "{label}",
                    class: if is_active(&current, &route) { "tabbar__tab tabbar__tab--active" } else { "tabbar__tab" },
                    to: route,
                    {
                        match label {
                            "Library" => rsx! { LibraryIcon { size: 20 } },
                            "EQ" => rsx! { SlidersIcon { size: 20 } },
                            "Lyrics" => rsx! { LyricsIcon { size: 20 } },
                            _ => rsx! { GearIcon { size: 20 } },
                        }
                    }
                    span { class: "tabbar__label", "{label}" }
                }
            }
        }
    }
}
