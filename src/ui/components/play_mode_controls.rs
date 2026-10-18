use crate::ui::{
    components::icons::{RepeatIcon, ShuffleIcon},
    player::{use_player, RepeatMode},
};
use dioxus::prelude::*;

/// Shuffle + repeat tags for PlayerBar, as icons (repeat-one carries a small 1). The mini player has its own etched
/// Shuf/Rpt marks that drive the same player state.
#[component]
pub fn PlayModeControls() -> Element {
    let player = use_player();
    let (shuffle, repeat) = ((player.shuffle)(), (player.repeat)());

    let repeat_title = match repeat {
        RepeatMode::Off => "Repeat off - click to repeat the whole queue",
        RepeatMode::All => "Repeating the queue - click to repeat this track",
        RepeatMode::One => "Repeating this track - click to turn repeat off",
    };

    rsx! {
        button {
            onclick: move |_| player.toggle_shuffle(),
            title: if shuffle { "Shuffle on - click to play in order" } else { "Shuffle off - click to shuffle" },
            "aria-label": "Shuffle",
            "aria-pressed": "{shuffle}",
            class: "dev-tag",
            span { class: "tag-icon", ShuffleIcon { size: 18 } }
        }
        button {
            onclick: move |_| player.cycle_repeat(),
            title: "{repeat_title}",
            "aria-label": "Repeat",
            "aria-pressed": "{repeat != RepeatMode::Off}",
            class: "dev-tag",
            span { class: "tag-icon",
                RepeatIcon { size: 18 }
                if repeat == RepeatMode::One {
                    span { class: "tag-icon__one", "1" }
                }
            }
        }
    }
}
