# Improving the per-track Audio analysis EQ: research notes

Status: research, no code changed. Terms follow [GLOSSARY.md](../GLOSSARY.md), which calls this feature **Audio analysis** and says to avoid "EQ analysis"; this file says "suggested EQ" for the band list.

Evidence labels: **[opened]** means I read the page or file text. **[abstract]** means only the abstract or a search snippet was readable (AES and ITU full text is paywalled or blocked from here). **[secondary]** means not a primary source; treat as unverified. Numbers marked "my arithmetic" are mine, computed from the formulas named.

## Implementation status

Agreed plan (see `docs/adr/0002-suggested-eq-is-derived-from-the-stored-spectrum.md`), in four steps:

1. **Done.** Preamp computed from the real combined response of the suggested bands (`audio::eq::preamp_for_bands`, `combined_peak_db`), the **Suggested EQ strength** setting (default 70%, in Settings and in the EQ panel; scales what is heard, shown and exported), glossary terms, and the analysis window's heading renamed to "Audio analysis". Note: the preamp is now larger than before for boosted songs, so they play quieter until step 2 brings loudness normalisation in line.
2. **Done.** Integrated loudness (LUFS) and true peak measured with `ebur128` while the file decodes (`audio::loudness`), stored on each analysis (`loudness_lufs`, `true_peak_db`; older analyses keep working on the old RMS estimate until re-analysed), and playback levelled toward -14 LUFS with boosts held back so the true peak stays under -1 dBTP. Checked against `ffmpeg -af ebur128=peak=true` on four real tracks: LUFS identical to 0.1, true peak within 0.2 dB. Analysis time is unchanged (about 1.6-2.9 s on desktop). The analysis window shows a LUFS chip.
3. **Done.** Each analysis now stores its measured spectrum (29 third-octave levels), the lossy low-pass cut-off if one was found, and an `analysis_version` (2). The Suggested EQ and the verdict are worked out from the stored spectrum when an analysis loads (`audio::with_current_suggestion`), against a measured **reference** and normal range (`audio::eq`, `REFERENCE_DB` / `SPREAD_DB`) from `examples/reference_probe.rs` over 927 of the author's tracks (59 albums, 14 artist/series groups; the range is 1.5 robust standard deviations of track-level variation, so about two thirds of tracks fall inside it; the library's genre bias is noted in the code). Only the part of a deviation beyond the range is corrected, by half. The spectrum is measured per channel while decoding (powers averaged, so out-of-phase content is no longer cancelled) and bands above a detected lossy cut-off are left out, never boosted. The bulk "Analyze audio" redoes analyses with an older version. Memory check: the largest file in the library (122 MB 24-bit FLAC) analyses in 3.9 s, peak about 550 MB. Tests added: reference-shaped spectra give no suggestion, bumps inside/beyond the range, dull/bass-heavy verdicts, cut-off bands never boosted, out-of-phase stereo, cut-off detection.
4. **Done.** The analysis window draws this track's deviation from the reference as a line, the normal range shaded, the suggested EQ's response dashed (at the strength in use), and marks a lossy cut-off (`ui/components/spectrum_chart.rs`, data from `audio::chart_data`). Checked on desktop and phone in both skins.

## Summary

What the code does today, in one paragraph: `audio::analyze_band_levels` decodes to mono, runs a 16384-point Hann FFT at 50% overlap over the whole track, A-weights each bin's power, takes the per-frame mean power per 1/3-octave band (25 Hz to 16 kHz), and keeps the median across frames that pass a -60 dB-below-loudest-frame gate (`spectrum::measure`). `eq::suggest` compares those 29 levels with the mean of themselves plus a 1.5 dB/octave downward tilt pivoted at 1 kHz, sets gain to -0.5 x deviation (clamped to +/-12 dB), keeps points whose gain is at least 1.5 dB, merges runs around local maxima, and sets Q = 2.8 / (width in bands), clamped to 0.6..2.8. Preamp is -(largest boost)/2. Playback loudness is a separate path: `overall_db` (gated unweighted RMS) feeds `norm_db` in `ui/player.rs`, which aims at -14 dBFS with a +/-8 dB clamp.

Top recommendations, in order (details and tests in section 6):

1. **Fix the preamp (high value, small, low risk).** `preamp_for` assumes the worst-case boost equals half the largest band gain. Cascaded peaking filters add, so adjacent boosts exceed any single band's gain. In my arithmetic, three adjacent +6 dB bands at Q 2.8 peak at about +10.4 dB while the preamp is only -3 dB. Compute the preamp from the actual summed response of the suggested bands.
2. **Replace RMS normalisation with BS.1770 integrated loudness plus a stored true peak, and cap any boost by the peak (medium effort, high value).** Use the MIT-licensed `ebur128` crate. The current code boosts quiet-RMS tracks by up to 8 dB with no knowledge of their peaks.
3. **Make the reference defensible, or stop claiming one (medium effort, the real accuracy lever).** The current reference is "tilt applied on top of A-weighted per-bin power", which has no stated physical meaning, and the sources do not support Harman, ISO 226 or a universal slope as a target for music. Build the reference empirically from the user's own library (or a bundled corpus), compare in unweighted 1/3-octave terms, and add a user-facing strength control that defaults below 100%.

