//! End-to-end tests for audio analysis on synthetic WAVs: decode, measure,
//! tempo and EQ together, through `analyze_band_levels`.

use super::spectrum::SILENT_DB;
use super::*;


/// Builds a minimal canonical WAV (RIFF/WAVE, 16-bit PCM, mono) with a
/// single sine tone, so band-level tests don't depend on any real
/// network audio or external encoder.
fn sine_wav(freq: f32, sample_rate: u32, duration_secs: f32) -> Vec<u8> {
    let num_samples = (sample_rate as f32 * duration_secs) as u32;
    let data_len = num_samples * 2; // 16-bit mono
    let mut wav = Vec::with_capacity(44 + data_len as usize);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for n in 0..num_samples {
        let t = n as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * freq * t).sin() * i16::MAX as f32 * 0.8;
        wav.extend_from_slice(&(sample as i16).to_le_bytes());
    }

    wav
}

#[test]
fn low_frequency_tone_reads_as_bass_heavy() {
    let wav = sine_wav(100.0, 44_100, 1.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");

    assert!(
        levels.bass_db > levels.mid_db,
        "bass {} should exceed mid {}",
        levels.bass_db,
        levels.mid_db
    );
    assert!(
        levels.bass_db > levels.treble_db,
        "bass {} should exceed treble {}",
        levels.bass_db,
        levels.treble_db
    );
}

#[test]
fn high_frequency_tone_reads_as_treble_heavy() {
    let wav = sine_wav(8_000.0, 44_100, 1.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");

    assert!(
        levels.treble_db > levels.bass_db,
        "treble {} should exceed bass {}",
        levels.treble_db,
        levels.bass_db
    );
    assert!(
        levels.treble_db > levels.mid_db,
        "treble {} should exceed mid {}",
        levels.treble_db,
        levels.mid_db
    );
}

#[test]
fn eq_suggestion_cuts_the_dominant_tone_and_suggests_no_boosts() {
    // 1kHz is a FINE_BAND_CENTERS_HZ point exactly - that band should
    // read far louder than everything else, which is near-total
    // silence (FFT leakage aside).
    let wav = sine_wav(1_000.0, 44_100, 1.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");

    let dominant = levels
        .eq_bands
        .iter()
        .find(|b| (b.hz - 1_000.0).abs() < 1.0)
        .expect("expected a suggested band right at the tone's own frequency");
    assert!(
        dominant.suggested_gain_db < 0.0,
        "expected a cut for the dominant band, got {}",
        dominant.suggested_gain_db
    );
    // Everything else is near-silence - boosting any of it would just
    // amplify noise/FFT leakage, not correct a real tonal imbalance, so
    // nothing should suggest a boost here. A stray small cut elsewhere
    // (from the reference tilt alone, not this filter) is harmless and
    // not asserted against.
    assert!(
        levels.eq_bands.iter().all(|b| b.suggested_gain_db <= 0.0),
        "a pure tone shouldn't suggest boosting any near-silent band: {:?}",
        levels.eq_bands
    );

    // Nothing is being boosted (the only suggestion here is a cut), so
    // there's no clipping risk to leave headroom for - preamp should
    // stay at 0, not pull back for a boost that was correctly never
    // suggested.
    assert_eq!(levels.preamp_db, 0.0);
}

#[test]
fn adaptive_suggestion_can_land_on_a_frequency_a_fixed_10band_grid_never_would() {
    // 31.5/63/125/.../16000 (a classic 10-band layout) has nothing at
    // 3150Hz - a fixed grid could only ever spread this tone's
    // correction across the whole 2000-4000Hz octave. The fine
    // (1/3-octave) grid used here should land it exactly on the tone's
    // own frequency instead.
    let wav = sine_wav(3_150.0, 44_100, 1.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");

    let band = levels
        .eq_bands
        .iter()
        .find(|b| (b.hz - 3_150.0).abs() < 1.0)
        .expect("expected a band suggested exactly at the tone's own 1/3-octave center");
    assert!(band.suggested_gain_db < 0.0, "expected a cut, got {}", band.suggested_gain_db);
}

#[test]
fn resolves_a_deep_bass_band_narrower_than_a_coarse_ffts_own_bin_spacing() {
    // Regression test for a real measurement bug. The 1/3-octave band
    // centered on 31.5Hz spans only 28.1-35.4Hz - narrower than a
    // 4096-point FFT's ~10.8Hz bins at 44.1kHz, so its bin range
    // rounded to *zero bins* and the band always reported as total
    // silence no matter what the track actually did down there. That
    // both made a real deep-bass standout impossible to resolve at all,
    // and dragged the reference mean every other band is compared
    // against down with it (skewing their suggested gains too). Which
    // band degenerates depends on the sample rate - 31.5Hz at 44.1kHz,
    // 25Hz at 48kHz - so both are checked here.
    for (freq, sample_rate) in [(31.5f32, 44_100u32), (25.0, 48_000)] {
        let wav = sine_wav(freq, sample_rate, 2.0);
        let levels = analyze_band_levels(wav).expect("should decode and analyze");
        let band = levels
            .eq_bands
            .iter()
            .find(|b| (b.hz - freq).abs() < 1.0)
            .unwrap_or_else(|| {
                panic!("expected a band at {freq}Hz ({sample_rate}Hz file), got {:?}", levels.eq_bands)
            });
        assert!(
            band.suggested_gain_db < 0.0,
            "the one dominant tone should read as needing a cut, got {}",
            band.suggested_gain_db
        );
        assert!(
            band.measured_db > SILENT_DB,
            "band at {freq}Hz measured as digital silence - it has no FFT bins again"
        );
    }
}

/// A tone followed by an equal stretch of digital silence - stands in
/// for a track with a long silent intro/outro, or the lead-out either
/// side of one.
fn tone_then_silence_wav(freq: f32, sample_rate: u32, tone_secs: f32, silence_secs: f32) -> Vec<u8> {
    let tone_samples = (sample_rate as f32 * tone_secs) as u32;
    let silent_samples = (sample_rate as f32 * silence_secs) as u32;
    let num_samples = tone_samples + silent_samples;
    let data_len = num_samples * 2;
    let mut wav = Vec::with_capacity(44 + data_len as usize);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());

    for n in 0..tone_samples {
        let t = n as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * freq * t).sin() * i16::MAX as f32 * 0.8;
        wav.extend_from_slice(&(sample as i16).to_le_bytes());
    }
    for _ in 0..silent_samples {
        wav.extend_from_slice(&0i16.to_le_bytes());
    }

    wav
}

#[test]
fn silence_does_not_drag_down_the_measured_spectrum() {
    // Half this file is digital silence. Averaging it in as though it
    // were part of the music (which is what summing every frame's
    // energy did) pulls the measured level of every band down toward
    // the noise floor - the silence gate plus a median-across-frames
    // should leave the music's own profile essentially untouched.
    let tone_only = analyze_band_levels(sine_wav(1_000.0, 44_100, 2.0))
        .expect("should decode and analyze");
    let with_silence = analyze_band_levels(tone_then_silence_wav(1_000.0, 44_100, 2.0, 2.0))
        .expect("should decode and analyze");

    let measured_at_1k = |levels: &BandLevels| {
        levels
            .eq_bands
            .iter()
            .find(|b| (b.hz - 1_000.0).abs() < 1.0)
            .map(|b| b.measured_db)
            .expect("expected a band at the tone's own frequency")
    };
    let clean = measured_at_1k(&tone_only);
    let gated = measured_at_1k(&with_silence);
    assert!(
        (clean - gated).abs() < 1.5,
        "1kHz measured {clean}dB alone but {gated}dB with an equal stretch of silence appended - the silence is being averaged into the profile"
    );
}

#[test]
fn silence_does_not_drag_down_measured_loudness() {
    // `overall_db` is what playback levels tracks against (see `norm_db` in
    // ui/player.rs, which pulls every track toward a fixed target using it),
    // so an ungated RMS over the whole file made a track with a long silent
    // tail read as quieter than it sounds - and therefore get boosted on
    // playback for no reason.
    let tone_only = analyze_band_levels(sine_wav(1_000.0, 44_100, 2.0))
        .expect("should decode and analyze");
    let with_silence = analyze_band_levels(tone_then_silence_wav(1_000.0, 44_100, 2.0, 2.0))
        .expect("should decode and analyze");

    assert!(
        (tone_only.overall_db - with_silence.overall_db).abs() < 1.0,
        "loudness read {}dB for the tone alone but {}dB with an equal stretch of silence appended",
        tone_only.overall_db,
        with_silence.overall_db
    );
}

/// Sums N sine tones into one WAV - lets a test probe multiple
/// simultaneous spectral standouts, which a single-tone `sine_wav`
/// can't.
fn multi_tone_wav(freqs: &[f32], sample_rate: u32, duration_secs: f32) -> Vec<u8> {
    let num_samples = (sample_rate as f32 * duration_secs) as u32;
    let data_len = num_samples * 2; // 16-bit mono
    let mut wav = Vec::with_capacity(44 + data_len as usize);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());

    let amp_per_tone = 0.8 / freqs.len() as f32;
    for n in 0..num_samples {
        let t = n as f32 / sample_rate as f32;
        let sample: f32 = freqs
            .iter()
            .map(|&f| (2.0 * std::f32::consts::PI * f * t).sin() * amp_per_tone)
            .sum();
        wav.extend_from_slice(&((sample * i16::MAX as f32) as i16).to_le_bytes());
    }

    wav
}

#[test]
fn adaptive_suggestion_produces_separate_bands_for_separate_standout_frequencies() {
    let wav = multi_tone_wav(&[200.0, 3_150.0], 44_100, 1.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");

    assert!(
        levels.eq_bands.iter().any(|b| (b.hz - 200.0).abs() < 1.0),
        "expected a band near 200Hz: {:?}",
        levels.eq_bands
    );
    assert!(
        levels.eq_bands.iter().any(|b| (b.hz - 3_150.0).abs() < 1.0),
        "expected a band near 3150Hz: {:?}",
        levels.eq_bands
    );
}

#[test]
fn adaptive_suggestion_band_count_is_not_fixed_at_ten() {
    // A single pure tone leaves virtually the entire rest of the
    // spectrum at or near true silence - correctly nothing to suggest
    // there, unlike a fixed 10-band layout which always reports all 10
    // regardless of whether a track's spectrum actually warrants it.
    let wav = sine_wav(1_000.0, 44_100, 1.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");
    assert!(
        levels.eq_bands.len() < 10,
        "expected fewer than 10 adaptive bands for a single pure tone, got {}",
        levels.eq_bands.len()
    );
}

/// Same WAV builder as `sine_wav`, but silent except for a short
/// percussive-ish burst repeated every `60/bpm` seconds - a synthetic
/// "click track" so BPM detection tests don't depend on any real song.
fn click_track_wav(bpm: f32, sample_rate: u32, duration_secs: f32) -> Vec<u8> {
    let num_samples = (sample_rate as f32 * duration_secs) as u32;
    let data_len = num_samples * 2; // 16-bit mono
    let mut wav = Vec::with_capacity(44 + data_len as usize);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());

    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());

    let interval_samples = ((60.0 / bpm) * sample_rate as f32) as u32;
    let click_len_samples = (sample_rate as f32 * 0.01) as u32; // 10ms burst
    for n in 0..num_samples {
        let phase = n % interval_samples;
        let sample = if phase < click_len_samples {
            let t = phase as f32 / sample_rate as f32;
            (2.0 * std::f32::consts::PI * 1_000.0 * t).sin() * i16::MAX as f32 * 0.8
        } else {
            0.0
        };
        wav.extend_from_slice(&(sample as i16).to_le_bytes());
    }

    wav
}

#[test]
fn detects_bpm_of_a_click_track() {
    let wav = click_track_wav(120.0, 44_100, 10.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");
    let bpm = levels.bpm.expect("should detect a tempo from a clear beat");
    assert!((bpm - 120.0).abs() < 5.0, "expected ~120 bpm, got {bpm}");
}

#[test]
fn detects_bpm_at_a_different_tempo() {
    let wav = click_track_wav(90.0, 44_100, 10.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");
    let bpm = levels.bpm.expect("should detect a tempo from a clear beat");
    assert!((bpm - 90.0).abs() < 5.0, "expected ~90 bpm, got {bpm}");
}

#[test]
fn sustained_tone_has_no_detectable_bpm() {
    // A steady, unchanging tone has no attacks for the onset envelope
    // to lock onto - should report "no tempo," not a bogus number.
    let wav = sine_wav(440.0, 44_100, 8.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");
    assert!(levels.bpm.is_none());
}

#[test]
fn too_short_for_tempo_still_reports_band_levels() {
    // Under MIN_SECONDS_FOR_TEMPO but still long enough for the (much
    // shorter) FFT-band analysis - bpm should be None, not an error.
    let wav = sine_wav(440.0, 44_100, 1.0);
    let levels = analyze_band_levels(wav).expect("should decode and analyze");
    assert!(levels.bpm.is_none());
}

#[test]
fn too_short_audio_is_rejected() {
    let wav = sine_wav(440.0, 44_100, 0.01); // way under one FFT window
    let err = analyze_band_levels(wav).unwrap_err();
    assert!(err.contains("too short"));
}


/// Stereo 16-bit WAV: `left`/`right` are peak amplitudes (0..1) of the same sine on each channel.
fn stereo_sine_wav(freq: f32, sample_rate: u32, duration_secs: f32, left: f32, right: f32) -> Vec<u8> {
    let n = (sample_rate as f32 * duration_secs) as u32;
    let data_len = n * 4;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        let s = (2.0 * std::f32::consts::PI * freq * i as f32 / sample_rate as f32).sin();
        wav.extend_from_slice(&((s * left * i16::MAX as f32) as i16).to_le_bytes());
        wav.extend_from_slice(&((s * right * i16::MAX as f32) as i16).to_le_bytes());
    }
    wav
}

#[test]
fn loudness_of_a_stereo_sine_matches_bs1770() {
    // A 1 kHz sine of peak A on both channels: K-weighting is about +0.7 dB there, cancelling the
    // -0.691 offset, so each channel reads its mean-square level 20log10(A) - 3.01 dB, and the two
    // channels' powers add (+3.01 dB): the integrated loudness is 20log10(A), here -20 LUFS.
    let levels = analyze_band_levels(stereo_sine_wav(1000.0, 48_000, 6.0, 0.1, 0.1)).unwrap();
    let lufs = levels.loudness_lufs.expect("a loud enough file has a loudness");
    assert!((lufs + 20.0).abs() < 0.2, "measured {lufs} LUFS, expected about -20");
    let peak = levels.true_peak_db.expect("true peak");
    assert!((peak + 20.0).abs() < 0.3, "measured {peak} dBTP, expected about -20");
}

#[test]
fn loudness_counts_both_channels() {
    // The same tone in one channel only is 3 dB quieter than in both.
    let both = analyze_band_levels(stereo_sine_wav(1000.0, 48_000, 6.0, 0.1, 0.1)).unwrap().loudness_lufs.unwrap();
    let one = analyze_band_levels(stereo_sine_wav(1000.0, 48_000, 6.0, 0.1, 0.0)).unwrap().loudness_lufs.unwrap();
    assert!((both - one - 3.0).abs() < 0.2, "both {both}, one {one}");
}

#[test]
fn a_mono_file_reads_like_one_channel() {
    let lufs = analyze_band_levels(sine_wav(1000.0, 48_000, 6.0)).unwrap().loudness_lufs.unwrap();
    // sine_wav peaks at 0.8: 20log10(0.8) - 3.01 = -4.98 LUFS for one channel.
    assert!((lufs + 4.98).abs() < 0.3, "{lufs}");
}

#[test]
fn a_louder_track_reads_louder_and_gets_a_smaller_gain() {
    let quiet = analyze_band_levels(stereo_sine_wav(1000.0, 48_000, 6.0, 0.05, 0.05)).unwrap();
    let loud = analyze_band_levels(stereo_sine_wav(1000.0, 48_000, 6.0, 0.5, 0.5)).unwrap();
    assert!((loud.loudness_lufs.unwrap() - quiet.loudness_lufs.unwrap() - 20.0).abs() < 0.3);
    let gq = normalisation_gain_db(quiet.loudness_lufs.unwrap(), quiet.true_peak_db);
    let gl = normalisation_gain_db(loud.loudness_lufs.unwrap(), loud.true_peak_db);
    assert!(gq > gl, "quiet {gq} dB, loud {gl} dB");
}

/// A stereo WAV whose right channel is the left one inverted.
fn out_of_phase_wav(freq: f32, sample_rate: u32, duration_secs: f32) -> Vec<u8> {
    stereo_sine_wav(freq, sample_rate, duration_secs, 0.5, -0.5)
}

#[test]
fn out_of_phase_stereo_is_measured_not_cancelled() {
    // Averaging the two channels' samples would give silence; averaging their power gives the tone.
    let wav = {
        // `stereo_sine_wav` takes amplitudes, so a negative one inverts the channel.
        out_of_phase_wav(100.0, 48_000, 6.0)
    };
    let levels = analyze_band_levels(wav).unwrap();
    let band_100 = spectrum::FINE_BAND_CENTERS_HZ.iter().position(|&hz| hz == 100.0).unwrap();
    let level = levels.spectrum_db[band_100].expect("100 Hz band measured");
    assert!(level > -60.0, "an out-of-phase tone read as {level} dB, as if cancelled");
}

#[test]
fn a_file_that_stops_at_11_khz_is_found_to_be_low_passed() {
    let wav = multi_tone_wav(&[200.0, 1000.0, 3000.0, 6000.0, 8000.0, 10_000.0], 48_000, 6.0);
    let levels = analyze_band_levels(wav).unwrap();
    let cut = levels.lowpass_hz.expect("a cut-off above the last tone");
    assert!((10_500.0..11_800.0).contains(&cut), "cut-off {cut}");
    let last = levels.spectrum_db.len() - 1;
    assert!(levels.spectrum_db[last].is_none(), "the 16 kHz band is beyond the cut-off");
    let band_1k = spectrum::FINE_BAND_CENTERS_HZ.iter().position(|&hz| hz == 1000.0).unwrap();
    assert!(levels.spectrum_db[band_1k].is_some());
}

#[test]
fn a_full_bandwidth_file_has_no_cutoff() {
    let wav = multi_tone_wav(&[200.0, 1000.0, 3000.0, 6000.0, 8000.0, 10_000.0, 12_500.0, 16_000.0], 48_000, 6.0);
    let levels = analyze_band_levels(wav).unwrap();
    assert_eq!(levels.lowpass_hz, None);
    assert!(levels.spectrum_db.iter().rev().take(2).all(|b| b.is_some()));
}

#[test]
fn the_spectrum_and_version_are_saved_with_the_analysis() {
    let a = analyze_band_levels(sine_wav(1000.0, 48_000, 6.0))
        .unwrap()
        .into_analysis("t".into(), "T".into(), "A".into(), None);
    assert_eq!(a.analysis_version, crate::models::ANALYSIS_VERSION);
    assert_eq!(a.spectrum_db.len(), spectrum::FINE_BAND_CENTERS_HZ.len());
}

#[test]
fn analysing_a_file_gives_the_same_result_as_analysing_its_bytes() {
    // A 3-minute click track: long enough that the tempo excerpt is cut to the middle 90 s.
    let wav = click_track_wav(120.0, 44_100, 180.0);
    let path = std::env::temp_dir().join(format!("tanqi-analyze-file-{}.wav", std::process::id()));
    std::fs::write(&path, &wav).unwrap();
    let from_file = analyze_file(path.to_str().unwrap()).unwrap();
    let from_bytes = analyze_band_levels(wav).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(from_file.spectrum_db, from_bytes.spectrum_db);
    assert_eq!(from_file.loudness_lufs, from_bytes.loudness_lufs);
    assert_eq!(from_file.bpm, from_bytes.bpm);
    assert!(from_file.bpm.is_some(), "the click track has a tempo");
}

#[test]
fn a_missing_file_is_an_error_not_a_panic() {
    assert!(analyze_file("/definitely/not/here.flac").unwrap_err().contains("failed to read"));
}
