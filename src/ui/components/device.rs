//! Parts shared by the mini player and the player bar, which are the same
//! device at two sizes (see assets/css/device.css).

use crate::ui::player::use_player;
use dioxus::prelude::*;

/// Etched tick marks around a platter's rim, as `(x1, y1, x2, y2)` in a
/// 100x100 SVG space: a short tick every 5 degrees, a longer one every 30.
pub fn rim_ticks() -> Vec<(f32, f32, f32, f32)> {
    (0..72)
        .map(|i| {
            let angle = (i as f32 * 5.0).to_radians();
            let (outer, inner) = (47.2, if i % 6 == 0 { 45.2 } else { 46.1 });
            (50.0 + outer * angle.cos(), 50.0 + outer * angle.sin(), 50.0 + inner * angle.cos(), 50.0 + inner * angle.sin())
        })
        .collect()
}

/// The master volume fader: two slim vertical grooves with a hairline cursor
/// across both, driving (and reflecting) the player's volume. Muting parks it
/// at zero. Sized by `--fw` / `--fh` on itself or an ancestor.
#[component]
pub fn VolumeFader() -> Element {
    let player = use_player();
    let (volume, muted) = ((player.volume)(), (player.muted)());
    let level = if muted { 0.0 } else { volume };
    let text = if muted { "Muted".to_string() } else { format!("Volume {}%", (volume * 100.0).round() as i32) };

    rsx! {
        div { class: "dev-fader", style: "--lvl: {level}",
            span { class: "dev-fader__groove dev-fader__groove--l" }
            span { class: "dev-fader__groove dev-fader__groove--r" }
            span { class: "dev-fader__cursor" }
            input {
                r#type: "range",
                min: "0",
                max: "1",
                step: "0.01",
                value: "{level}",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f32>() {
                        player.set_volume(v);
                    }
                },
                "aria-label": "Volume",
                "aria-orientation": "vertical",
                "aria-valuetext": "{text}",
                title: "{text}",
                class: "dev-fader__range",
            }
        }
    }
}

/// How far the knob sweeps: from 7 o'clock (silent) round to 5 o'clock (full).
const KNOB_START_DEG: f64 = -135.0;
const KNOB_SWEEP_DEG: f64 = 270.0;
/// Tick marks round the knob, lit up to the current level.
const KNOB_TICKS: usize = 21;

/// The master volume as a rotary knob: turn it by dragging round it, with the
/// wheel or the arrow keys. A pointer notch shows where it is, the ticks round
/// it light up to the level and the cap's tooltip reads the volume out. Muting shows it
/// at zero (the speaker button un-mutes), like the fader it replaced.
#[component]
pub fn VolumeKnob() -> Element {
    let player = use_player();
    let (volume, muted) = ((player.volume)(), (player.muted)());
    let level = if muted { 0.0 } else { volume as f64 };
    let percent = (level * 100.0).round() as i32;
    let deg = KNOB_START_DEG + KNOB_SWEEP_DEG * level;
    let text = if muted { "Muted".to_string() } else { format!("Volume {percent}%") };

    let mut mounted = use_signal(|| None::<std::rc::Rc<MountedData>>);
    // While turning: the knob's centre in window coordinates.
    let mut turning = use_signal(|| None::<(f64, f64)>);

    // The pointer's angle round the centre, 0 = straight up and clockwise
    // positive, becomes a level; the dead zone at the bottom snaps to the
    // nearer end.
    let aim = move |x: f64, y: f64, cx: f64, cy: f64| {
        let angle = (x - cx).atan2(cy - y).to_degrees().clamp(KNOB_START_DEG, KNOB_START_DEG + KNOB_SWEEP_DEG);
        player.set_volume(((angle - KNOB_START_DEG) / KNOB_SWEEP_DEG) as f32);
    };
    let step = move |delta: f32| player.set_volume((level as f32 + delta).clamp(0.0, 1.0));

    let ticks = (0..KNOB_TICKS).map(|i| {
        let t = i as f64 / (KNOB_TICKS - 1) as f64;
        let rad = (KNOB_START_DEG + KNOB_SWEEP_DEG * t - 90.0).to_radians();
        let (x1, y1, x2, y2) = (50.0 + 49.0 * rad.cos(), 50.0 + 49.0 * rad.sin(), 50.0 + 43.0 * rad.cos(), 50.0 + 43.0 * rad.sin());
        (i, x1, y1, x2, y2, t <= level + 1e-9 && level > 0.0)
    });

    rsx! {
        div {
            class: "dev-knob",
            tabindex: "0",
            role: "slider",
            title: "{text}",
            "aria-label": "Volume",
            "aria-valuemin": "0",
            "aria-valuemax": "100",
            "aria-valuenow": "{percent}",
            "aria-valuetext": "{text}",
            style: "--deg: {deg}deg",
            onmounted: move |e| mounted.set(Some(e.data())),
            onmousedown: move |e| {
                let p = e.client_coordinates();
                let Some(m) = mounted() else { return };
                spawn(async move {
                    if let Ok(r) = m.get_client_rect().await {
                        let (cx, cy) = (r.origin.x + r.size.width / 2.0, r.origin.y + r.size.height / 2.0);
                        aim(p.x, p.y, cx, cy);
                        turning.set(Some((cx, cy)));
                    }
                });
            },
            onwheel: move |e| {
                let dy = e.delta().strip_units().y;
                step(if dy < 0.0 { 0.03 } else { -0.03 });
            },
            onkeydown: move |e| match e.key() {
                Key::ArrowUp | Key::ArrowRight => step(0.02),
                Key::ArrowDown | Key::ArrowLeft => step(-0.02),
                Key::PageUp => step(0.1),
                Key::PageDown => step(-0.1),
                Key::Home => step(-1.0),
                Key::End => step(1.0),
                _ => {}
            },
            svg { class: "dev-knob__ticks", view_box: "0 0 100 100", "aria-hidden": "true",
                for (i, x1, y1, x2, y2, on) in ticks {
                    line {
                        key: "{i}",
                        class: if on { "dev-knob__tick dev-knob__tick--on" } else { "dev-knob__tick" },
                        x1: "{x1:.2}", y1: "{y1:.2}", x2: "{x2:.2}", y2: "{y2:.2}",
                    }
                }
            }
            div { class: "dev-knob__cap",
                div { class: "dev-knob__pointer" }
            }
            // Dragging off the knob keeps turning it: a see-through layer over the window catches the mouse.
            if let Some((cx, cy)) = turning() {
                div {
                    class: "dev-knob__grab",
                    onmousemove: move |e| {
                        let p = e.client_coordinates();
                        aim(p.x, p.y, cx, cy);
                    },
                    onmouseup: move |_| turning.set(None),
                    onmouseleave: move |_| turning.set(None),
                }
            }
        }
    }
}