Honest bottom line on the premise: I found no primary evidence that listener-side per-track corrective EQ improves preference for music, and some evidence that commercial recordings already share a consistent spectral shape (section 5). The feature is best framed as "flag unusual spectral deviations and let the user choose how much to apply", not "make this song balanced".

## 0. How the current analysis works (read from the code)

| Step | Where | What it does |
|---|---|---|
| Decode | `src/audio/decode.rs` `decode_to_mono`, `append_mono_samples` | symphonia decodes the whole file into memory; channels averaged by amplitude into mono `Vec<f32>` |
| Spectrum | `src/audio/spectrum.rs` `measure` | `FFT_SIZE` 16384 (about 2.7 Hz/bin at 44.1 kHz), `FFT_HOP_SIZE` 8192, Hann window (denominator N-1), bins 1..N/2 |
| Weighting | `a_weighting_amplitude_ratio` | IEC 61672 A-weighting squared, applied per bin before banding |
| Banding | `FINE_BAND_CENTERS_HZ` (29 ISO 266 centres, 25 Hz to 16 kHz), `band_edges`, `band_index` | edges at centre x 2^(+/-1/6), rounded to bins; per-frame band value is summed weighted power divided by bin count, so it is a power spectral density, not band power |
| Frame gate | `SILENCE_GATE_BELOW_PEAK_DB` = 60 | frames more than 60 dB below the loudest frame's weighted power are dropped, identically for all bands |
| Summary | `gated_median_db` | median over kept frames per band, then `10 log10` |
| Broad bands | `BASS_MAX_HZ` 250, `MID_MAX_HZ` 4000, `TREBLE_MAX_HZ` 8000 | feed `bass_db/mid_db/treble_db` and the text verdict `eq_summary` in `ui/components/audio_analysis_modal.rs` (thresholds 8 and 15 dB, which its own comment calls arbitrary) |
| Loudness | `Spectrum::overall_db` | gated mean of unwindowed frame mean-square, no K-weighting; no peak is stored |
| Reference | `eq::suggest` | `target[i] = mean_of_measured_bands + (1.5/3) x (pivot - i)`; `mean` is a plain mean of dB values over all measured bands |
| Gain | `eq::suggest` | `unclamped = -0.5 x (db - target)`; keep if `abs >= 1.5`; boosts dropped if the band is more than 30 dB below the loudest band; clamp to +/-12 (`MAX_SUGGESTED_GAIN_DB`); max 15 bands |
| Q | `eq::suggest` | `q = (2.8 / width).clamp(0.6, 2.8)`, `width` = bands in the merged region |
| Preamp | `eq::preamp_for` | `-(max boost)/2`, clamped to -12..0 |
| Engine | `assets/engine.js` `rebuildEq`, `applyEqValues`, `setNorm` | one Web Audio `BiquadFilterNode` of type `peaking` per band, cascaded, then `preamp` gain, then `norm` gain, then `volume`. Optional imported AutoEQ/PEACE correction (`rebuildCorrection`) sits earlier in the chain with its own preamp. `player.rs` `norm_db` = `(TARGET_LEVEL_DBFS - overall_db)` clamped to +/-8 dB |
| Tempo/key | `src/audio/tempo.rs` | only the middle 90 s (`MAX_SECONDS_FOR_TEMPO`); the spectrum uses the whole track |

Weaknesses visible in the code (each is checked against a source or my arithmetic below):

- W1. Reference stacks two corrections: A-weighting on the data, then a 1.5 dB/octave tilt on the target (`REFERENCE_TILT_DB_PER_OCTAVE`, called "a judgment call" in its comment). See 1.
- W2. `mean` includes every measured band, including bands above a lossy codec's low-pass (the `TREBLE_MAX_HZ` comment describes AAC rolling off above about 8-10 kHz). Those low bands pull the mean down and make the audible bands read "loud".
- W3. `preamp_for` underestimates the combined boost. See 3 and 4.
- W4. Q is derived from region width in bands, and its upper end (2.8) is wider than one measurement band. See 4.
- W5. Amplitude-average mono downmix cancels out-of-phase content.
- W6. `norm_db` ignores peaks, K-weighting and the EQ's own effect on level.
- W7. The frame gate is relative to the loudest frame (-60 dB), far looser than the broadcast gates.
- W8. `models.rs` doc on `AudioAnalysis` still says the bands are "for a real 10-band EQ", which is out of date (cosmetic).

## 1. What should "suggested EQ" be measured against?

### What the sources say

- **Harman target (Olive, Welti, McMullin).** AES Convention 134 paper 8867 [abstract] compared headphone target curves in double-blind tests and found an in-room-loudspeaker-derived target most preferred for both headphones tested. Convention 135 paper 8994 [abstract, via search snippet] had listeners adjust bass and treble of a flat in-room loudspeaker and a matched headphone; it reports a significant spread in preferred bass and treble across individuals. Convention 137 [abstract, via search snippet] reports the main finding held across experience, age, gender and culture. These targets define how a **playback transducer** should respond at the ear. They say nothing about what spectrum a recording should have. The right use here is the one Tanqi already has: the imported AutoEQ correction (`parse_parametric_eq`, `rebuildCorrection`). The secondary claim of a roughly 64/15/21 percent split of listener groups comes from a vendor case study [secondary, unverified].
  - https://aes2.org/publications/elibrary-page/?id=16768 (paper 8867)
  - https://aes2.org/publications/elibrary-page/?id=17500 and https://aes2.org/publications/elibrary-page/?id=17042 appeared in search results for papers 8994 and 137; I could not open them, so the paper-to-id mapping is unverified.
