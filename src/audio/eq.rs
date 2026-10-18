//! The EQ policy: turns measured band levels into a track-adaptive suggested
//! EQ (which bands, how much, how wide) and the preamp that keeps it from
//! clipping. All the tuning constants live here.

use super::spectrum::{FINE_BAND_CENTERS_HZ, SILENT_DB};
use crate::models::EqBand;

/// Suggested gains are clamped to this - the range a typical 10-band EQ's
/// sliders cover.
pub const MAX_SUGGESTED_GAIN_DB: f32 = 12.0;

/// What the Suggested EQ is measured against: how music usually sounds, not a flat or a "perfect" curve.
///
/// `REFERENCE_DB[i]` is the typical level of 1/3-octave band `i` (see `spectrum::FINE_BAND_CENTERS_HZ`) relative
/// to the same track's own anchor, the mean of its 100 Hz - 5 kHz bands, in the A-weighted power terms
/// `spectrum::measure` reports. `SPREAD_DB[i]` is the normal range around it: how far individual tracks stray,
/// 1.5 robust standard deviations wide, so about two thirds of tracks sit inside it. Both were measured
/// over 927 tracks in 59 albums from the author's own library, grouped by
/// artist or series (14 groups, so a dozen releases of one soundtrack count once): the reference is the median
/// over groups, the range the weighted spread of every track around it. That library leans towards
/// rock, soundtracks, K-pop and Malay pop; it is a reference for "usual", not for "correct".
const REFERENCE_DB: [f32; 29] = [-50.0, -40.5, -29.1, -20.4, -14.7, -7.1, -4.6, -1.8, -0.3, 1.4, 2.0, 3.1, 3.4, 2.8, 3.5, 4.1, 2.6, 2.0, 1.3, 0.1, -1.5, -2.6, -5.1, -8.9, -12.8, -16.2, -20.0, -26.2, -34.5];
const SPREAD_DB: [f32; 29] = [12.6, 12.1, 11.4, 13.6, 12.1, 8.5, 8.4, 6.1, 4.6, 4.4, 5.0, 7.3, 6.7, 5.1, 4.8, 4.1, 3.3, 2.9, 3.6, 4.6, 6.2, 5.5, 6.1, 5.2, 5.5, 7.2, 8.5, 10.9, 12.2];

/// A band is never flagged for straying by less than this, whatever the measured range says.
const MIN_SPREAD_DB: f32 = 3.0;
/// Only the part of a deviation beyond the normal spread is corrected, and only this fraction of that part:
/// a hint toward the usual, not a flattening.
const CORRECTION_FRACTION: f32 = 0.5;
/// The bands whose mean sets a track's own level for comparison (100 Hz - 5 kHz): present in every file, a
/// lossy low-pass or a missing sub-bass cannot move it.
const ANCHOR_BANDS: std::ops::RangeInclusive<usize> = 6..=23;

/// A band's correction has to reach this before
/// it's worth a suggested band at all - most of a well-mixed track's
/// spectrum shouldn't get touched, which is exactly what lets the band
/// count vary by track instead of always being a fixed 10.
const MIN_SIGNIFICANT_GAIN_DB: f32 = 1.0;
/// A point this far below the track's own loudest measured band is treated
/// as having nothing real there to boost - just noise/FFT leakage, which
/// boosting would only amplify. Cuts aren't filtered this way: cutting
/// something already quiet is harmless regardless of its absolute level.
const NOISE_FLOOR_RELATIVE_DB: f32 = 30.0;
/// Defensive cap for a pathological alternating-sign spectrum that would
/// otherwise merge into an unreasonable number of bands - in practice the
/// threshold+merge logic below keeps counts far below this on real music.
const MAX_ADAPTIVE_BANDS: usize = 15;

/// The mean of the anchor bands that were measured; `None` when too few were.
fn anchor_db(measured_db: &[Option<f32>]) -> Option<f32> {
    let v: Vec<f32> = ANCHOR_BANDS.filter_map(|i| measured_db.get(i).copied().flatten()).collect();
    (v.len() >= 6).then(|| v.iter().sum::<f32>() / v.len() as f32)
}

