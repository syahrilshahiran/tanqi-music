//! The spectral profile of a track: per-band median level over the loud
//! frames, A-weighted, plus overall loudness. Pure numbers in, numbers out.

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

/// Band edges in Hz, following common EQ conventions: bass covers sub-bass
/// through low-bass, mid covers the vocal/instrument body, treble is the
/// presence/brightness range above that.
///
/// Treble is deliberately capped at `TREBLE_MAX_HZ` rather than extending to
/// Nyquist (~22kHz at 44.1kHz sample rate). The cap was set from
/// measurements of lossy AAC audio (YouTube downloads, this app's first
/// source of audio), which rolls off almost everything above ~8-10kHz
/// regardless of genre: a "treble" bucket that ran all the way to Nyquist
/// was ~78% near-silent bins by count (8kHz commonly read around +6 to
/// +9dB while 16kHz read -2 to -9dB on the same tracks) - dragging the
/// average down hard and making virtually every song, guitar/piano ones
/// included, misreport as "very bass-heavy" regardless of its actual tonal
/// balance. The cap is the same for every file. It only affects the broad
/// 3-band summary; the fine `FINE_BAND_CENTERS_HZ` breakdown already
/// compares each band against its own expected level individually and
/// isn't subject to this averaging-in-dead-air problem the same way.
const BASS_MAX_HZ: f32 = 250.0;
const MID_MAX_HZ: f32 = 4_000.0;
const TREBLE_MAX_HZ: f32 = 8_000.0;

/// FFT window size. At 44.1kHz this is ~2.7Hz per bin.
///
/// Deliberately much larger than the ~10.8Hz/bin a 4096-point window gives:
/// the lowest `FINE_BAND_CENTERS_HZ` bands are *narrower than one bin* at
/// that resolution (the 1/3-octave band around 31.5Hz spans just 28.1-35.4Hz,
/// ~7Hz wide), so their bin range would round to zero bins and the band
/// would report as pure silence regardless of what the track actually does
/// down there - dragging the reference mean in `eq::suggest` down with it
/// and skewing every other band's suggested gain. Which band degenerates
/// depends on the sample rate (31.5Hz at 44.1kHz, 25Hz at 48kHz), so this
/// isn't something a one-off tweak to the band table could fix. At 2.7Hz
/// per bin every band from 25Hz up spans at least two real bins.
pub(super) const FFT_SIZE: usize = 16384;

/// Step between successive FFT frames - half a window, i.e. 50% overlap.
///
/// Frames used to be taken back-to-back with no overlap, which badly
/// underweights whatever lands near a frame boundary: the Hann window below
/// tapers to zero at both edges, so with no overlap those samples are
/// attenuated to nothing and the measured spectrum depends on where frame
/// boundaries happen to fall relative to the music. At 50% overlap Hann
/// sums to a constant across frames, so every sample contributes equally no
/// matter how it lines up - the standard (Welch) fix for exactly this bias.
const FFT_HOP_SIZE: usize = FFT_SIZE / 2;

/// Frames this far below the track's loudest frame are treated as silence
/// and left out of the spectral profile entirely. A quiet intro/outro, or
/// the lead-in/lead-out either side of a track, otherwise contributes its
/// noise floor to the profile as though it were part of the music.
/// Deliberately generous - only genuine silence and dither noise should be
/// dropped here, never a quiet-but-real passage.
const SILENCE_GATE_BELOW_PEAK_DB: f64 = 60.0;

/// ISO 266 preferred 1/3-octave center frequencies, 25Hz through 16kHz - the
/// standard "31-band graphic EQ" grid. This is the measurement resolution
/// for the suggested EQ (see `eq::suggest`): three times finer than a
/// classic 10-band layout, which is what actually lets a suggestion land on
/// a specific, narrow standout frequency (e.g. ~3150Hz) instead of always
/// spreading it across a whole 2000-4000Hz octave regardless of where in
/// that octave the track's own issue actually sits. Each band spans 1/3 of
/// an octave centered on its frequency (edges at center/2^(1/6) to
/// center*2^(1/6)), which is why every 3rd entry doubles the one 3 back -
/// exactly the classic 10-band centers (31.5, 63, 125, ...) as a subset.
pub(super) const FINE_BAND_CENTERS_HZ: [f32; 29] = [
    25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0,
    630.0, 800.0, 1_000.0, 1_250.0, 1_600.0, 2_000.0, 2_500.0, 3_150.0, 4_000.0, 5_000.0, 6_300.0,
    8_000.0, 10_000.0, 12_500.0, 16_000.0,
];