- **Commercial music already has a typical spectral shape.** Pestana et al., "Spectral Characteristics of Popular Commercial Recordings 1950-2010", AES Convention 135, paper 8960 [abstract]: analysed long-term spectra of a large set and report a consistent leaning toward a target equalisation contour from industry practice that also loosely mimics natural acoustic spectra, with year and genre patterns. https://aes2.org/publications/elibrary-page/?id=17010 . I could not read the actual dB/octave numbers, so a quoted "music averages -3 dB/octave" is **unverified**.
- **Mastering tools build targets from music, not from psychoacoustics.** iZotope documents Tonal Balance Control curves as derived from analysing many commercial tracks per genre, shown as a band of acceptable spread, with user-loaded reference tracks as an alternative [secondary-vendor, opened via search snippet]. https://www.izotope.com/en/products/tonal-balance-control-2/features/tonal-balance-curves.html . Matchering matches a user-chosen reference track's RMS, frequency response, peak and stereo width to the target track [opened]. https://github.com/sergree/matchering (README, "Matching + Mastering"). Its code averages loudest-piece STFT magnitudes (4096-point, `defaults.py` `fft_size`), takes the reference/target ratio, smooths it with LOWESS on a log-frequency grid and builds an FIR (`stage_helpers/match_frequencies.py` `get_fir`, `__smooth_exponentially`). Matchering is GPL-3.0, so its code cannot be copied into Tanqi (check Tanqi's licence); the idea is free to reuse.
- **ISO 226:2023** gives equal-loudness contours for pure tones, free-field, binaural, young adults, 20 Hz to 12.5 kHz in third-octave points [secondary catalogue listings; the iso.org page returned 403]. https://webstore.ansi.org/preview-pages/ISO/preview_ISO+226-2023.pdf was listed as a preview. The contours depend on playback level, which Tanqi does not know (device volume, headphones).
- **Pink noise** is equal energy per octave, i.e. a power spectral density falling 3 dB per octave. That is a definition; whether music or "perceptually neutral" playback should match it is not established by anything I could open.

### Which is defensible, which over-claims

- **Defensible for "make this song sound balanced"**: a reference derived from real music, stated as such: "this track's spectrum versus the typical spectrum of tracks in your library (or in a bundled sample set)". It supports a claim of unusual-ness, not of correctness. This is the approach of Tonal Balance Control and Matchering, and it is checkable with a corpus test.
- **Over-claiming**: presenting Harman's curve as a target for music (category error, above); presenting ISO 226 as a correction (pure tones, level-dependent, unknown playback level); or presenting any fixed dB/octave slope as "perceptually neutral" without a measured corpus behind it. The current 1.5 dB/octave constant falls in this last group, by its own comment.

### A concrete problem with the current reference (W1, my arithmetic)

The data is A-weighted per-bin power (a density), and the target tilt is applied on that weighted data. Using the IEC 61672 A-curve as coded in `a_weighting_amplitude_ratio`, a pink-noise density (-3 dB/octave) after A-weighting, relative to 1 kHz, is about -9 dB at 100 Hz and -19 dB at 16 kHz, while the code's target is +5 dB at 100 Hz and -6 dB at 16 kHz. So pink noise would deviate by about -14 dB at 100 Hz and -13 dB at 16 kHz from this reference, i.e. the implied unweighted reference is a bass- and treble-heavy U shape relative to pink. That may happen to match real tracks (the comments say it was tuned on a few songs), but it is not derived from anything, and it is unverified against a corpus. Recommendation 3 replaces it.

## 2. Robust spectrum estimation

### What the sources say

- **Welch averaging**: split the record into overlapping segments, window each, average the periodograms (Welch, IEEE Trans. Audio Electroacoust. 15(2), 1967, 70-73, DOI 10.1109/TAU.1967.1161901) [abstract]. The code's 50% Hann overlap follows this. SciPy's `welch` defaults to a periodic Hann window and 50% overlap, and offers `average='median'` as "more robust" to outliers [opened]. https://docs.scipy.org/doc/scipy/reference/generated/scipy.signal.welch.html
- **Mean versus median**: the median is robust to a crash or drop, which is the stated reason in `measure`. One caveat from my derivation (not a sourced claim): for a noise-like band the median of single-frame power is below the mean by a fixed factor, and that bias is the same in every band, so it cancels in a comparison between bands. It would not cancel against an externally measured reference computed with a mean, so use the same estimator for both.
- **Fractional-octave bands**: IEC 61260-1:2014 specifies octave and fractional-octave filters with base-10 band ratios and class 1/2 tolerances [secondary catalogue listings]. https://webstore.iec.ch/publication/5063 . The code uses base-2 edges (2^(1/6)); the difference from base-10 (10^(0.05)) is under 0.1 percent per edge, which is immaterial next to the resolution issues below.
- **ERB/Bark banding**: Glasberg and Moore (Hearing Research 47, 1990, 103-138) give ERB as linear in frequency, about 24.7 x (4.37 f/kHz + 1) Hz [secondary, formula not verified against the paper]. Essentia's `ERBBands` is a gammatone-style filterbank defaulting to 40 bands from 50 Hz [opened]. https://essentia.upf.edu/reference/std_ERBBands.html . ERB bands are about 1/3 octave wide near 1 kHz and wider in octave terms below it, so 1/3-octave bands over-resolve the bass relative to hearing.
- **K-weighting** is part of BS.1770 loudness: a high shelf (about 1.68 kHz, +4 dB, Q 0.707) followed by a high-pass (about 38 Hz, Q 0.5), per the ebur128 crate's `filter_coefficients` [opened, MIT source]. https://github.com/sdroege/ebur128 (src/filter.rs). It is designed to predict loudness, not tonal balance, so it is not a good weighting for the balance spectrum.
- **Gating**: EBU Tech 3341 section 2.3 specifies 400 ms blocks at 75% overlap, an absolute gate at -70 LUFS and a relative gate 10 LU below the absolute-gated level [opened]. https://tech.ebu.ch/docs/tech/tech3341.pdf . R 128 (2023 edition, recommendation l) says to measure the signal in its entirety [opened]. https://tech.ebu.ch/docs/r/r128.pdf

