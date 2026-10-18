//! A track's Suggested EQ at the chosen strength: the one place the scaling, the preamp and the engine's
//! band JSON are derived, so the player and the analysis window cannot disagree.

use app_lib::models::{AudioAnalysis, EqBand};
use serde_json::{json, Value};

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Suggested {
    pub bands: Vec<EqBand>,
    pub preamp: f32,
}

impl Suggested {
    /// The analysis's suggestion at `strength` (0.0-1.0).
    pub fn of(analysis: &AudioAnalysis, strength: f32) -> Self {
        let (bands, preamp) = app_lib::audio::scale_suggestion(&analysis.eq_bands, strength);
        Suggested { bands, preamp }
    }

    /// `(freq, gain dB, q)` per band.
    pub fn triples(&self) -> impl Iterator<Item = (f32, f32, f32)> + '_ {
        self.bands.iter().map(|b| (b.hz, b.suggested_gain_db, b.q))
    }

    /// The engine's `setEq` payload.
    pub fn engine_json(&self, enabled: bool) -> Value {
        let bands: Vec<Value> = self.triples().map(|(freq, gain, q)| json!({"freq": freq, "gain": gain, "q": q})).collect();
        json!({"bands": bands, "preampDb": self.preamp, "enabled": enabled})
    }
}