/// Levels measured over the frames that aren't silence, all relative to
/// each other (not absolute loudness).
pub(super) struct Spectrum {
    pub bass_db: f32,
    pub mid_db: f32,
    pub treble_db: f32,
    pub overall_db: f32,
    /// One entry per `FINE_BAND_CENTERS_HZ`; `None` for a band with no
    /// usable bins (narrower than the FFT's bin spacing, or above Nyquist).
    pub fine_db: Vec<Option<f32>>,
    /// Where a lossy encoder cut the top of the spectrum off (Hz), if it did; bands above it are `None` in
    /// `fine_db`.
    pub lowpass_hz: Option<f32>,
}

/// Bin index edges bounding each of `centers`' bands, `edge_factor` wide on
/// each side of center (e.g. `2f32.sqrt()` for a full-octave band,
/// `2f32.powf(1.0 / 6.0)` for a 1/3-octave band): `edges[i]` is band `i`'s
/// low edge and `edges[i+1]` its high edge (= band `i+1`'s low edge, by
/// construction, as long as `centers` are evenly log-spaced by exactly
/// `edge_factor^2`).
fn band_edges(centers: &[f32], edge_factor: f32, bin_hz: f32, nyquist_bin: usize) -> Vec<usize> {
    let mut edges = Vec::with_capacity(centers.len() + 1);
    edges.push(((centers[0] / edge_factor) / bin_hz).round() as usize);
    for &center in centers {
        edges.push((((center * edge_factor) / bin_hz).round() as usize).min(nyquist_bin));
    }
    edges
}

/// Which band `bin` falls in, or `None` if it's below the lowest band's
/// edge or at/above the highest band's (e.g. above Nyquist).
fn band_index(bin: usize, edges: &[usize]) -> Option<usize> {
    if bin < edges[0] || bin >= edges[edges.len() - 1] {
        return None;
    }
    edges.windows(2).position(|w| bin >= w[0] && bin < w[1])
}

/// Standard IEC 61672 A-weighting curve, as a linear AMPLITUDE ratio
/// (square it for a power/energy multiplier). Raw FFT energy vastly
/// overstates bass relative to how loud it actually sounds: human hearing
/// is dramatically less sensitive to low frequencies than to the 1-4kHz
/// range at the same physical energy level - exactly why real sound level
/// meters use A-weighting instead of raw SPL. Without this, virtually
/// every track's band comparison read as "very bass-heavy" regardless of
/// genre (confirmed: even after excluding AAC's near-dead >8kHz encoding
/// rolloff from the broad treble bucket, the raw bass/treble gap on real
/// tracks was still 20-27dB, consistent with A-weighting's own ~-20dB
/// attenuation around 100Hz - the raw numbers were never wrong, they just
/// weren't measuring what a person actually perceives).
fn a_weighting_amplitude_ratio(freq_hz: f32) -> f32 {
    if freq_hz <= 0.0 {
        return 0.0;
    }
    let f2 = freq_hz * freq_hz;
    let numerator = 12_194.0f32.powi(2) * f2 * f2;
    let denominator = (f2 + 20.6f32.powi(2))
        * ((f2 + 107.7f32.powi(2)) * (f2 + 737.9f32.powi(2))).sqrt()
        * (f2 + 12_194.0f32.powi(2));
    numerator / denominator
}

/// Median of `values` (which must be non-empty), reordering it in place.
/// Used instead of a mean when summarizing a band across frames - see
/// `measure`.
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

/// Measures a spectrum while a file is decoded, packet by packet: each channel is windowed and transformed
/// on its own and the channels' band POWERS are averaged, so content that differs between left and right
/// (even out of phase) is counted, where averaging the samples first would cancel it. Only the pending
/// samples (under two windows per channel) and one small row of band levels per frame are kept.
pub(super) struct SpectrumBuilder {
    channels: usize,
    pending: Vec<Vec<f32>>,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    buffer: Vec<Complex<f32>>,
    window: Vec<f32>,
    power_weights: Vec<f64>,
    nyquist_bin: usize,
    treble_max_bin: usize,
    bass_max_bin: usize,
    mid_max_bin: usize,
    fine_edges: Vec<usize>,
    band_bins: [usize; 3],
    fine_bins: Vec<usize>,
    // Kept per frame rather than accumulated into a running total: the figure reported per band is the
    // MEDIAN across frames, not the mean. A mean is dominated by a track's loudest moments - one cymbal
    // crash or bass drop pulls the whole profile toward its spectrum - whereas the median describes what
    // the track's balance actually is most of the time, which is what an EQ suggestion should be built on.
    broad_frames: [Vec<f64>; 3],
    fine_frames: Vec<Vec<f64>>,
    frame_power: Vec<f64>,
    frame_mean_square: Vec<f64>,
}

impl SpectrumBuilder {
    pub(super) fn new(sample_rate: u32, channels: usize) -> Self {
        let channels = channels.max(1);
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);