/// How far `deviation` (dB from the reference at band `i`) lies beyond the band's normal spread, signed; 0 inside it.
fn excess_over_spread(deviation: f32, i: usize) -> f32 {
    let spread = SPREAD_DB[i].max(MIN_SPREAD_DB);
    deviation.signum() * (deviation.abs() - spread).max(0.0)
}

/// Turns fine-grained (`FINE_BAND_CENTERS_HZ`, 1/3-octave) measured levels
/// into a track-adaptive suggested EQ: unlike a fixed 10-band layout, both
/// the band *count* and each band's exact frequency/Q are derived from
/// where this specific track's spectrum actually deviates from its own
/// tilted reference target, not a preset grid.
///
/// Per-point suggested gain uses the same shape as a fixed-band approach
/// (damped-and-clamped deviation from a tilted target), but only a point
/// that clears `MIN_SIGNIFICANT_GAIN_DB` becomes part of a band at all;
/// consecutive significant points of the same sign (e.g. several adjacent
/// 1/3-octave points all reading "too loud") are merged into one band
/// centered on the region's own most-extreme point, with a lower Q the
/// wider that region is - a single isolated peak gets a narrow, high-Q
/// band, a broad tonal tilt gets a wide, low-Q one.
///
/// `measured_db[i]` is `None` for a band with no usable measurement at
/// all: one narrower than the FFT's own bin spacing, or entirely above
/// Nyquist for this file's sample rate (see `spectrum::measure`). Those are
/// skipped rather than folded in as silence, because a fabricated `-120dB`
/// reading would drag `mean` (the reference level every other band's
/// suggestion is measured against) down with it and skew the whole curve.
pub(super) fn suggest(measured_db: &[Option<f32>]) -> Vec<EqBand> {
    let n = measured_db.len();
    let Some(anchor) = anchor_db(measured_db) else { return Vec::new() };
    let measured: Vec<f32> = measured_db.iter().flatten().copied().collect();
    let loudest = measured.iter().cloned().fold(f32::MIN, f32::max);

    // Deliberately unclamped - peak-picking and tie-breaking below need each point's *real* deviation, not
    // one that's been capped to MAX_SUGGESTED_GAIN_DB. A genuinely dominant standout and the milder
    // spectral leakage it causes in its immediate neighbor bands can easily ALL exceed the cap and clamp to
    // the exact same ceiling value - if peak detection compared clamped magnitudes, that artificial tie
    // could let a neighbor "win" and mis-report the standout's own frequency. suggested_gain_db (the only
    // clamped value that leaves this function) is derived from this at the very end.
    // A `None` band contributes 0.0 so the peak-picking below can index uniformly - it is never
    // `significant`, so it can't become a band or be absorbed into one.
    let unclamped: Vec<f32> = (0..n)
        .map(|i| match measured_db[i] {
            Some(db) => -CORRECTION_FRACTION * excess_over_spread(db - anchor - REFERENCE_DB[i], i),
            None => 0.0,
        })
        .collect();

    let mut significant: Vec<bool> = (0..n)
        .map(|i| measured_db[i].is_some() && unclamped[i].abs() >= MIN_SIGNIFICANT_GAIN_DB)
        .collect();
    for i in 0..n {
        let Some(db) = measured_db[i] else { continue };
        if significant[i] && unclamped[i] > 0.0 && db < loudest - NOISE_FLOOR_RELATIVE_DB {
            significant[i] = false;
        }
    }

    // Anchored on local maxima of |raw| (among significant points) rather
    // than just merging any run of consecutive same-sign significant
    // points: a genuine narrow standout sitting right next to a broader,
    // milder tilt-driven region (e.g. a wide near-silent stretch that's
    // *just* significant purely from the reference tilt, not a real
    // feature) would otherwise get absorbed into that region and its exact
    // frequency mis-reported as wherever that region's own peak happened
    // to land - defeating the entire point of resolving a specific
    // frequency accurately. Each peak instead grows its own region only
    // while adjacent points are significant, the same sign, and
    // monotonically *no louder* moving away from the peak - a real valley
    // (magnitude turning back up) always stops growth before it can bleed
    // into a neighboring, unrelated peak's own territory.
    let mut is_peak = vec![false; n];
    for i in 0..n {
        if !significant[i] {
            continue;
        }
        let left_ok = i == 0 || unclamped[i - 1].abs() <= unclamped[i].abs();
        let right_ok = i + 1 >= n || unclamped[i + 1].abs() <= unclamped[i].abs();
        if left_ok && right_ok {
            is_peak[i] = true;
        }
    }

    let mut claimed = vec![false; n];
    let mut bands = Vec::new();
    for i in 0..n {
        if !is_peak[i] || claimed[i] {
            continue; // not a peak, or already absorbed by an earlier (necessarily >=-magnitude) peak's own region
        }
        let sign = unclamped[i].signum();

        let mut start = i;
        let mut prev_mag = unclamped[i].abs();
        while start > 0 {
            let cand = start - 1;
            if !significant[cand] || claimed[cand] || unclamped[cand].signum() != sign || unclamped[cand].abs() > prev_mag
            {
                break;
            }
            prev_mag = unclamped[cand].abs();
            start = cand;
        }
        let mut end = i;
        prev_mag = unclamped[i].abs();
        while end + 1 < n {
            let cand = end + 1;
            if !significant[cand] || claimed[cand] || unclamped[cand].signum() != sign || unclamped[cand].abs() > prev_mag
            {
                break;
            }
            prev_mag = unclamped[cand].abs();
            end = cand;
        }

        for c in claimed.iter_mut().take(end + 1).skip(start) {
            *c = true;
        }

        let width = end - start + 1;
        let q = (2.8 / width as f32).clamp(0.6, 2.8);
        bands.push(EqBand {
            hz: FINE_BAND_CENTERS_HZ[i],
            // Peaks are always `significant`, which requires `Some`.
            measured_db: measured_db[i].unwrap_or(SILENT_DB),
            suggested_gain_db: unclamped[i].clamp(-MAX_SUGGESTED_GAIN_DB, MAX_SUGGESTED_GAIN_DB),
            q,
        });
    }
    bands.sort_by(|a, b| a.hz.total_cmp(&b.hz));

    if bands.len() > MAX_ADAPTIVE_BANDS {
        bands.sort_by(|a, b| b.suggested_gain_db.abs().total_cmp(&a.suggested_gain_db.abs()));
        bands.truncate(MAX_ADAPTIVE_BANDS);
        bands.sort_by(|a, b| a.hz.total_cmp(&b.hz));
    }

    bands
}

