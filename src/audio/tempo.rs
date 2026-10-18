//! Tempo and musical key, via stratum-dsp.

/// Below this many seconds of audio there aren't enough beat repetitions
/// for stratum-dsp's tempogram/autocorrelation to lock onto a period with
/// any confidence - cheap to reject up front rather than running the full
/// analysis pipeline on a clip that's too short to say anything about.
const MIN_SECONDS_FOR_TEMPO: f32 = 5.0;

/// Tempo and key are read from at most this much audio, taken from the middle
/// of the track. stratum-dsp builds spectrogram tables the size of what it is
/// given, so a whole long song ran a phone out of memory (the app aborted). A
/// minute and a half holds plenty of beats and harmony, and the middle skips
/// a quiet intro and outro.
pub(super) const MAX_SECONDS_FOR_TEMPO: f32 = 90.0;

/// The middle `max_seconds` of `samples`, or all of them if shorter.
fn excerpt(samples: &[f32], sample_rate: u32, max_seconds: f32) -> &[f32] {
    let max = (sample_rate as f32 * max_seconds) as usize;
    if samples.len() <= max {
        return samples;
    }
    let start = (samples.len() - max) / 2;
    &samples[start..start + max]
}

/// Hop size (in samples) for the coarse loudness envelope `detect`
/// pre-checks before handing anything to stratum-dsp.
const TEMPO_HOP_SIZE: usize = 256;

/// A real beat needs the track's loudness to actually swing over time
/// (quiet-then-loud attacks); a sustained tone or steady drone barely moves
/// at all frame-to-frame. Calibrated against real songs, not just the
/// synthetic click-track tests below: a loudness-mastered real track's
/// coefficient of variation lands far lower than a synthetic click's
/// near-silent gaps between clicks (confirmed against 3 real songs - a
/// clearly-beated pop track measured ~0.46-0.5, well under an initial 0.5
/// cutoff that was silently rejecting it) while a genuinely non-rhythmic
/// signal (the sustained-tone test below) measures two orders of magnitude
/// lower still (~0.008) - leaving a wide, safe margin at this value.
const MIN_LOUDNESS_VARIATION: f32 = 0.15;

/// Cheap pre-filter for `detect`: does this audio's loudness actually
/// swing enough over time to plausibly contain a beat at all? Needed
/// because stratum-dsp's own confidence score doesn't reliably separate "no
/// rhythm whatsoever" (a sustained tone) from "a real but musically atonal
/// rhythm" - both read as low-confidence/weak-tonality to it, since that
/// score is tuned against real, tonal DJ tracks rather than this kind of
/// edge case.
fn has_rhythmic_energy_variation(samples: &[f32]) -> bool {
    let energy: Vec<f32> = samples
        .chunks(TEMPO_HOP_SIZE)
        .map(|chunk| {
            (chunk.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / chunk.len() as f64).sqrt() as f32
        })
        .collect();
    if energy.len() < 8 {
        return false;
    }

    let mean_energy: f32 = energy.iter().sum::<f32>() / energy.len() as f32;
    if mean_energy <= 0.0 {
        return false; // true digital silence
    }
    let energy_variance: f32 =
        energy.iter().map(|e| (e - mean_energy).powi(2)).sum::<f32>() / energy.len() as f32;
    energy_variance.sqrt() / mean_energy >= MIN_LOUDNESS_VARIATION
}

/// Below this, stratum-dsp's own key confidence reads as guesswork rather
/// than a real reading - the crate documents <0.3 as "may be unreliable",
/// and it already folds tonal clarity into that score. Tempo has no
/// equivalent gate because it's both far more accurate here and far less
/// embarrassing to get slightly wrong.
const MIN_KEY_CONFIDENCE: f32 = 0.3;

/// Tempo and musical key together, because `stratum_dsp::analyze_audio`
/// computes both in one pass - asking for them separately would run the
/// entire (~70ms/track) analysis twice for no benefit.
#[derive(Default)]
pub(super) struct TempoAndKey {
    pub bpm: Option<f32>,
    /// Musical shorthand - "C", "F#", "Am", "D#m" - or `None` when the
    /// reading wasn't confident enough to be worth showing.
    pub key: Option<String>,
}

/// Estimates tempo via `stratum-dsp` - a purpose-built, professional-grade
/// BPM/beat-tracking engine (dual FFT+autocorrelation tempogram, HMM beat
/// grid, multi-resolution octave-error correction; validated at 87.7%
/// accuracy within ±2 BPM on 155 real-world DJ tracks - see the crate's own
/// docs for algorithm references) - rather than the hand-rolled
/// single-lag-autocorrelation heuristic this used to be.
pub(super) fn detect(samples: &[f32], sample_rate: u32, max_seconds: f32) -> TempoAndKey {
    if (samples.len() as f32) < sample_rate as f32 * MIN_SECONDS_FOR_TEMPO {
        return TempoAndKey::default();
    }
    let samples = excerpt(samples, sample_rate, max_seconds);
    if !has_rhythmic_energy_variation(samples) {
        // No rhythm to find. Key detection doesn't depend on rhythm, but
        // this same gate catches silence and pure tones, which have no
        // meaningful key either.
        return TempoAndKey::default();
    }

    let Ok(result) =
        stratum_dsp::analyze_audio(samples, sample_rate, stratum_dsp::AnalysisConfig::default())
    else {
        return TempoAndKey::default();
    };
    let confidence = stratum_dsp::compute_confidence(&result);

    TempoAndKey {
        // `bpm <= 0.0` is stratum-dsp's own "detection failed" signal.
        bpm: if result.bpm > 0.0 { Some(result.bpm) } else { None },
        // Unlike tempo, key detection is genuinely unreliable on weakly
        // tonal or heavily percussive material, and the crate documents
        // <0.3 confidence as "may be unreliable" - reporting a wrong key
        // confidently is worse than reporting none, so those are dropped.
        key: if confidence.key_confidence >= MIN_KEY_CONFIDENCE {
            Some(result.key.name())
        } else {
            None
        },
    }
}

#[cfg(test)]
mod excerpt_tests {
    use super::*;

    #[test]
    fn a_short_track_is_used_whole() {
        let s = vec![0.0; 44_100 * 60];
        assert_eq!(excerpt(&s, 44_100, MAX_SECONDS_FOR_TEMPO).len(), s.len());
    }

    #[test]
    fn a_long_track_is_cut_to_its_middle() {
        let sr = 1_000;
        let s: Vec<f32> = (0..sr * 300).map(|i| i as f32).collect();
        let e = excerpt(&s, sr as u32, MAX_SECONDS_FOR_TEMPO);
        assert_eq!(e.len(), sr * 90);
        // 300 s long, 90 s kept: it starts 105 s in.
        assert_eq!(e[0], (sr * 105) as f32);
    }
}
