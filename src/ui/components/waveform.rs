use crate::ui::{player::use_player, util::format_seconds};
use dioxus::prelude::*;

/// Unplayed/played layers render the very same bars; as a memoized child
/// (props compare equal while the peaks don't change) the ~20Hz position
/// updates re-render only the Waveform's own cheap parts, not 200 divs.
#[component]
fn Bars(peaks: Vec<f32>) -> Element {
    rsx! {
        for (i, peak) in peaks.iter().enumerate() {
            div {
                key: "{i}",
                class: "waveform__bar",
                style: "height: {(peak * 100.0).max(8.0)}%",
            }
        }
    }
}

/// A real waveform (not a decorative one) with a click/drag-to-seek range
/// input overlaid invisibly on top - the waveform bars are purely visual,
/// the actual interaction still goes through a native range input so
/// dragging/keyboard-seeking behavior doesn't need reimplementing.
///
/// `peaks`: normalized (0-1) peak amplitude per bar - empty renders a plain
/// flat line instead (e.g. before a track has finished decoding).
///
/// Subscribes to the playback position itself rather than taking it as a
/// prop: it is the only thing on screen that renders the position, so
/// keeping the subscription here means a tick re-renders this component
/// alone instead of everything that contains it.
#[component]
pub fn Waveform(peaks: Vec<f32>, duration: f64, on_seek: EventHandler<f64>, set_seeking: EventHandler<bool>) -> Element {
    let player = use_player();
    let current_time = (player.position)();
    // While dragging, the displayed position follows the drag directly
    // rather than the (still-actually-playing) player position - the real
    // seek only commits on release. Committing on every drag tick would
    // restart playback that many times, which both sounds bad and fights the
    // waveform's own progress fill visually.
    let mut drag_value = use_signal(|| None::<f64>);
    let display_time = drag_value().unwrap_or(current_time);
    let progress_pct = if duration > 0.0 { (display_time / duration * 100.0).min(100.0) } else { 0.0 };
    let clip_right = 100.0 - progress_pct;
    let has_peaks = !peaks.is_empty();
    let range_value = display_time.min(duration);

    let mut commit = move |value: f64| {
        set_seeking.call(false);
        drag_value.set(None);
        on_seek.call(value);
    };
    // mouseup/touchend/keyup: the native `change` event only fires when the
    // value actually moved, so a press-and-release in place would otherwise
    // leave the engine in "seeking" mode forever.
    let mut release = move || {
        let pending = *drag_value.peek();
        match pending {
            Some(v) => commit(v),
            None => set_seeking.call(false),
        }
    };

    rsx! {
        span { class: "waveform__time waveform__time--elapsed",
            "{format_seconds(display_time)}"
        }

        div { class: "waveform__track",
            if has_peaks {
                // Unplayed layer - full waveform, muted color.
                div { class: "waveform__layer waveform__layer--unplayed",
                    Bars { peaks: peaks.clone() }
                }
                // Played layer - identical bars (same container, same
                // children, so positions match exactly), revealed only up to
                // playback progress via clip-path. Clipping (vs. resizing the
                // container) is what keeps this a single cheap style update
                // per tick instead of 100 bars reflowing.
                div {
                    class: "waveform__layer waveform__layer--played",
                    style: "clip-path: inset(0 {clip_right}% 0 0)",
                    Bars { peaks: peaks.clone() }
                }
            } else {
                div { class: "waveform__layer",
                    div { class: "waveform__flat" }
                }
            }

            input {
                r#type: "range",
                min: "0",
                max: "{duration}",
                step: "0.1",
                value: "{range_value}",
                onmousedown: move |_| set_seeking.call(true),
                ontouchstart: move |_| set_seeking.call(true),
                onkeydown: move |_| set_seeking.call(true),
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        drag_value.set(Some(v));
                    }
                },
                onchange: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        commit(v);
                    }
                },
                onmouseup: move |_| release(),
                ontouchend: move |_| release(),
                onkeyup: move |_| release(),
                disabled: duration == 0.0,
                class: "waveform__range",
            }
        }

        span { class: "waveform__time",
            "{format_seconds(duration)}"
        }
    }
}