### Assessment of the current estimator

- Whole track plus silence gate is the right shape. 16384 points gives a bin of about 2.7 Hz; the 25 Hz band is only about 2 bins wide, so its estimate is noisy (the code comment acknowledges this trade-off). Nothing in the sources says what window is "right"; it is a resolution/variance trade. A cheap, evidence-neutral improvement is to leave the FFT size alone and smooth across adjacent bands (section 4).
- The -60 dB gate removes only near-silence. The broadcast approach drops blocks 10 LU under the foreground level. For tonal balance a gate of that tightness is probably too aggressive (it would drop quiet verses), but -60 dB below the loudest frame is looser than anything in a source. I have no source for a better number; treat any choice between -20 and -40 dB as a tunable to test on a corpus.
- Amplitude-averaged mono (W5): a wide-stereo or out-of-phase element cancels. Averaging the L and R power spectra (or measuring mid) avoids that. I did not find a source that quantifies the problem for music; it follows from arithmetic on the downmix.
- Lossy sources: the existing finding that AAC lacks content above 8-10 kHz is documented only in the code comment (not independently verified here). The robust fix is to detect the effective low-pass (highest band within N dB of the loudest) and exclude bands above it from `mean`, not to hard-cap treble.

## 3. Loudness normalisation

### What the sources say

