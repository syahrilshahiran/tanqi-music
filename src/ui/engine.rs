//! The audio engine (`assets/engine.js`) as the player sees it: commands go out through `cmd`, events come
//! back through the player's event handler.

use dioxus::prelude::*;
use serde_json::{json, Value};

/// Sends one command to the engine.
pub fn cmd(name: &str, args: Value) {
    document::eval(&format!("tqEngine.cmd({}, {})", json!(name), args));
}

/// The engine gain for a volume slider position (squared for a perceptual curve).
pub fn volume_gain(volume: f32, muted: bool) -> f32 {
    if muted {
        0.0
    } else {
        let v = volume.clamp(0.0, 1.0);
        v * v
    }
}

/// The waveform peaks the engine reports for a loaded track.
pub fn waveform_from(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
}
