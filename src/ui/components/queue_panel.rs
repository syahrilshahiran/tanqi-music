use crate::ui::{
    components::icons::{ChevronDownIcon, ChevronUpIcon, CloseIcon, QueueIcon, TrashIcon},
    player::use_player,
};
use dioxus::prelude::*;

/// The play queue as a popover above the player bar: jump to, reorder or
/// remove entries, or clear everything but the playing track.
#[component]
pub fn QueuePanel() -> Element {
    let mut open = use_signal(|| false);

    rsx! {
        div { class: "player-bar__queue",
            button {
                onclick: move |_| open.set(!open()),
                title: "Queue",
                "aria-label": "Queue",
                "aria-expanded": "{open()}",
                class: "dev-tag",
                QueueIcon { size: 14 }
            }
            if open() {
                div { class: "queue-panel",
                    QueueList { on_close: move |_| open.set(false) }
                }
            }
        }
    }
}

/// The queue itself: its heading with clear and close, and the list. Shown in a popover by `QueuePanel`
/// and over the mini player by itself.
#[component]
pub fn QueueList(on_close: EventHandler<()>) -> Element {
    let player = use_player();
    let queue = (player.queue)();
    let current = (player.current_index)();
    let last = queue.len().saturating_sub(1);

    rsx! {
            div { class: "queue-panel__head",
                span { class: "section-label", "Queue · {queue.len()}" }
                button { class: "dev-tag", title: "Clear the queue", "aria-label": "Clear the queue", onclick: move |_| player.clear_queue(), TrashIcon { size: 14 } }
                button {
                    class: "dev-tag",
                    title: "Close",
                    "aria-label": "Close queue",
                    onclick: move |_| on_close.call(()),
                    CloseIcon { size: 12 }
                }
            }
            if (player.shuffle)() {
                p { class: "queue-panel__note", "Shuffle is on: playback order differs from this list." }
            }
            ol { class: "queue-panel__list",
                for (i, t) in queue.iter().enumerate() {
                    li {
                        key: "{i}-{t.id}",
                        class: if current == Some(i) { "queue-panel__row queue-panel__row--current" } else { "queue-panel__row" },
                        span { class: "queue-panel__num", "{i + 1:02}" }
                        button {
                            class: "queue-panel__body",
                            title: "Play",
                            onclick: move |_| player.play_index(i),
                            span { class: "queue-panel__title truncate", "{t.title}" }
                            span { class: "queue-panel__artist truncate", "{t.artist}" }
                        }
                        button {
                            class: "queue-panel__btn",
                            title: "Move up",
                            "aria-label": "Move up",
                            disabled: i == 0,
                            onclick: move |_| player.move_in_queue(i, -1),
                            ChevronUpIcon { size: 12 }
                        }
                        button {
                            class: "queue-panel__btn",
                            title: "Move down",
                            "aria-label": "Move down",
                            disabled: i == last,
                            onclick: move |_| player.move_in_queue(i, 1),
                            ChevronDownIcon { size: 12 }
                        }
                        button {
                            class: "queue-panel__btn",
                            title: "Remove",
                            "aria-label": "Remove",
                            disabled: current == Some(i),
                            onclick: move |_| player.remove_from_queue(i),
                            CloseIcon { size: 12 }
                        }
                    }
                }
            }

    }
}