        let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
        let bass_max_bin = (BASS_MAX_HZ / bin_hz).round() as usize;
        let mid_max_bin = (MID_MAX_HZ / bin_hz).round() as usize;
        let nyquist_bin = FFT_SIZE / 2;
        let treble_max_bin = ((TREBLE_MAX_HZ / bin_hz).round() as usize).min(nyquist_bin);
        let fine_edges = band_edges(&FINE_BAND_CENTERS_HZ, 2f32.powf(1.0 / 6.0), bin_hz, nyquist_bin);
        let fine_count = FINE_BAND_CENTERS_HZ.len();

        // Precomputed once per bin (not per frame) - squared here so the hot loop just multiplies
        // straight into power/energy terms.
        let power_weights: Vec<f64> = (0..nyquist_bin)
            .map(|bin| {
                let ratio = a_weighting_amplitude_ratio(bin as f32 * bin_hz);
                (ratio * ratio) as f64
            })
            .collect();

        // Hann window, precomputed once rather than recomputed (with a `cos` per sample) on every frame.
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE as f32 - 1.0)).cos())
            .collect();

        // Which bins belong to which band never changes frame to frame, so count them once up front.
        // Bands span wildly different bandwidths (bass is a sliver of the spectrum, treble is everything
        // above ~4kHz), so comparing summed energy would make the wide bands look dominant purely from
        // covering more bins - these counts are what turn each band's total into a comparable
        // average-energy-per-bin.
        let mut band_bins = [0usize; 3];
        let mut fine_bins = vec![0usize; fine_count];
        for bin in 1..nyquist_bin {
            if bin < treble_max_bin {
                let band = if bin < bass_max_bin {
                    0
                } else if bin < mid_max_bin {
                    1
                } else {
                    2
                };
                band_bins[band] += 1;
            }
            if let Some(nb) = band_index(bin, &fine_edges) {
                fine_bins[nb] += 1;
            }
        }

        SpectrumBuilder {
            channels,
            pending: vec![Vec::new(); channels],
            fft,
            buffer: vec![Complex { re: 0.0f32, im: 0.0f32 }; FFT_SIZE],
            window,
            power_weights,
            nyquist_bin,
            treble_max_bin,
            bass_max_bin,
            mid_max_bin,
            fine_edges,
            band_bins,
            fine_bins,
            broad_frames: [Vec::new(), Vec::new(), Vec::new()],
            fine_frames: vec![Vec::new(); fine_count],
            frame_power: Vec::new(),
            frame_mean_square: Vec::new(),
        }
    }

    /// Adds interleaved samples (`channels` per frame). Whole windows are processed as soon as they fill.
    pub(super) fn push_interleaved(&mut self, samples: &[f32]) {
        for frame in samples.chunks_exact(self.channels) {
            for (c, &s) in frame.iter().enumerate() {
                self.pending[c].push(s);
            }
        }
        while self.pending[0].len() >= FFT_SIZE {
            self.process_frame();
            for p in &mut self.pending {
                p.drain(..FFT_HOP_SIZE);
            }
        }
    }

    fn process_frame(&mut self) {
        let fine_count = FINE_BAND_CENTERS_HZ.len();
        let mut broad = [0.0f64; 3];
        let mut fine = vec![0.0f64; fine_count];
        let mut total = 0.0f64;
        let mut mean_square = 0.0f64;

        for c in 0..self.channels {
            let frame = &self.pending[c][..FFT_SIZE];
            // Mean square of the raw (unwindowed) frame - the window would attenuate the edges and
            // understate real loudness.
            mean_square += frame.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / FFT_SIZE as f64;
            for (i, (&s, slot)) in frame.iter().zip(self.buffer.iter_mut()).enumerate() {
                *slot = Complex { re: s * self.window[i], im: 0.0 };
            }
            self.fft.process(&mut self.buffer);

            for bin in 1..self.nyquist_bin {
                // bin 0 is DC, skipped
                let mag_sq = self.buffer[bin].norm_sqr() as f64 * self.power_weights[bin];
                total += mag_sq;

                // Bins above TREBLE_MAX_HZ are excluded from the broad 3-band summary entirely (not
                // folded into "treble") - see TREBLE_MAX_HZ's doc comment.
                if bin < self.treble_max_bin {
                    let band = if bin < self.bass_max_bin {
                        0
                    } else if bin < self.mid_max_bin {
                        1
                    } else {
                        2
                    };
                    broad[band] += mag_sq;
                }
                if let Some(nb) = band_index(bin, &self.fine_edges) {
                    fine[nb] += mag_sq;
                }
            }
        }

        // Average of the channels' powers.
        let k = self.channels as f64;
        for b in 0..3 {
            self.broad_frames[b].push(broad[b] / k / self.band_bins[b].max(1) as f64);
        }
        for (i, &bins) in self.fine_bins.iter().enumerate() {
            // A band with no bins at all (narrower than the FFT's own resolution, or entirely above
            // Nyquist for this sample rate) gets no entry rather than a fabricated zero - see
            // `eq::suggest`, which treats "no data" differently from "measured as silent".
            if bins > 0 {
                self.fine_frames[i].push(fine[i] / k / bins as f64);
            }
        }
        self.frame_power.push(total / k);
        self.frame_mean_square.push(mean_square / k);
    }

    pub(super) fn finish(self) -> Spectrum {
        // Silence gate, applied by frame index so every band drops the exact same frames and their
        // medians stay directly comparable.
        let keep: Vec<bool> = if self.frame_power.is_empty() {
            Vec::new()
        } else {
            let peak = self.frame_power.iter().cloned().fold(f64::MIN, f64::max);
            let gate = peak / 10f64.powf(SILENCE_GATE_BELOW_PEAK_DB / 10.0);
            let kept: Vec<bool> = self.frame_power.iter().map(|&p| p >= gate).collect();
            // A track that somehow gates away entirely (all frames identical, digital silence) falls
            // back to using everything - reporting nothing at all would be worse than reporting a
            // quiet spectrum.
            if kept.iter().any(|&k| k) { kept } else { vec![true; self.frame_power.len()] }
        };

        let gated_median_db = |per_frame: &[f64]| -> Option<f32> {
            let mut kept: Vec<f64> =
                per_frame.iter().zip(&keep).filter(|(_, &k)| k).map(|(&v, _)| v).collect();
            if kept.is_empty() {
                return None;
            }
            Some(to_db(median(&mut kept)))
        };

        let mut fine_median_db: Vec<Option<f32>> = self.fine_frames.iter().map(|f| gated_median_db(f)).collect();
        let lowpass_hz = drop_above_lowpass(&mut fine_median_db);

        // Loudness over the same gated frames, so a track with a long silent intro/outro doesn't read as
        // quieter than it actually sounds. A mean of power: this is an energy measure.
        let kept_mean_square: Vec<f64> = self
            .frame_mean_square
            .iter()
            .zip(&keep)
            .filter(|(_, &k)| k)
            .map(|(&v, _)| v)
            .collect();
        let mean_square = if kept_mean_square.is_empty() {
            0.0
        } else {
            kept_mean_square.iter().sum::<f64>() / kept_mean_square.len() as f64
        };

        Spectrum {
            bass_db: gated_median_db(&self.broad_frames[0]).unwrap_or(SILENT_DB),
            mid_db: gated_median_db(&self.broad_frames[1]).unwrap_or(SILENT_DB),
            treble_db: gated_median_db(&self.broad_frames[2]).unwrap_or(SILENT_DB),
            // 10*log10 of mean-square is the same figure as 20*log10 of RMS.
            overall_db: 10.0 * mean_square.max(1e-18).log10() as f32,
            fine_db: fine_median_db,
            lowpass_hz,
        }
    }
}

