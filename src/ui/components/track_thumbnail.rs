use crate::ui::util::resolve_thumb;
use dioxus::prelude::*;

/// What a thumbnail stands for, which decides its placeholder art.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ThumbKind {
    /// An album or track: a record sleeve with its vinyl sliding out.
    #[default]
    Album,
    /// An artist: a person.
    Artist,
}

/// Renders a thumbnail the same way everywhere it's shown (local `local:`
/// covers resolve through the `tq://` file protocol - see util::resolve_thumb).
/// With no thumbnail it shows a placeholder: `fallback` if given, otherwise a
/// bold icon for what it stands for (`kind`).
#[component]
pub fn TrackThumbnail(url: Option<String>, class: String, fallback: Option<Element>, kind: Option<ThumbKind>) -> Element {
    match resolve_thumb(url.as_deref()) {
        None => rsx! {
            div { class: "{class} track-thumbnail--placeholder",
                if let Some(fallback) = fallback {
                    {fallback}
                } else {
                    Glyph { kind: kind.unwrap_or_default() }
                }
            }
        },
        Some(src) => rsx! {
            img { src: "{src}", alt: "", class: "{class}", loading: "lazy", decoding: "async" }
        },
    }
}

/// The placeholder art: heavy solid shapes in the spirit of a hardware
/// maker's catalogue icons. Drawn in `currentColor` so each theme sets its
/// ink; `--glyph-ground` is the placeholder's own background, for the parts
/// cut out of the solid shapes.
#[component]
fn Glyph(kind: ThumbKind) -> Element {
    rsx! {
        svg { class: "track-thumbnail__glyph", view_box: "0 0 100 100", fill: "none", "aria-hidden": "true",
            match kind {
                // A person: a head and a pair of shoulders.
                ThumbKind::Artist => rsx! {
                    g { fill: "currentColor",
                        circle { cx: "50", cy: "34", r: "18" }
                        path { d: "M16 88 C16 64 31 54 50 54 C69 54 84 64 84 88 Z" }
                    }
                },
                // A record sleeve with the vinyl sliding out of its right edge.
                ThumbKind::Album => rsx! {
                    circle { cx: "66", cy: "50", r: "30", fill: "currentColor" }
                    circle { cx: "66", cy: "50", r: "19", stroke: "var(--glyph-ground, #000)", stroke_width: "2" }
                    circle { cx: "66", cy: "50", r: "7", fill: "var(--glyph-ground, #000)" }
                    rect { x: "8", y: "22", width: "46", height: "56", fill: "var(--glyph-ground, #000)", stroke: "currentColor", stroke_width: "9" }
                },
            }
        }
    }
}
