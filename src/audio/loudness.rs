//! Loudness the way broadcasters measure it (ITU-R BS.1770 / EBU R128, through the `ebur128` crate):
//! integrated loudness in LUFS and the true peak, fed packet by packet while the file decodes so no
//! second copy of the audio is held. Playback uses them to bring tracks to one level without letting a
//! boost push a loud master past full scale.

use ebur128::{EbuR128, Mode};

/// What playback aims at, in LUFS (the level most streaming services normalise to).
pub const TARGET_LUFS: f32 = -14.0;
/// A boosted track's true peak may not go above this (dBTP), leaving a little room for the codec and DAC.
pub const PEAK_CEILING_DBTP: f32 = -1.0;
/// Largest boost and cut normalisation will apply.
pub const MAX_BOOST_DB: f32 = 8.0;
pub const MAX_CUT_DB: f32 = 12.0;

/// A track's measured integrated loudness and true peak.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    pub lufs: f32,
    pub true_peak_db: f32,
}

/// Accumulates loudness over interleaved `f32` frames of a file with a known channel count and rate.
pub(super) struct Meter(EbuR128);

impl Meter {
    pub(super) fn new(channels: u32, sample_rate: u32) -> Option<Self> {
        EbuR128::new(channels, sample_rate, Mode::I | Mode::TRUE_PEAK).ok().map(Meter)
    }

    pub(super) fn add_interleaved(&mut self, samples: &[f32]) {
        let _ = self.0.add_frames_f32(samples);
    }

    /// `None` for a file with nothing above the absolute gate (silence).
    pub(super) fn finish(&self) -> Option<Loudness> {
        let lufs = self.0.loudness_global().ok().filter(|l| l.is_finite())?;
        let peak = (0..self.0.channels()).filter_map(|c| self.0.true_peak(c).ok()).fold(0.0f64, f64::max);
        let true_peak_db = if peak > 0.0 { 20.0 * peak.log10() } else { -120.0 };
        Some(Loudness { lufs: lufs as f32, true_peak_db: true_peak_db as f32 })
    }
}

/// The gain (dB) that brings a track to `TARGET_LUFS`, within `MAX_CUT_DB`/`MAX_BOOST_DB`, and never so far up
/// that its true peak passes `PEAK_CEILING_DBTP` (as ReplayGain 2.0 does). Always applied after the EQ's
/// own preamp, which already keeps the EQ from raising the peak.
pub fn normalisation_gain_db(lufs: f32, true_peak_db: Option<f32>) -> f32 {
    let wanted = (TARGET_LUFS - lufs).clamp(-MAX_CUT_DB, MAX_BOOST_DB);
    match true_peak_db {
        Some(peak) => wanted.min(PEAK_CEILING_DBTP - peak),
        None => wanted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_track_is_brought_up_to_the_target() {
        assert_eq!(normalisation_gain_db(-20.0, Some(-10.0)), 6.0);
    }

    #[test]
    fn a_loud_track_is_brought_down() {
        assert_eq!(normalisation_gain_db(-8.0, Some(-0.5)), -6.0);
        // Never more than the cut limit.
        assert_eq!(normalisation_gain_db(5.0, None), -MAX_CUT_DB);
    }

    #[test]
    fn a_boost_stops_at_the_peak_ceiling() {
        // Wants +6 dB, but the peak is already at -3 dBTP: only 2 dB of room below -1 dBTP.
        assert_eq!(normalisation_gain_db(-20.0, Some(-3.0)), 2.0);
        // A peak already above the ceiling is brought down even if the loudness is under target.
        assert_eq!(normalisation_gain_db(-16.0, Some(0.0)), -1.0);
    }

    #[test]
    fn a_boost_is_capped_without_a_peak_too() {
        assert_eq!(normalisation_gain_db(-40.0, None), MAX_BOOST_DB);
    }
}