/// A lossy file (MP3, AAC) is low-passed at its encoder's cut-off: everything above it is near-silent,
/// not music. Such bands would drag the reference mean down and read as "missing treble" to boost. This
/// finds the cut-off as the first band at or above 6.3 kHz that falls at least `LOWPASS_DROP_DB` below the
/// band before it with the rest staying down there, sets every band from it upward to `None`, and returns
/// the cut-off frequency (the lower edge of the first dropped band).
const LOWPASS_DROP_DB: f32 = 12.0;

fn drop_above_lowpass(fine_db: &mut [Option<f32>]) -> Option<f32> {
    let first = FINE_BAND_CENTERS_HZ.iter().position(|&hz| hz >= 6_300.0)?;
    for i in first.max(1)..fine_db.len() {
        let (Some(below), Some(here)) = (fine_db[i - 1], fine_db[i]) else { continue };
        if below - here < LOWPASS_DROP_DB {
            continue;
        }
        let stays_down = fine_db[i..].iter().flatten().all(|&db| db <= below - LOWPASS_DROP_DB);
        if stays_down {
            for slot in fine_db[i..].iter_mut() {
                *slot = None;
            }
            return Some(FINE_BAND_CENTERS_HZ[i] / 2f32.powf(1.0 / 6.0));
        }
    }
    None
}

fn to_db(energy: f64) -> f32 {
    10.0 * energy.max(1e-12).log10() as f32
}

/// What `to_db` bottoms out at - the level reported for a band measured as
/// true digital silence.
pub(super) const SILENT_DB: f32 = -120.0;