- **BS.1770 / R 128 integrated loudness**: K-weighted, channel-summed, gated at an absolute -70 LUFS and a relative -10 LU (Tech 3341 section 2.3, above; the relative gate was changed from -8 to -10 LU in 2011 per R 128's revision table [opened]). BS.1770-5 (11/2023) Annex 1 specifies the algorithm and Annex 2 the true-peak measurement [table of contents opened]. https://itu.int/dms_pubrec/itu-r/rec/bs/R-REC-BS.1770-5-202311-I!!TOC-HTM-E.htm . The ITU PDF body was blocked, so the 4x-oversampling statement comes from the crate source below, not from the ITU text.
- **R 128 limits**: programme loudness target -23.0 LUFS, true peak not above -1 dBTP in production [opened]. https://tech.ebu.ch/docs/r/r128.pdf . This is a broadcast target, not a music-player target.
- **ReplayGain 2.0** uses BS.1770 loudness and a -18 LUFS reference, and requires player-side clipping prevention either by limiting or by reducing gain based on stored peak metadata [opened via Internet Archive copy; the page is labelled a proposal "under construction"]. https://wiki.hydrogenaudio.org/index.php?title=ReplayGain_2.0_specification (sections "Reference level", "Clipping prevention", "Reduced gain"). It also notes that after normalisation, quieter dynamic tracks have louder peaks than loud compressed ones, which is exactly the boost-clips case.
- **ffmpeg** `ebur128` filter reports M, S, I and LRA and, with `peak=sample+true`, sample and true peak; mono input is measured as-is unless `dualmono` is set [opened]. https://ffmpeg.org/ffmpeg-filters.html (section `ebur128`). A local ffmpeg exists on this machine (`~/.local/bin/ffmpeg`), SoX does not.
- **The `ebur128` crate**: Rust port of libebur128; M, S, I, LRA, sample peak and true peak modes; handles all sample rates; README says it passes all EBU Tech 3341 and 3342 tests and has performance comparable to the C library [opened]. https://github.com/sdroege/ebur128 . crates.io metadata: version 0.1.10, MIT, updated 2024-10-26, MSRV 1.60, about 1.05 million downloads [opened via API at fetch time; I did not check whether a newer release exists]. https://crates.io/api/v1/crates/ebur128 . Dependencies listed on docs.rs: bitflags, dasp_frame, dasp_sample, smallvec [opened]. https://docs.rs/ebur128/latest/ebur128/ . Source: 400 ms first block then 100 ms steps for 75% overlap, a histogram mode flag, true peak via a polyphase FIR interpolator oversampling 4x below 96 kHz (src/ebur128.rs) [opened]. The README says nothing about WASM or no_std; since the dependencies are small and pure Rust it very likely builds for Android, but that is an inference to confirm with `cargo check --target aarch64-linux-android`.
- **Essentia** has a `LoudnessEBUR128` algorithm following the same standards [opened] (https://essentia.upf.edu/reference/std_LoudnessEBUR128.html), but Essentia is a C++ library with a different licence and a heavy build, so it is not a fit for a phone Rust app.

### Assessment of the current approach (W6)

- `overall_db` is unweighted RMS, so bass-heavy tracks read louder than they sound and are attenuated, and the reverse for thin tracks. K-weighting is exactly the correction BS.1770 adds. The gated mean also uses a -60 dB gate, not -70 LUFS absolute plus -10 LU relative.
- `TARGET_LEVEL_DBFS = -14` as unweighted RMS is not the same as -14 LUFS and sits well above the ReplayGain reference of -18 LUFS. I have not measured how far apart they are for real music; compare with ffmpeg on a sample set before changing the target.
- No peak is stored (`AudioAnalysis` has no peak field; see `src/models.rs`), so `norm_db` can boost by up to `MAX_NORMALIZATION_DB` = 8 dB with no idea whether that exceeds full scale. ReplayGain's "reduced gain" strategy (cap gain by the stored peak) is the sourced remedy.
- Headroom with EQ: the engine chain is peaking filters, `preamp`, `norm`. The chain's final level is peak + EQ's peak boost + preamp + norm. Nothing ensures this stays below 0 dBFS. The preamp is -max_boost/2 (W3). Using my own RBJ-cookbook peaking response (the shape Web Audio's `peaking` type implements; the W3C Web Audio spec section 1.13 says the peaking `gain` is "the boost, in dB" and that Q controls width, https://www.w3.org/TR/webaudio-1.1/ ; I did not verify the spec's exact coefficients), adjacent same-sign bands sum: +6 dB at 500, 630 and 800 Hz with Q 2.8 gives a +10.4 dB maximum; eight adjacent +6 dB bands give +13.0 dB. A preamp of -3 dB leaves +7.4 and +10 dB net boost respectively. The engine's `BiquadFilterNode.getFrequencyResponse` (spec section 1.13.3) can compute the exact total in JS, but doing it in Rust keeps it testable.
- Not verified: whether the browser output clips hard at +/-1.0 or applies any limiting; I did not find that in the spec text I read.

## 4. Gain and Q selection

### What the sources say

- **AutoEQ** (jaakkopasanen/AutoEq, MIT-licensed project) [opened source files]:
  - Smoothing: `smoothen` uses Savitzky-Golay filters, 1/12-octave window below about 6 kHz and a 2-octave window above, blended with a sigmoid between 6 and 8 kHz (`constants.py`: `DEFAULT_SMOOTHING_WINDOW_SIZE` = 1/12, `DEFAULT_TREBLE_SMOOTHING_WINDOW_SIZE` = 2.0, `DEFAULT_TREBLE_SMOOTHING_F_LOWER/UPPER` = 6000/8000).
  - `equalize` (`frequency_response.py`): target is the negated smoothed error; peaks and dips found with `find_peaks(prominence=1)`; slope limited to `DEFAULT_MAX_SLOPE` = 18 dB/octave; positive gain clipped to `DEFAULT_MAX_GAIN` = 6 dB, then smoothed again at 1/5 octave.
  - Parametric fit (`peq.py`): initial peaking filters are placed at the largest `width x height` peaks of the remaining target, Q from the peak width in octaves via `q = sqrt(2^bw)/(2^bw - 1)`, then an SLSQP fit (`fmin_slsqp`) minimises RMS error. Limits: Q 0.18248 to 6.0, gain -20 to +20 dB, centre 20 Hz to 10 kHz (`DEFAULT_PEAKING_FILTER_*`). A `sharpness_penalty` discourages high gain at high Q (about 18 dB per 1/Q unit). Above 10 kHz the loss uses only the mean, "treats +10 kHz range as average value" (README, 2022-09-18 update). Presets such as `8_PEAKING_WITH_SHELVES` use 8 peaking filters plus two shelves.
  - Files: https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/autoeq/frequency_response.py , .../peq.py , .../constants.py , and the README https://github.com/jaakkopasanen/AutoEq . The README does not document the algorithm; the code is the source.
  - Note: AutoEQ corrects a **measured transducer** against a target, with repeatable measurements. That is a different problem from inferring a correction from a single music file's long-term spectrum.
- **Audibility of deviations**: Bücklein's study of just-perceptible response irregularities reports peaks are far more audible than equal-size dips [abstract]. https://aes2.org/publications/elibrary-page/?id=10291 (the id appeared in a search result for this topic; I could not open it to confirm the title). Toole and Olive, JAES 36(3), 1988, 122-142, "The Modification of Timbre by Resonances" [abstract, could not open] is the standard reference that audibility depends on resonance width, amplitude and programme material. https://aes2.org/publications/elibrary-page/?id=5163 . A citing paper's summary of Moore et al. (narrowband peaks up to 0.7 octave detectable at roughly 1-2 dB, notches in broadband noise needing about 7-10 dB) is [secondary, unverified] and measured on noise, not music.
- **Q versus bandwidth**: for a peaking biquad, bandwidth in octaves is about (2/ln 2) x asinh(1/(2Q)) (Audio EQ Cookbook relation, my arithmetic): Q 0.6 is 2.19 octaves, Q 1 is 1.39, Q 2.8 is 0.51, Q 4.32 is 0.33 (one 1/3-octave band).
- **Parametric fit tools elsewhere**: librosa's `spectral_contrast` (peak-versus-valley energy per octave band, defaults 6 bands from 200 Hz, quantile 0.02) is a descriptor, not an EQ designer [opened]. https://librosa.org/doc/0.10.2/generated/librosa.feature.spectral_contrast.html . Essentia likewise provides descriptors, not a fitter. I found no library that fits a corrective EQ to a single track's long-term spectrum other than Matchering (reference-based, FIR rather than parametric).

### Assessment of the current gain/Q rules

- `-0.5 x deviation` is a damped correction; AutoEQ applies full correction but caps boosts at 6 dB and slopes at 18 dB/octave. The half-correction factor and 1.5 dB threshold are the author's judgment (no source). The effective threshold is a 3 dB deviation, then the first band appears at 1.5 dB gain: a step, not a ramp. That is acceptable but means small deviations are silently ignored while a 3 dB one gets a distinct filter.
- Boost and cut share the same +/-12 dB cap. The sourced asymmetry (peaks more audible than dips; AutoEQ caps boost at 6) argues for a smaller boost cap than cut cap.
- W4: for a one-band region the code uses Q 2.8 (0.51 octave) over a 0.33-octave measurement, which is reasonable. For a 4-band region (1.33 octaves) it uses Q 0.7, about 1.9 octaves, so the filter's skirts correct well outside the measured region. Setting bandwidth from the region width (width/3 octaves) gives Q about 1.05 for width 4.
- Gain is read at the peak band only, while the correction applies over the whole width. Taking the mean deviation of the region (or a smoothed peak) would avoid overshooting at the flanks.
- 1/3-octave points are noisy at the bass end (25-40 Hz bands span 2-4 bins). A three-point smooth across bands before peak picking, as AutoEQ does with Savitzky-Golay, prevents a single noisy low band from becoming a high-Q filter.
- Cascaded filters interact (section 3). The engine applies all bands in series, so each band's gain is not independent; a final check of the combined response against the intended gain is cheap.

## 5. Genre, dynamics, stereo, and does per-track EQ help?

- **What the evidence says.** Pestana et al. (section 1) report a consistent spectral contour across commercial recordings with year and genre structure: tracks largely already sit near a shared shape, so large measured deviations are more likely deliberate (bass-forward dance, bright lo-fi, acoustic) than errors. The code history says the same: a flat target made a bass-forward K-pop track "under-bassed" (comment on `REFERENCE_TILT_DB_PER_OCTAVE`).
- **Listener preference differs.** Olive, Welti and McMullin (8994) report significant individual variation in preferred bass and treble; AES 137 reports the main target finding was stable across demographics (section 1). Both concern transducer targets, not per-track correction. So an app-wide, user-chosen tone control has better support than an automatic per-track one.
- **Per-track adaptive listener-side EQ.** I found no primary study showing listeners prefer it for music. The adaptive-EQ literature I found (Hafezi and Reiss, JAES 63(5), 2015, masking-based multitrack EQ [abstract]; Ma, Reiss, Black, AES 134, 2013 [could not open]) is about mixing multitrack sessions, where the engineer is the user and the goal is masking reduction. It is not evidence for correcting finished masters at playback. I rate the evidence for the product idea as **weak**.
- **When it is more likely to help**: obvious outliers (a mono-ish lo-fi rip with a 3 kHz resonance, a track with a rumble or a low-pass), or when the user's own library is the reference and the track sits far outside it.
- **When it hurts**: genre conventions, intentional filtering, dense/loud masters (boost clips), wide stereo (mono downmix misleads), very dynamic music (median spectrum describes the quiet passages), and double-correcting with an imported headphone correction (the AutoEQ profile already moves the spectrum).
- **Stereo and dynamics are not measured at all** today. Stereo width or correlation and a crest factor / loudness range value are cheap to compute (ebur128 gives LRA via `Mode::LRA`) and would help the UI decide when not to suggest anything.

## 6. Ranked improvements for this codebase

Constraints: Rust, rustfft, phone, a couple of seconds per track, and the engine stays a cascade of `peaking` biquads plus `preamp` and `norm` gains (no change proposed to `engine.js` graph).

Rank by (value, effort, risk):

| # | Change | Value | Effort | Risk |
|---|---|---|---|---|
| 1 | Preamp from actual summed response | High | S | Low |
| 2 | `ebur128` integrated LUFS + true peak, cap boosts by peak | High | M | Low-Med (new dep, DB column) |
| 3 | Strength control (scale band gains and preamp by 0..100%) | Med-High | S | Low |
| 4 | Exclude dead HF bands from `mean`; reference excludes bands above detected low-pass | Med-High | S | Low |
| 5 | Q from region width; boost cap 6 dB, cut cap 12 dB; smooth 3 bands before peak picking | Medium | S | Low-Med (changes output) |
| 6 | Empirical reference from a corpus, unweighted, replace A-weight+tilt | High but unproven | M | Med (retune everything) |
| 7 | L/R power-average spectrum instead of amplitude-average mono | Low-Med | S-M | Low (decode returns 2 channels, 2x memory) |
| 8 | Derive the verdict text from fine bands/reference deviation | Low | S | Low |
| 9 | Suggest nothing when deviation is within the corpus spread | Medium | S | Low |

### 1. Preamp from the summed response

Change in `eq.rs`: replace `preamp_for` with a function that evaluates the magnitude response of the cascade of peaking biquads (RBJ cookbook form; `A = 10^(gain/40)`, `alpha = sin(w0)/(2Q)`) on a log grid (say 20 Hz to 20 kHz, 1/24-octave) at the file's sample rate (assume 48 kHz if unknown) and returns `-max(0, peak_db)` rounded up to 0.1 dB, clamped at `-MAX_SUGGESTED_GAIN_DB`. Keep returning 0 when nothing boosts.
How to test:
- Unit: three adjacent +6 dB / Q 2.8 bands must give a preamp at or below about -10.4 dB; a single +6 dB band must give about -6.0 (not -3.0 as now).
- Cross-check: generate a log sweep or pink noise, apply the same bands in ffmpeg (`equalizer=f=..:t=q:w=..:g=..` chained, then `volume=<preamp>dB`), and check `astats` peak level does not exceed the input peak (https://ffmpeg.org/ffmpeg-filters.html, `astats`). I did not verify ffmpeg's `equalizer` coefficients equal Web Audio's; a difference of a few tenths of a dB is expected to be irrelevant, but confirm.
- Existing test `eq_suggestion_cuts_the_dominant_tone_and_suggests_no_boosts` (preamp stays 0) must still pass.

### 2. BS.1770 loudness and peak

Change: add `ebur128 = "0.1"` (MIT) to `Cargo.toml`; have `decode_to_mono` also return interleaved stereo (or a `decode_stereo` variant); feed `EbuR128::new(2, rate, Mode::I | Mode::TRUE_PEAK)`; store `integrated_lufs` and `true_peak_dbtp` on `BandLevels`/`AudioAnalysis`/`db.rs` (new nullable columns via the existing migration pattern); in `norm_db` use `(TARGET_LUFS - lufs).min(-1.0 - true_peak_dbtp - eq_headroom)` so a boost never pushes the peak past -1 dBTP (R 128 limit; ReplayGain "reduced gain"). Keep the +/-8 clamp. Choose the target after measuring (candidate -18 LUFS, the ReplayGain 2.0 value; the current -14 dBFS RMS may be louder). Fall back to the current RMS for rows without the new columns.
How to test:
- Synthetic: a 997 Hz sine at -20 dBFS in both channels should read -23.0 LUFS (BS.1770 constant; the Tech 3341 test set includes 400 ms at -23.0 dBFS style cases, https://tech.ebu.ch/docs/tech/tech3341.pdf); a fs/4 sine at 0.5 full scale has a true peak of -6.0 dBTP in Tech 3341's table.
- Reference tool: `ffmpeg -nostats -i track.flac -af ebur128=peak=true -f null -` and compare I and True peak to the crate within about 0.1 LU/dB on 20 real tracks.
- Timing: `ebur128` on a 5-minute track should be a small fraction of decode time; measure on the phone. Confirm Android build with `cargo check --target aarch64-linux-android`.
- Risk: stereo samples double the in-memory buffer (the code already notes phone memory limits for tempo). Feed the analyser per decoded packet instead of holding a second buffer.

### 3. Strength control

Change: scale `suggested_gain_db` and `preamp_db` by a user setting before `cmd("setEq", ...)` in `ui/player.rs` (around the `bands_from_analysis` call sites and `set_preamp_gain`). Default 50-70% because the evidence for full correction is weak (section 5). No engine change.
How to test: set 0% and check the engine receives all-zero gains; 100% reproduces today's values; unit test the scaling helper.

### 4. Dead-band exclusion

Change in `eq::suggest`: compute `mean` (and `loudest`) over bands at or below an effective low-pass: the highest band whose level is within a threshold (start at 40 dB; tune) of the loudest band. Bands above it are treated as `None`.
How to test: synthetic 1/3-octave profile with a smooth tilt up to 8 kHz and -80 dB above (mimicking AAC); today the 8 kHz-and-below bands read "loud" and get cuts (this is what the existing `bands_with_no_usable_bins_are_skipped...` test guards for `None` bands); after the change, the baseline and gapped suggestions should match within 0.5 dB. Verify on a few real AAC and FLAC files of the same song that the suggestion agrees.

### 5. Q, caps, smoothing

Changes in `eq::suggest`: `bandwidth_oct = (width as f32) / 3.0`; `q = 1.0 / (2.0 * (bandwidth_oct * LN_2 / 2.0).sinh())`, clamped to AutoEQ-like 0.5..4 (AutoEQ's limits are 0.18..6, `constants.py`); separate `MAX_BOOST_DB` (start at 6, AutoEQ's `DEFAULT_MAX_GAIN`) and `MAX_CUT_DB` (12); light 3-tap smoothing of `unclamped` before `significant`/peak picking; gain read as the mean of the merged region rather than the peak band.
How to test: existing synthetic-tone tests still land the filter on 1 kHz/3150 Hz; add a synthetic broad shelf (e.g. +6 dB over 4 bands) and assert the resulting cascade response, evaluated with the same RBJ routine as in item 1, is within 1.5 dB of the intended correction across the region and under 1 dB outside +/- one band of it.

### 6. Empirical reference (the one that decides accuracy)

Change: add a dev-only test or binary that runs `spectrum::measure` on a folder of tracks and dumps `fine_db` unweighted; derive the median and the 10th/90th percentile per band; ship them as a constant table (or compute from the user's analysed library in `db.rs`); deviation = band - median (after level-aligning on 200 Hz-4 kHz), suggestion only outside the spread. Remove `a_weighting_amplitude_ratio` from the balance path (keep it only if it earns its place in the 3-band verdict).
How to test: leave-one-out on the corpus: a typical track should yield zero or few suggested bands; known outliers (a bass-heavy and a thin track) should yield the expected sign. Report the share of tracks that get any suggestion; if it is above roughly a third, thresholds are too eager (my heuristic, no source).
Caveat: the sources give no reference to calibrate against; this is a measurement project, not a lookup.

### 7-9. Smaller items

- 7: add L/R handling in `decode.rs` and average the two power spectra in `measure`; test with a synthetic out-of-phase pair (today's mono sum reads silence; the stereo-aware path should not).
- 8: keep `eq_summary` but compute it from the same fine-band deviations as the suggestions so the text and the sliders cannot disagree (they can today, because broad bands cut at 8 kHz and use A-weighting while the bands do not).
- 9: if every kept deviation is inside the corpus spread, return no bands and the verdict "balanced"; this directly addresses "fights the artist's intent".

### Not recommended

- Harman or ISO 226 as the music reference (section 1).
- K-weighted balance spectrum, ERB gammatone filterbank, Essentia, or a Matchering-style FIR (licence GPL-3.0 for Matchering; FIR would also need an engine change, since the engine is parametric).
- Cutting the FFT size to save time: the current cost is small; the 16384 window exists to resolve low bands.

## Sources

Opened (text read):
- EBU R 128 (2023): https://tech.ebu.ch/docs/r/r128.pdf
- EBU Tech 3341: https://tech.ebu.ch/docs/tech/tech3341.pdf
- ITU-R BS.1770-5 table of contents only: https://itu.int/dms_pubrec/itu-r/rec/bs/R-REC-BS.1770-5-202311-I!!TOC-HTM-E.htm (body PDF blocked)
- ebur128 crate: https://github.com/sdroege/ebur128 (README, src/ebur128.rs, src/filter.rs), https://docs.rs/ebur128/latest/ebur128/ , https://crates.io/api/v1/crates/ebur128
- ReplayGain 2.0 (Internet Archive copy of the wiki page): https://wiki.hydrogenaudio.org/index.php?title=ReplayGain_2.0_specification
- ffmpeg filters (`ebur128`, `astats`): https://ffmpeg.org/ffmpeg-filters.html
- AutoEQ: https://github.com/jaakkopasanen/AutoEq , files autoeq/frequency_response.py, autoeq/peq.py, autoeq/constants.py (raw.githubusercontent.com, master)
- Matchering: https://github.com/sergree/matchering (README, matchering/defaults.py, matchering/stage_helpers/match_frequencies.py)
- Essentia: https://essentia.upf.edu/reference/std_LoudnessEBUR128.html , https://essentia.upf.edu/reference/std_ERBBands.html
- librosa: https://librosa.org/doc/0.10.2/generated/librosa.feature.spectral_contrast.html
- SciPy `welch`: https://docs.scipy.org/doc/scipy/reference/generated/scipy.signal.welch.html
- W3C Web Audio API, BiquadFilterNode section 1.13: https://www.w3.org/TR/webaudio-1.1/

Abstract or snippet only (full text not read):
- Olive, Welti, McMullin, AES 134 paper 8867: https://aes2.org/publications/elibrary-page/?id=16768
- Olive, Welti, McMullin, AES 135 paper 8994 and AES 137 (ids 17500, 17042, mapping unverified)
- Pestana et al., AES 135 paper 8960: https://aes2.org/publications/elibrary-page/?id=17010
- Toole and Olive, JAES 36(3), 1988: https://aes2.org/publications/elibrary-page/?id=5163
- Bücklein (AES e-library id 10291, title not confirmed): https://aes2.org/publications/elibrary-page/?id=10291
- Hafezi and Reiss, JAES 63(5), 2015: https://aes2.org/publications/elibrary-page/?id=17637
- Welch 1967, DOI 10.1109/TAU.1967.1161901 (via IBM research listing)
- Glasberg and Moore, Hearing Research 47, 1990, 103-138 (citation only)

Secondary or vendor (unverified as evidence):
- ISO 226:2023 catalogue listings (iso.org returned 403): https://webstore.ansi.org/preview-pages/ISO/preview_ISO+226-2023.pdf
- IEC 61260-1:2014 listing: https://webstore.iec.ch/publication/5063
- iZotope Tonal Balance Control: https://www.izotope.com/en/products/tonal-balance-control-2/features/tonal-balance-curves.html

Not verified at all: BS.1770 K-weighting and gating constants as printed in the ITU text (taken from the crate and EBU documents instead); the dB/octave spectral tilt of average music; absolute audibility thresholds for music; whether Web Audio output hard-clips at +/-1.0; whether ffmpeg's `equalizer` coefficients match Web Audio's `peaking`.