/// Sample rate the headroom check is evaluated at. Peaking filters barely change shape between 44.1 and
/// 48 kHz below 16 kHz, so one rate is enough.
const RESPONSE_RATE_HZ: f32 = 48_000.0;

/// Magnitude in dB at `f` Hz of one peaking filter (RBJ cookbook, the same form as Web Audio's
/// `BiquadFilterNode` of type `peaking`), as `[center Hz, gain dB, Q]`.
fn peaking_db_at(center_hz: f32, gain_db: f32, q: f32, f: f32) -> f32 {
    let a = 10f32.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * center_hz / RESPONSE_RATE_HZ;
    let alpha = w0.sin() / (2.0 * q.max(0.01));
    let (b0, b1, b2) = (1.0 + alpha * a, -2.0 * w0.cos(), 1.0 - alpha * a);
    let (a0, a1, a2) = (1.0 + alpha / a, -2.0 * w0.cos(), 1.0 - alpha / a);
    let w = 2.0 * std::f32::consts::PI * f / RESPONSE_RATE_HZ;
    let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    let num = ((b0 + b1 * c1 + b2 * c2).powi(2) + (b1 * s1 + b2 * s2).powi(2)).max(1e-20);
    let den = ((a0 + a1 * c1 + a2 * c2).powi(2) + (a1 * s1 + a2 * s2).powi(2)).max(1e-20);
    10.0 * (num / den).log10()
}

