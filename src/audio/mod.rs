//! Audio files on disk: what they sound like (`analyze_band_levels`) and what
//! they say about themselves (`read_local_track_tags`).
//!
//! Analysis is a pipeline of independent steps - decode to mono samples,
//! measure the spectrum, detect tempo and key, turn the measurement into a
//! suggested EQ - each in its own private module over plain numbers; this
//! module only joins them. Tag reading shares nothing with it but the
//! decoder library.

mod decode;
mod eq;
mod loudness;
mod spectrum;
mod tags;
mod tempo;
#[cfg(test)]
mod tests;

pub use loudness::{normalisation_gain_db, Loudness, TARGET_LUFS};
pub use eq::{chart_data, verdict, ChartData, Tone, Verdict, combined_peak_db, preamp_for_bands, scale_suggestion, MAX_SUGGESTED_GAIN_DB};
pub use tags::{read_local_track_tags, EmbeddedCover, LocalTrackTags};

use crate::models::{AudioAnalysis, EqBand};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandLevels {
    pub bass_db: f32,
    pub mid_db: f32,
    pub treble_db: f32,
    pub overall_db: f32,
    /// Integrated loudness (LUFS) and true peak (dBTP), BS.1770; `None` for a file with nothing audible.
    pub loudness_lufs: Option<f32>,
    pub true_peak_db: Option<f32>,
    /// The measured spectrum (one level per third-octave band, `None` where unusable) and the cut-off of a
    /// lossy encoder if one was found (Hz).
    pub spectrum_db: Vec<Option<f32>>,
    pub lowpass_hz: Option<f32>,
    /// However many bands actually stand out from this track's own tilted
    /// reference curve, each at its own measured frequency/gain/Q (see
    /// `eq::suggest`) - not a fixed count or fixed layout. A well-mixed,
    /// already-flat track can legitimately come back with very few bands
    /// (or none at all).
    pub eq_bands: Vec<EqBand>,
    /// Suggested preamp adjustment - just enough negative headroom to keep
    /// the loudest suggested boost from clipping, 0 if nothing's boosted.
    pub preamp_db: f32,
    /// Estimated tempo in beats per minute (see `tempo::detect`) - `None`
    /// when the track is too short to analyze or has no discernible
    /// rhythmic pulse (e.g. a sustained tone or near-silence), not when
    /// detection merely failed.
    pub bpm: Option<f32>,
    /// Detected musical key in shorthand ("C", "Am", "F#m"), or `None` when
    /// the detection wasn't confident enough to report - see
    /// `MIN_KEY_CONFIDENCE`.
    pub key: Option<String>,
}

impl BandLevels {
    /// The saved form of this measurement, snapshotting the track's
    /// `title`/`artist`/`thumbnail_url` so it lists properly whether or not
    /// the track is still in the library.
    pub fn into_analysis(
        self,
        track_id: String,
        title: String,
        artist: String,
        thumbnail_url: Option<String>,
    ) -> AudioAnalysis {
        AudioAnalysis {
            track_id,
            bass_db: self.bass_db,
            mid_db: self.mid_db,
            treble_db: self.treble_db,
            overall_db: self.overall_db,
            loudness_lufs: self.loudness_lufs,
            true_peak_db: self.true_peak_db,
            spectrum_db: self.spectrum_db,
            lowpass_hz: self.lowpass_hz,
            analysis_version: crate::models::ANALYSIS_VERSION,
            eq_bands: self.eq_bands,
            preamp_db: self.preamp_db,
            bpm: self.bpm,
            key: self.key,
            title,
            artist,
            thumbnail_url,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// The Suggested EQ and preamp for an analysis, worked out now from its stored spectrum against the current
/// reference. An analysis from before the spectrum was stored keeps the bands it was saved with.
pub fn with_current_suggestion(mut a: AudioAnalysis) -> AudioAnalysis {
    if !a.spectrum_db.is_empty() {
        a.eq_bands = eq::suggest(&a.spectrum_db);
        a.preamp_db = eq::preamp_for(&a.eq_bands);
    }
    a
}

/// How a track's balance leans, for filtering: from its spectrum against the reference, or for an analysis
/// from before the spectrum was stored, from its bass-to-treble gap. `None` when balanced.
pub fn tone(a: &AudioAnalysis) -> Option<Tone> {
    match eq::verdict(&a.spectrum_db) {
        Some(v) => v.tone,
        None => {
            let diff = a.bass_db - a.treble_db;
            if diff > 8.0 {
                Some(Tone::Bassy)
            } else if diff < -8.0 {
                Some(Tone::Bright)
            } else {
                None
            }
        }
    }
}

/// Decodes an in-memory audio file (any symphonia-supported container/codec),
/// downmixes it to mono, and measures its spectrum, tempo and key, with an EQ
/// suggested from the spectrum.
pub fn analyze_band_levels(bytes: Vec<u8>) -> Result<BandLevels, String> {
    analyze_source(Box::new(std::io::Cursor::new(bytes)), tempo::MAX_SECONDS_FOR_TEMPO)
}

/// Analyses the audio file at `path`, reading it as it decodes: unlike `analyze_band_levels` the whole file is
/// never held in memory. CPU-heavy: call it from a blocking task, not an async one.
pub fn analyze_file(path: &str) -> Result<BandLevels, String> {
    analyze_file_with_excerpt(path, tempo::MAX_SECONDS_FOR_TEMPO)
}

/// `analyze_file` with the length of the middle part tempo and key are read from, in seconds (for comparing
/// lengths: memory grows with it).
pub fn analyze_file_with_excerpt(path: &str, excerpt_seconds: f32) -> Result<BandLevels, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    analyze_source(Box::new(file), excerpt_seconds)
}

fn analyze_source(source: Box<dyn symphonia::core::io::MediaSource>, excerpt_seconds: f32) -> Result<BandLevels, String> {
    // Loudness and the spectrum are measured per channel while the file decodes; of the mono mix only the
    // middle of the track is kept, for tempo and key.
    let mut meter: Option<Option<loudness::Meter>> = None;
    let mut builder: Option<spectrum::SpectrumBuilder> = None;
    let decoded = decode::decode_source(source, excerpt_seconds, &mut |interleaved, channels, rate| {
        if let Some(m) = meter.get_or_insert_with(|| loudness::Meter::new(channels as u32, rate)) {
            m.add_interleaved(interleaved);
        }
        builder
            .get_or_insert_with(|| spectrum::SpectrumBuilder::new(rate, channels))
            .push_interleaved(interleaved);
    })?;
    let (samples, sample_rate) = (decoded.excerpt, decoded.sample_rate);
    if decoded.frames < spectrum::FFT_SIZE {
        return Err("decoded audio is too short to analyze".to_string());
    }
    let spectrum = builder.ok_or("no audio decoded")?.finish();
    let loudness = meter.flatten().and_then(|m| m.finish());
    let tempo = tempo::detect(&samples, sample_rate, excerpt_seconds);
    let eq_bands = eq::suggest(&spectrum.fine_db);
    Ok(BandLevels {
        bass_db: spectrum.bass_db,
        mid_db: spectrum.mid_db,
        treble_db: spectrum.treble_db,
        overall_db: spectrum.overall_db,
        loudness_lufs: loudness.map(|l| l.lufs),
        true_peak_db: loudness.map(|l| l.true_peak_db),
        lowpass_hz: spectrum.lowpass_hz,
        spectrum_db: spectrum.fine_db.clone(),
        preamp_db: eq::preamp_for(&eq_bands),
        eq_bands,
        bpm: tempo.bpm,
        key: tempo.key,
    })
}
