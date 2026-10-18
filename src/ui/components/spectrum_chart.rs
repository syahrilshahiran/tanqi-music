use app_lib::audio::ChartData;
use dioxus::prelude::*;

const W: f32 = 320.0;
const H: f32 = 124.0;
const LEFT: f32 = 30.0;
const RIGHT: f32 = 8.0;
const TOP: f32 = 8.0;
const BOTTOM: f32 = 20.0;
/// The chart shows +/- this many dB around the reference.
const SPAN_DB: f32 = 20.0;

fn x_of_band(i: f32, bands: usize) -> f32 {
    LEFT + (i / (bands as f32 - 1.0)) * (W - LEFT - RIGHT)
}

fn y_of_db(db: f32) -> f32 {
    let plot = H - TOP - BOTTOM;
    TOP + plot / 2.0 - db.clamp(-SPAN_DB, SPAN_DB) / SPAN_DB * (plot / 2.0)
}

/// Third-octave band position of a frequency (the bands are spaced 1/3 octave apart from 25 Hz).
fn band_pos(hz: f32) -> f32 {
    3.0 * (hz / 25.0).log2()
}

fn points(pts: impl Iterator<Item = (f32, f32)>) -> String {
    pts.map(|(x, y)| format!("{x:.1},{y:.1}")).collect::<Vec<_>>().join(" ")
}

/// This track against how music usually sounds: its spectrum as a line (0 = the usual level at each
/// frequency), the normal range shaded, and the suggested EQ's response dashed over it.
#[component]
pub fn SpectrumChart(data: ChartData, strength_pct: u32) -> Element {
    let n = data.centers_hz.len();
    let zero = y_of_db(0.0);

    // The normal range, as a polygon: along the top edge then back along the bottom.
    let top = points((0..n).map(|i| (x_of_band(i as f32, n), y_of_db(data.range_db[i]))));
    let bottom = points((0..n).rev().map(|i| (x_of_band(i as f32, n), y_of_db(-data.range_db[i]))));
    let range = format!("{top} {bottom}");

    let eq = points((0..n).map(|i| (x_of_band(i as f32, n), y_of_db(data.eq_db[i]))));

    // The track's line, broken wherever a band has no measurement.
    let mut runs: Vec<String> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    for i in 0..n {
        match data.deviation_db[i] {
            Some(d) => current.push((x_of_band(i as f32, n), y_of_db(d))),
            None => {
                if !current.is_empty() {
                    runs.push(points(current.drain(..)));
                }
            }
        }
    }
    if !current.is_empty() {
        runs.push(points(current.drain(..)));
    }

    let cutoff_x = data.lowpass_hz.map(|hz| x_of_band(band_pos(hz).clamp(0.0, n as f32 - 1.0), n));
    let bottom_y = H - BOTTOM;
    let label_y = H - 6.0;

    rsx! {
        div { class: "sc",
            svg {
                class: "sc__svg",
                view_box: "0 0 {W} {H}",
                role: "img",
                "aria-label": "This track's spectrum compared with how music usually sounds, with the suggested EQ",
                // Grid: the 0 line, +/-10 dB, and 100 Hz, 1 kHz, 10 kHz.
                for db in [-10.0f32, 10.0] {
                    line { class: "sc__grid", x1: "{LEFT}", x2: "{W - RIGHT}", y1: "{y_of_db(db)}", y2: "{y_of_db(db)}" }
                }
                for (hz, label) in [(100.0f32, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
                    {
                        let x = x_of_band(band_pos(hz), n);
                        rsx! {
                            line { class: "sc__grid", x1: "{x}", x2: "{x}", y1: "{TOP}", y2: "{bottom_y}" }
                            text { class: "sc__label", x: "{x}", y: "{label_y}", text_anchor: "middle", "{label}" }
                        }
                    }
                }
                text { class: "sc__label", x: "{LEFT - 4.0}", y: "{y_of_db(10.0) + 3.0}", text_anchor: "end", "+10" }
                text { class: "sc__label", x: "{LEFT - 4.0}", y: "{zero + 3.0}", text_anchor: "end", "0" }
                text { class: "sc__label", x: "{LEFT - 4.0}", y: "{y_of_db(-10.0) + 3.0}", text_anchor: "end", "-10" }

                if let Some(x) = cutoff_x {
                    rect { class: "sc__cutoff", x: "{x}", y: "{TOP}", width: "{W - RIGHT - x}", height: "{bottom_y - TOP}" }
                    text { class: "sc__label", x: "{x - 3.0}", y: "{TOP + 8.0}", text_anchor: "end", "lossy cut-off" }
                }

                polygon { class: "sc__range", points: "{range}" }
                line { class: "sc__zero", x1: "{LEFT}", x2: "{W - RIGHT}", y1: "{zero}", y2: "{zero}" }
                polyline { class: "sc__eq", points: "{eq}" }
                for (k, run) in runs.iter().enumerate() {
                    polyline { key: "{k}", class: "sc__track", points: "{run}" }
                }
            }
            div { class: "sc__legend",
                span { class: "sc__key sc__key--track", "This track" }
                span { class: "sc__key sc__key--range", "Usual range" }
                span { class: "sc__key sc__key--eq", "Suggested EQ at {strength_pct}%" }
            }
        }
    }
}