/// The highest point, in dB, of the combined response of a cascade of peaking filters `[Hz, gain dB, Q]`
/// over 20 Hz - 20 kHz (checked every 1/24 octave). Cascaded filters add in dB, so neighbouring boosts
/// rise above any single band's gain.
pub fn combined_peak_db(bands: &[(f32, f32, f32)]) -> f32 {
    if bands.is_empty() {
        return 0.0;
    }
    let steps = (24.0 * (20_000.0f32 / 20.0).log2()).ceil() as usize;
    (0..=steps)
        .map(|i| {
            let f = 20.0 * 2f32.powf(i as f32 / 24.0);
            bands.iter().map(|&(hz, g, q)| peaking_db_at(hz, g, q, f)).sum::<f32>()
        })
        .fold(f32::MIN, f32::max)
}

/// The preamp that keeps a suggested EQ from clipping: exactly the loudest point of the bands' combined
/// response, so the EQ never lifts a signal above where it started (0 when nothing is boosted).
pub fn preamp_for_bands(bands: &[(f32, f32, f32)]) -> f32 {
    (-combined_peak_db(bands).max(0.0)).clamp(-MAX_SUGGESTED_GAIN_DB, 0.0)
}

pub(super) fn preamp_for(bands: &[EqBand]) -> f32 {
    let triples: Vec<_> = bands.iter().map(|b| (b.hz, b.suggested_gain_db, b.q)).collect();
    preamp_for_bands(&triples)
}

/// A suggestion at `strength` (0 = none, 1 = all of it): every gain scaled, and the preamp recomputed for
/// the scaled bands. This is what is heard and shown.
pub fn scale_suggestion(bands: &[EqBand], strength: f32) -> (Vec<EqBand>, f32) {
    let k = strength.clamp(0.0, 1.0);
    let scaled: Vec<EqBand> = bands.iter().map(|b| EqBand { suggested_gain_db: b.suggested_gain_db * k, ..b.clone() }).collect();
    let preamp = preamp_for(&scaled);
    (scaled, preamp)
}

/// What the analysis window's chart draws, one entry per third-octave band (`spectrum::FINE_BAND_CENTERS_HZ`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChartData {
    pub centers_hz: Vec<f32>,
    /// How far this track is above (+) or below (-) the reference at each band, in dB; `None` where nothing
    /// was measured (above a lossy cut-off, or past Nyquist).
    pub deviation_db: Vec<Option<f32>>,
    /// Half the normal range around the reference at each band, in dB.
    pub range_db: Vec<f32>,
    /// The suggested EQ's own response at each band centre (without its preamp), in dB.
    pub eq_db: Vec<f32>,
    pub lowpass_hz: Option<f32>,
}

/// The chart for a measured spectrum and the suggested bands (already scaled to the strength in use).
/// `None` when too little of the spectrum was measured.
pub fn chart_data(measured_db: &[Option<f32>], bands: &[EqBand], lowpass_hz: Option<f32>) -> Option<ChartData> {
    let anchor = anchor_db(measured_db)?;
    let n = FINE_BAND_CENTERS_HZ.len();
    let triples: Vec<(f32, f32, f32)> = bands.iter().map(|b| (b.hz, b.suggested_gain_db, b.q)).collect();
    Some(ChartData {
        centers_hz: FINE_BAND_CENTERS_HZ.to_vec(),
        deviation_db: (0..n).map(|i| measured_db.get(i).copied().flatten().map(|db| db - anchor - REFERENCE_DB[i])).collect(),
        range_db: (0..n).map(|i| SPREAD_DB[i].max(MIN_SPREAD_DB)).collect(),
        eq_db: FINE_BAND_CENTERS_HZ
            .iter()
            .map(|&f| triples.iter().map(|&(hz, g, q)| peaking_db_at(hz, g, q, f)).sum::<f32>())
            .collect(),
        lowpass_hz,
    })
}

/// The way a track's balance leans compared with most music, for filtering lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Bass-heavy.
    Bassy,
    /// Light on bass.
    Light,
    /// Mids forward.
    MidForward,
    /// Scooped mids.
    Scooped,
    /// Bright (strong highs).
    Bright,
    /// Dark (weak highs).
    Dark,
}

/// A plain-language reading of a track's spectrum against the reference: which broad region (bass, mids,
/// highs) departs most from how music usually sounds, in units of that band's normal spread, or that it is
/// balanced like most music.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// A few words, for a list row ("Leans bass-heavy", "Balanced like most music").
    pub short: String,
    /// A sentence for the analysis window.
    pub long: String,
    pub balanced: bool,
    /// How it leans, `None` when balanced.
    pub tone: Option<Tone>,
}

/// Mean deviation, in spreads, of the measured bands in `range` (None if fewer than three were measured).
fn region_z(measured_db: &[Option<f32>], anchor: f32, range: std::ops::RangeInclusive<usize>) -> Option<f32> {
    let z: Vec<f32> = range
        .filter_map(|i| {
            let db = measured_db.get(i).copied().flatten()?;
            Some((db - anchor - REFERENCE_DB[i]) / SPREAD_DB[i].max(MIN_SPREAD_DB))
        })
        .collect();
    (z.len() >= 3).then(|| z.iter().sum::<f32>() / z.len() as f32)
}

/// `None` when too little of the spectrum was measured to say anything.
pub fn verdict(measured_db: &[Option<f32>]) -> Option<Verdict> {
    let anchor = anchor_db(measured_db)?;
    // (low / mid / high, its deviation in spreads)
    let regions = [
        (0, region_z(measured_db, anchor, 0..=10)),
        (1, region_z(measured_db, anchor, 11..=19)),
        (2, region_z(measured_db, anchor, 20..=28)),
    ];
    let (region, z) = regions
        .iter()
        .filter_map(|&(r, z)| z.map(|z| (r, z)))
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
    if z.abs() < 1.0 {
        return Some(Verdict {
            short: "Balanced like most music".into(),
            long: "Balanced like most music - there is little or nothing for the suggested EQ to ease.".into(),
            balanced: true,
            tone: None,
        });
    }
    let strong = z.abs() >= 2.0;
    let short = match (region, z > 0.0, strong) {
        (0, true, true) => "Very bass-heavy",
        (0, true, false) => "Leans bass-heavy",
        (0, false, true) => "Very light on bass",
        (0, false, false) => "A little light on bass",
        (1, true, true) => "Very mid-forward",
        (1, true, false) => "Leans mid-forward",
        (1, false, true) => "Very scooped mids",
        (1, false, false) => "Slightly scooped mids",
        (_, true, true) => "Very bright",
        (_, true, false) => "Leans bright",
        (_, false, true) => "Very dark",
        (_, false, false) => "Leans dark",
    };
    let tone = match (region, z > 0.0) {
        (0, true) => Tone::Bassy,
        (0, false) => Tone::Light,
        (1, true) => Tone::MidForward,
        (1, false) => Tone::Scooped,
        (_, true) => Tone::Bright,
        (_, false) => Tone::Dark,
    };
    Some(Verdict {
        short: short.into(),
        long: format!("{short} compared with most music - the suggested EQ eases it."),
        balanced: false,
        tone: Some(tone),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_with_no_usable_bins_are_skipped_rather_than_read_as_silence() {
        // A band can legitimately have no FFT bins at all - here, one above
        // Nyquist for the file's own sample rate. Folding those in as a
        // fabricated -120dB reading would drag `mean` (the reference level
        // every other band's suggestion is measured against) down and skew
        // the entire curve, so they're passed as `None` and skipped.
        let n = FINE_BAND_CENTERS_HZ.len();
        let flat: Vec<Option<f32>> = (0..n)
            .map(|i| if i == 16 { Some(-20.0) } else { Some(-40.0) })
            .collect();
        let baseline = suggest(&flat);

        // Same spectrum, but the top two bands are unmeasurable.
        let mut with_gaps = flat.clone();
        with_gaps[n - 1] = None;
        with_gaps[n - 2] = None;
        let gapped = suggest(&with_gaps);

        assert!(
            gapped.iter().all(|b| b.hz < FINE_BAND_CENTERS_HZ[n - 2]),
            "an unmeasurable band should never be suggested: {gapped:?}"
        );
        let loud_gain = |bands: &[EqBand]| {
            bands
                .iter()
                .find(|b| (b.hz - FINE_BAND_CENTERS_HZ[16]).abs() < 1.0)
                .map(|b| b.suggested_gain_db)
                .expect("the one loud band should still be suggested")
        };
        assert!(
            (loud_gain(&baseline) - loud_gain(&gapped)).abs() < 0.5,
            "dropping two unmeasurable bands shifted the loud band's suggested gain from {} to {} - they're still skewing the reference mean",
            loud_gain(&baseline),
            loud_gain(&gapped)
        );
    }

    /// A spectrum exactly on the reference, at some arbitrary level.
    fn on_reference(offset: f32) -> Vec<Option<f32>> {
        // The reference is a median per band, so its own anchor mean is not exactly zero; remove it.
        let own_anchor = ANCHOR_BANDS.map(|i| REFERENCE_DB[i]).sum::<f32>() / ANCHOR_BANDS.count() as f32;
        REFERENCE_DB.iter().map(|r| Some(r - own_anchor + offset)).collect()
    }

    #[test]
    fn music_that_sounds_like_the_reference_gets_no_suggestion() {
        for offset in [-60.0, -35.0, -10.0] {
            let m = on_reference(offset);
            assert!(suggest(&m).is_empty(), "offset {offset}");
            assert!(verdict(&m).unwrap().balanced);
        }
    }

    #[test]
    fn the_usual_spread_is_not_corrected_but_beyond_it_is() {
        let i = 7; // 125 Hz
        let mut inside = on_reference(-30.0);
        inside[i] = inside[i].map(|v| v + SPREAD_DB[i].max(MIN_SPREAD_DB) * 0.9);
        assert!(suggest(&inside).is_empty(), "a bump inside the normal spread was corrected");
        let mut beyond = on_reference(-30.0);
        beyond[i] = beyond[i].map(|v| v + SPREAD_DB[i].max(MIN_SPREAD_DB) + 10.0);
        let bands = suggest(&beyond);
        assert_eq!(bands.len(), 1, "{bands:?}");
        assert_eq!(bands[0].hz, 125.0);
        assert!(bands[0].suggested_gain_db < -3.0 && bands[0].suggested_gain_db > -6.0, "{:?}", bands[0]);
    }

    #[test]
    fn verdicts_carry_the_tone_they_describe() {
        assert_eq!(verdict(&on_reference(-30.0)).unwrap().tone, None);
        let mut dark = on_reference(-30.0);
        for slot in dark.iter_mut().skip(20) {
            *slot = slot.map(|v| v - 14.0);
        }
        assert_eq!(verdict(&dark).unwrap().tone, Some(Tone::Dark));
        let mut bright = on_reference(-30.0);
        for slot in bright.iter_mut().skip(20) {
            *slot = slot.map(|v| v + 14.0);
        }
        assert_eq!(verdict(&bright).unwrap().tone, Some(Tone::Bright));
        let mut bassy = on_reference(-30.0);
        for slot in bassy.iter_mut().take(11) {
            *slot = slot.map(|v| v + 12.0);
        }
        assert_eq!(verdict(&bassy).unwrap().tone, Some(Tone::Bassy));
        let mut mid = on_reference(-30.0);
        for slot in mid.iter_mut().skip(11).take(9) {
            *slot = slot.map(|v| v + 12.0);
        }
        assert_eq!(verdict(&mid).unwrap().tone, Some(Tone::MidForward));
    }

    #[test]
    fn a_dull_track_is_boosted_up_there_and_called_dark() {
        let mut m = on_reference(-30.0);
        for slot in m.iter_mut().skip(20) {
            *slot = slot.map(|v| v - 14.0);
        }
        let bands = suggest(&m);
        assert!(bands.iter().any(|b| b.hz >= 2500.0 && b.suggested_gain_db > 2.0), "{bands:?}");
        assert!(verdict(&m).unwrap().short.contains("dark"));
    }

    #[test]
    fn a_bass_heavy_track_is_called_so_and_cut_down_low() {
        let mut m = on_reference(-30.0);
        for slot in m.iter_mut().take(11) {
            *slot = slot.map(|v| v + 12.0);
        }
        let v = verdict(&m).unwrap();
        assert!(v.short.contains("bass-heavy"), "{}", v.short);
        assert!(suggest(&m).iter().any(|b| b.hz <= 250.0 && b.suggested_gain_db < -2.0));
    }

    #[test]
    fn bands_above_a_lossy_cutoff_are_never_boosted() {
        // A 16 kHz-low-passed file: the top bands are unmeasured, not missing treble.
        let mut m = on_reference(-30.0);
        for slot in m.iter_mut().skip(26) {
            *slot = None;
        }
        assert!(suggest(&m).is_empty());
        assert!(verdict(&m).unwrap().balanced);
    }

    #[test]
    fn too_little_measured_says_nothing() {
        let mut m = vec![None; 29];
        m[10] = Some(-30.0);
        assert!(suggest(&m).is_empty());
        assert!(verdict(&m).is_none());
    }

    #[test]
    fn the_chart_shows_deviation_range_and_the_eq_response() {
        let mut m = on_reference(-30.0);
        m[7] = m[7].map(|v| v + 15.0); // 125 Hz
        let bands = suggest(&m);
        let c = chart_data(&m, &bands, None).unwrap();
        assert_eq!(c.centers_hz.len(), 29);
        // The bumped band is part of the track's own anchor (mean of 18 bands), which rises by 15/18 dB.
        assert!((c.deviation_db[7].unwrap() - 15.0 * 17.0 / 18.0).abs() < 0.2);
        assert!((c.deviation_db[0].unwrap() + 15.0 / 18.0).abs() < 0.2);
        assert!(c.range_db.iter().all(|&r| r >= MIN_SPREAD_DB));
        // The EQ cuts where the track is loud and leaves the far ends alone.
        assert!(c.eq_db[7] < -1.0);
        assert!(c.eq_db[28].abs() < 0.5);
    }

    #[test]
    fn unmeasured_bands_are_gaps_in_the_chart() {
        let mut m = on_reference(-30.0);
        m[28] = None;
        let c = chart_data(&m, &[], Some(14_000.0)).unwrap();
        assert!(c.deviation_db[28].is_none());
        assert_eq!(c.lowpass_hz, Some(14_000.0));
        assert!(chart_data(&vec![None; 29], &[], None).is_none());
    }

    fn band(hz: f32, gain: f32, q: f32) -> EqBand {
        EqBand { hz, measured_db: 0.0, suggested_gain_db: gain, q }
    }

    #[test]
    fn neighbouring_boosts_stack_and_the_preamp_covers_the_real_peak() {
        // Three adjacent +6 dB bands at Q 2.8 add up well past +6 dB between them.
        let bands = [band(500.0, 6.0, 2.8), band(630.0, 6.0, 2.8), band(800.0, 6.0, 2.8)];
        let triples: Vec<_> = bands.iter().map(|b| (b.hz, b.suggested_gain_db, b.q)).collect();
        let peak = combined_peak_db(&triples);
        assert!(peak > 9.0 && peak < 12.0, "stacked peak was {peak}");
        let pre = preamp_for(&bands);
        assert!((pre + peak).abs() < 0.01, "preamp {pre} does not cancel peak {peak}");
        // The old rule (half the biggest single boost) left this much over full scale.
        assert!(peak - 3.0 > 6.0);
    }

    #[test]
    fn cuts_alone_need_no_preamp() {
        assert_eq!(preamp_for(&[band(1000.0, -8.0, 1.0), band(4000.0, -5.0, 2.0)]), 0.0);
        assert_eq!(preamp_for(&[]), 0.0);
    }

    #[test]
    fn a_single_boost_peaks_at_its_own_gain() {
        let peak = combined_peak_db(&[(1000.0, 6.0, 1.0)]);
        assert!((peak - 6.0).abs() < 0.3, "{peak}");
    }

    #[test]
    fn strength_scales_gains_and_recomputes_the_preamp() {
        let bands = [band(100.0, 8.0, 1.0), band(3000.0, -6.0, 1.0)];
        let (full, pre_full) = scale_suggestion(&bands, 1.0);
        let (half, pre_half) = scale_suggestion(&bands, 0.5);
        let (none, pre_none) = scale_suggestion(&bands, 0.0);
        assert_eq!(full[0].suggested_gain_db, 8.0);
        assert_eq!(half[0].suggested_gain_db, 4.0);
        assert_eq!(half[1].suggested_gain_db, -3.0);
        assert!(none.iter().all(|b| b.suggested_gain_db == 0.0));
        assert!(pre_full < pre_half && pre_half <= 0.0 && pre_none == 0.0);
    }
}
