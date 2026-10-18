# Tempo (BPM) detection libraries for Tanqi: stratum-dsp vs bpm-analyzer vs aubio-rs

Status: research and a scratch trial, no project code changed (`src/` and `Cargo.toml` untouched). Trial code lives outside the repo in `/tmp/claude-1000/-home-syasha-Project-tanqi-music/b2350044-f570-40d8-928f-9aba83dea26e/scratchpad/` (`tempo_trial/`, `bpma_patched/`, `results.tsv`, `android.log`).

Evidence labels: **[opened]** = I read the page or file text; **[measured]** = I ran it on this machine (AMD Ryzen 5 8600G, 12 threads, 30 GB RAM, desktop Linux, single run per cell); **[inferred]** = my reasoning, not stated by a source. Sources are crates.io API/pages, the crates' GitHub repos, the crate tarballs from crates.io, and aubio's own repo/site. No blogs or forums.

## Summary table

Accuracy is on the five user-supplied references, using the middle 60 s of each track (what the app analyses, apart from its 90 s cap). Classes: close = within 2 BPM, near = within 5%, octave = about 2x or 0.5x, 3:2 = about 1.5x or 0.67x.

| | stratum-dsp 1.0.0 (current) | aubio-rs 0.2.0 (aubio 0.4.9 C) | bpm-analyzer 0.2.0 |
|---|---|---|---|
| Algorithm (per own docs/source) | FFT + autocorrelation tempogram, HMM beat grid, multi-resolution octave correction | spectral-flux onset function, autocorrelation + comb filterbank + Rayleigh tempo prior | Daubechies D4 wavelet bands, envelope, autocorrelation |
| Licence | MIT OR Apache-2.0 | **GPL-3.0** | MIT |
| Pure Rust? | Yes (optional `ml` feature pulls `ort`, off by default) | No: bundled C source compiled by `cc` | Rust, but hard dependency on `cpal` (live audio capture) |
| Builds on Android (`cargo check`, both ABIs) | Yes | **No by default** (no prebuilt bindings for `*-linux-android`); **Yes with `bindgen` feature** | Yes (cpal uses ndk/jni) |
| Offline file/buffer API | Yes (`analyze_audio(&[f32], sr, cfg)`) | Yes (streaming `Tempo::do_`) | **No** (microphone/loopback only; I had to patch it) |
| Aerials (ref 81) | 159.0, octave | 164.1 final / 162.1 median, octave | 104.4, no class |
| A Certain Romance (139) | 135.0, near | 141.1 / 141.1, near | 91.7, 3:2 |
| Phoenix (134) | 134.0, close | 136.2 / 136.1, near | 90.5, 3:2 |
| Schism (109) | 71.5, 3:2 | 108.2 / 109.1, close | 218.2, octave |
| Feel Good Inc. (139) | 138.8, close | 140.6 / 140.6, close | 92.3, 3:2 |
| Within 5% of reference | 3 of 5 | 4 of 5 | 0 of 5 |
| Wall time, 60 s excerpt | 0.52-1.33 s (about 0.74 s on 6 tracks, 1.2-1.3 s on 8, 0.52 s on a 42 s clip) | 0.05-0.08 s | 0.22-0.31 s (my 1 s-hop offline adaptation) |
| Peak RSS, 60 s excerpt | 296 MB | 12 MB | 19 MB |

RSS includes the 10.6 MB input vector in every case. "Final / median" for aubio: its running estimate at the end of the excerpt, and the median of its estimate over the second half of the excerpt (my choice of summary, see section 2).

## 1. Primary-source facts

### 1.1 stratum-dsp

- **Algorithm.** "Dual tempogram approach (FFT + autocorrelation) based on Grosche et al. (2012)", "Multi-resolution escalation for metrical-level disambiguation", "HMM-based beat tracking with tempo drift correction", plus a key detector (chroma + Krumhansl-Kessler templates). [opened] README Features, https://github.com/HLLMR/stratum-dsp/blob/main/README.md
- **Licence.** MIT OR Apache-2.0. [opened] crates.io API `https://crates.io/api/v1/crates/stratum-dsp` (version 1.0.0 `license`), README "License" section, repo has `LICENSE-MIT` and `LICENSE-APACHE`. (GitHub's licence detector shows "Other/NOASSERTION" because of the dual files, not a different licence.)
- **Version / date.** Only one release exists: 1.0.0, 2025-12-19. [opened] crates.io API versions list.
- **Last commit.** 2025-12-19 ("Fix docs.rs build issues"). [opened] GitHub API `repos/HLLMR/stratum-dsp/commits`. Maintained? One release, one day of activity visible, one open issue (#1 "BPM slightly off", asks why 140 BPM reads 139.85; no reply). Whether it is actively maintained is **not documented**; the commit history shows no activity after release.
- **Downloads.** 27,569 total, 5,432 recent (crates.io API `downloads`, `recent_downloads`; "recent" is crates.io's 90-day figure).
- **Dependencies.** Normal: `log`, `rayon`, `rustfft`, `serde`, `serde_json`, `symphonia ^0.5`; optional `ort` behind feature `ml` (default features are empty). README: "pure Rust, zero FFI dependencies". No C library, no ffmpeg, no system packages. [opened] crates.io `.../1.0.0/dependencies`, crate `Cargo.toml` `[features]`.
- **Input.** Whole buffer: mono f32 normalised to [-1, 1] plus sample rate. README: "Fixed sample rate: Optimized for 44.1 kHz (other rates work but may have slight accuracy differences)". Minimum length: not documented (Tanqi enforces 5 s itself). [opened] `analyze_audio` doc comment in `src/lib.rs`, README Known Limitations.
- **Output.** BPM, BPM confidence, key (+ key confidence), beat grid, grid stability. [opened] README usage.
- **Documented accuracy and limits.** 87.7% within +-2 BPM (136/155 tracks), 88.4% within +-5, on 155 Beatport/ZipDJ DJ tracks; the same README says its FMA Small development set gives 56.7% within +-2. "~12% of tracks show octave/harmonic confusions (1/2x, 2x, 2/3x ratios)", common on "double-time feels, triplet-based tracks, or tracks with weak kick-snare patterns". [opened] README "Performance" and "Known Limitations".

### 1.2 bpm-analyzer

- **Algorithm.** "using wavelet decomposition and autocorrelation analysis", based on Tzanetakis, Essl, Cook, "Audio Analysis using the Discrete Wavelet Transform". Pipeline: capture from an audio device, resample to 22.05 kHz, 4-level Daubechies D4 DWT, amplitude envelope per band (full-wave rectification + low-pass), autocorrelation of the summed envelopes, peak picking. [opened] README "How It Works", https://github.com/not-jan/bpm-analyzer/blob/main/README.md; source `src/analyzer.rs`, `src/config.rs`.
- **Licence.** MIT. [opened] crates.io API, `Cargo.toml`, GitHub licence field.
- **Version / date.** 0.2.0, 2026-01-20 (0.1.0 the same day). [opened] crates.io API.
- **Last commit.** 2026-01-20 ("Add beat picking"). One open issue: #1 "detect from audio file", whose text says there "currently no way" to detect from a file (https://github.com/not-jan/bpm-analyzer/issues/1). Maintained? 7 stars, two releases in one day, nothing since; not documented beyond that.
- **Downloads.** 511 total, 276 recent. [opened] crates.io API.
- **Dependencies.** `cpal ^0.17.1` (mandatory), `fundsp`, `osclet`, `resampler`, `ringbuffer`, `crossbeam-channel`, `bon`, `itertools`, `thiserror`, `tracing`; GUI deps optional under feature `bin`. No C code of its own. On Linux `cpal` pulls `alsa-sys`, which needs the ALSA development package: [measured] on this machine an unmodified `cargo build` of `bpm-analyzer` fails with
  `The system library 'alsa' required by crate 'alsa-sys' was not found. The file 'alsa.pc' needs to be installed ...` (alsa-sys-0.4.0 build.rs:13). Tanqi's Cargo.toml has no `cpal`/`alsa` today, so this would be a new build requirement (`libasound2-dev` or equivalent). README Audio Setup only describes macOS BlackHole loopback.
- **Input.** Live audio only. The public API is `begin(config)` / `begin_with_device(config, &device)`, which open a `cpal` input stream and return a channel of `BpmDetection`. There is no function that takes a buffer or file. [opened] `src/lib.rs`, `src/analyzer.rs`. Sample rates accepted by the internal resampler: 16000, 22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000 Hz, else `UnsupportedSampleRate`. Mono-mixed internally. Default window 65,536 samples at 22.05 kHz (about 3 s), default range 40-240 BPM.
- **Output.** Up to 5 BPM candidates with an autocorrelation-value "confidence", plus the last 8 beat timestamps with strength, once per window. No key. [opened] `src/types.rs`.
- **Documented limits.** README "Limitations": tempo changes, polyrhythmic music, accuracy may drop outside 40-240 BPM. No accuracy numbers: **not documented**.
- **A code observation [inferred from reading `analyzer.rs`].** The code sorts autocorrelation lags by value and then reports `peaks_buffer[1..=peak_count]`, i.e. it skips the strongest lag (the comment says it is "usually the zero-lag", but lags below `min_lag` are already excluded). In my trial the effect was small because the second-strongest lag is usually the neighbour of the strongest (see 2.4).

### 1.3 aubio-rs (and aubio)

- **Algorithm.** aubio's `tempo` object: default method is the "specflux" spectral-flux onset function (`tempo.c`: `strcmp(tempo_mode, "default") == 0 -> "specflux"`), peak-picked into a detection function over a window "worth about 6 seconds", then `beattracking.c` (authors "Matthew Davies and Paul Brossier") which computes an autocorrelation of the detection function, passes it through a "shift invariant comb filterbank", applies a "Rayleigh weight" (comment: "sets preferred tempo to 120bpm"), and a second context-dependent Gaussian model. [opened] https://github.com/aubio/aubio/blob/master/src/tempo/tempo.c and `beattracking.c`. The tempo.h doc comment names no papers: "Tempo detection object ... returns the estimated beat locations" (https://aubio.org/doc/latest/tempo_8h.html). Papers cited by the code: **not documented** in the files I read.
- **Licence.** aubio: GPL-3.0 (`COPYING` is the GPL v3 text; source headers say "either version 3 of the License, or (at your option) any later version"). aubio.org: "aubio is free and open source software under the GNU/GPL license ... aubio is not MIT or BSD licensed." `aubio-rs` and `aubio-sys` crates: `license = "GPL-3.0"` in crates.io metadata. [opened] https://aubio.org/ , https://github.com/aubio/aubio/blob/master/COPYING, crates.io API.
  - **What the licence text says (not legal advice).** GPL-3.0 section 5(c): "You must license the entire work, as a whole, under this License to anyone who comes into possession of a copy." Section 6 requires offering the Corresponding Source when conveying a work in object form. Read literally, a Tanqi binary that is distributed with aubio compiled in (the bundled-source build links it statically) would be a work conveyed under GPL-3.0 as a whole, with Tanqi's source obligated to be available under GPL-compatible terms; MIT-licensed files can be included in a GPL work, but the combined binary cannot be offered as MIT-only. Whether linking creates a "work based on the Program" is a legal question I did not resolve. Distribution through an app store with extra terms is a separate compatibility question I did not check.
- **Version / date.** aubio-rs 0.2.0, 2021-04-30 (crates.io). aubio-sys 0.2.1, 2021-05-22 (aubio-rs 0.2.0 depends on `aubio-sys ^0.2.0`, which resolves to 0.2.1). The bundled C library is aubio 0.4.9 (`LIB_VER = "0.4.9"` in `sys/build.rs`); aubio 0.4.9 was released 2019-02-27 (GitHub releases). aubio master's `VERSION` file says 0.5.0-alpha.
- **Last commit.** aubio-rs: 2023-01-21 ("Update CI"). aubio: 2026-04-10 (a Python-binding fix), 160 open issues. Maintained? aubio-rs: no release since 2021, no commit since 2023, open issues include #29 "Unsoundness: Unbound lifetime enables use-after-free on `CVec`" (2026-03-16) and #27 "Failing to build a project that depends on aubio-rs". aubio: last release 2019, small ongoing commit activity. [opened] GitHub API.
- **Downloads.** aubio-rs 23,021 total / 5,654 recent; aubio-sys 34,295 / 7,162. [opened] crates.io API.
- **Dependencies.** aubio-rs -> aubio-sys; aubio-sys build-depends on `cc` (always), optionally `bindgen 0.58` and `pkg-config`, optionally `fftw-sys`. **Default (no `pkg-config` feature) compiles aubio's C source, which is shipped inside the `aubio-sys` crate (`aubio/` directory, 5.8 MB package), with the C compiler**; with feature `pkg-config` it uses an installed `libaubio >= 0.4.9`. No ffmpeg or other system packages needed in the default path. [opened] `sys/build.rs` (`try_find_and_use_library`), `sys/Cargo.toml`; [measured] `cargo build --release` succeeded on this Linux machine with only gcc.
- **Input.** Streaming: `Tempo::new(OnsetMode, buf_size, hop_size, sample_rate)` then `do_(hop-sized f32 slice, ...)` repeatedly; mono f32. Sample-rate limits: the constructor only rejects rate < 1; upstream issue #284 "Wrong tempo with sample rates < 44.1kHz" is open. [opened] `aubio-rs/src/tempo.rs`, `tempo.c`, https://github.com/aubio/aubio/issues/284
- **Output.** Per hop: a beat flag; accessors `get_bpm`, `get_confidence`, `get_last_s`, `get_period_s`. Doc: bpm and confidence are `0` when "no consistent value is found". No key. [opened] https://aubio.org/doc/latest/tempo_8h.html
- **Documented accuracy / limits.** No accuracy figures in the docs. Open upstream issues: #406 "BPM accuracy looks suspect" (2024), #366 "demo_bpm_extract.py consistently oversamples bpm by +2" (2022), #227 "Tempo tracking & accuracy" (2019), #391 "Variable bpm". The ChangeLog for 0.4.9 lists a fixed buffer overflow in `new_aubio_tempo` (CVE-2018-19800). [opened] https://github.com/aubio/aubio/issues/406 and `#366`, `ChangeLog`.

## 2. Hands-on trial

### 2.1 Method

- Each track decoded once with ffmpeg to mono 44.1 kHz f32 (`-ac 1 -ar 44100 -f f32le`), the middle 60 s cut with the same rule as `excerpt()` in `src/audio/tempo.rs` (`start = (len - max) / 2`), then each library run as its own release process under `/usr/bin/time -v` (peak RSS = "Maximum resident set size"; time = the library call only, measured in-process). Full-track runs were also done.
- **stratum-dsp:** exactly `analyze_audio(samples, 44100, AnalysisConfig::default())`, `default-features = false`. The 60 s numbers reproduce the figures the user already had (159, 135, 134.0, 71.5, 138.8). The loudness pre-check from `tempo.rs` was not applied (it only gates silence/tones).
- **aubio-rs:** `Tempo::new(OnsetMode::SpecFlux, 1024, 512, 44100)`, hop 512, fed the whole excerpt. Window and hop are my choice (aubio's own command-line tools use 1024/512, not re-verified). aubio is a running estimator, so I report `get_bpm()` at the end ("final") and the median of `get_bpm()` over the second half of the run ("median"). Which to use in production is a design decision the library does not make for you.
- **bpm-analyzer:** the published crate cannot read a buffer and cannot build here without ALSA headers. I made a copy (`bpma_patched/`) with `cpal` and the device code removed and a new `analyze_offline` that feeds mono chunks through the **unchanged** `run_analysis` loop (only change: the audio channel carries `Vec<f32>` and the loop blocks on it). It is fed 1 s hops, giving one detection per second after the first 3 s window fills; the table reports the median of the top candidate across windows (`bpm`) and the 1-BPM histogram mode. A second variant (`BPMA_FIX=1`) reports the strongest lag instead of the second strongest (see 1.2). This is an adaptation of a live-capture library, so the results measure the algorithm as I could drive it, not a supported mode. **[my adaptation, not the vendor's usage]**.

### 2.2 Five reference tracks (middle 60 s)

| Track | Ref | stratum-dsp | aubio final / median | bpm-analyzer median / mode | bpm-analyzer strongest-lag variant |
|---|---|---|---|---|---|
| Aerials | 81 | 158.98 (octave) | 164.10 / 162.09 (octave) | 104.40 / 107 | 104.40 / 106 |
| A Certain Romance | 139 | 135.00 (near, -2.9%) | 141.10 / 141.06 (near, +1.5%) | 91.67 / 68 (3:2) | 92.60 / 68 |
| Phoenix | 134 | 134.00 (close) | 136.18 / 136.12 (near, +1.6%) | 90.47 / 89 (3:2) | 91.37 / 89 |
| Schism | 109 | 71.50 (3:2) | 108.21 / 109.10 (close) | 218.17 / 220 (octave) | 213.66 / 236 |
| Feel Good Inc. | 139 | 138.79 (close) | 140.55 / 140.56 (close) | 92.29 / 92 (3:2) | 92.39 / 92 |

Observations:
- aubio and stratum both read Aerials at about 2x the reference. Both are plausible readings of the same pulse; which one counts as "the BPM" is the user's convention, so this is an agreed octave error, not a library difference.
- Schism (odd metre) is the one that separates them: aubio close, stratum 3:2 off, bpm-analyzer octave off.
- aubio sat 1.5-2.1 BPM above the reference on three tracks (141.1, 136.2, 140.6) [measured]. Its estimate is quantised by its frame period; I did not verify the cause.
- Full-track runs (not what the app does) changed stratum's answers on 2 of the 5: Feel Good Inc. 92.5 (worse, 3:2) and Schism 105.5 (better, near); A Certain Romance moved to 137.7. So whole-track input is not simply better or worse for accuracy, and it costs 4-7x the memory. For aubio the final value on the full track was unstable (Aerials 172, A Certain Romance 179.2) while its second-half median stayed near the 60 s values (161.6, 141.6, 136.3, 109.6, 140.6).

### 2.3 Ten more tracks, mid 60 s (no reference; agreement only)

| Track (artist) | stratum | aubio final / median | bpm-analyzer median / mode |
|---|---|---|---|
| Thundersnail (Undertale; clip is 42 s, used whole) | 149.72 | 152.63 / 152.47 | 75.03 / 75 |
| Kitai (Death Note OST) | 101.88 | 103.26 / 103.31 | 101.46 / 101 |
| Atmospheric Ignition (Oppenheimer OST) | 119.86 | 126.15 / 124.19 | 178.59 / 183 |
| Telemachus (The Odyssey) | 159.82 | 123.72 / 122.88 | 210.94 / 234 |
| Hijab Kekasih (M Nasir) | 99.68 | 100.90 / 101.35 | 185.81 / 206 |
| The Bad Thing (Arctic Monkeys) | 111.00 | 112.92 / 112.77 | 110.99 / 111 |
| Dare (Gorillaz) | 119.86 | 122.14 / 122.10 | 210.40 / 221 |
| Kill Rock 'n Roll (System Of A Down) | 90.00 | 93.65 / 94.49 | 181.33 / 186 |
| BOOM (NMIXX) | 110.00 | 111.61 / 111.53 | 219.91 / 223 |
| Kereta Malam (Siti Nurhaliza, mp3) | 105.00 | 106.74 / 106.60 | 52.90 / 53 |

- stratum and aubio agree within about 5% on 9 of 10 (the exception is Telemachus, 159.8 vs 122.9, ratio 1.3, not a simple octave). aubio reads 1-4% above stratum on the agreeing tracks, consistent with its offset on the references.
- bpm-analyzer matches stratum on 2 of 10 (Kitai, The Bad Thing); on 6 others it is near 2x or 0.5x of the others (75 vs 150, 52.9 vs 105, 220 vs 110, 181 vs 90, 210 vs 120, 186 vs 100). Without references I cannot say which of the three is "right" on these ten; I only report agreement.

### 2.4 Time and memory

| | stratum-dsp | aubio-rs | bpm-analyzer (adaptation) |
|---|---|---|---|
| 60 s excerpt, wall | 0.52 s (42 s clip), 0.73-0.76 s on 6 tracks, 1.22-1.33 s on 8 | 0.05-0.08 s | 0.22-0.31 s |
| 60 s excerpt, peak RSS | 295-296 MB (207 MB for the 42 s clip) | 12 MB (9 MB for 42 s) | 18-19 MB (16 MB) |
| Full track (142-408 s), wall | 0.5-8.1 s | 0.05-0.57 s | 0.22-2.1 s |
| Full track, peak RSS | 207 MB-2.0 GB | 9-71 MB | 16-77 MB |

stratum's memory grows roughly with the audio it is given (about 296 MB at 60 s, 1.1-2.0 GB for 4-7 minute tracks), matching the comment in `tempo.rs` about phones running out of memory. aubio's grows only with the input vector. Desktop numbers only: Android speed and memory were not measured. The bpm-analyzer time is dominated by my 1 s-hop driver (58 windows, each with an O(n^2) autocorrelation); it is not a measure of its live cost.

## 3. Android feasibility

Setup: `ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/26.3.11579264`, `CC_<target>` / `CARGO_TARGET_<TARGET>_LINKER` set to `aarch64-linux-android24-clang` and `x86_64-linux-android24-clang`, `cargo check --target ...` in a bare crate depending only on the library, run under bash. `cargo check` runs build scripts (so the C is compiled) but does **not link**; linking and running on a device were not tested. Log: `scratchpad/android.log`.

| Library | aarch64-linux-android | x86_64-linux-android |
|---|---|---|
| stratum-dsp 1.0 (`default-features = false`) | OK | OK |
| bpm-analyzer 0.2.0 unmodified | OK (cpal uses `ndk`, `ndk-sys`, `jni`; no C++ build seen) | OK |
| bpm-analyzer, my patched copy without cpal | OK | OK |
| aubio-rs 0.2.0 default features | **FAIL** | **FAIL** |
| aubio-rs 0.2.0 + `bindgen` feature | OK (C compiled by the NDK clang; `libaubio.a` is an ELF aarch64 object) | OK |

Exact aubio failure (both targets): `aubio-sys-0.2.1/build.rs:34:13: No prebuilt bindings. Try use 'bindgen' feature.` The crate ships prebuilt bindings only for `*-linux-gnu`, macOS and Windows targets (`aubio-sys/src/bindings`, listing from the repo), none for Android. With `features = ["bindgen"]` it generates them at build time with `bindgen 0.58`, which here found a system libclang; so the Android build needs libclang on the build machine, plus the NDK clang for the C code. aubio-rs gets the aubio C library by compiling the source bundled inside the `aubio-sys` crate (default); `pkg-config` would use a system libaubio, which does not apply to Android.

Not verified: the real link step, runtime behaviour on a device, whether `bindgen 0.58` keeps working with newer libclang versions in the user's CI, and whether `cpal` (bpm-analyzer) would be acceptable in an app that does not capture audio.

## 4. Recommendation

**Keep stratum-dsp as the tempo engine. Do not switch to bpm-analyzer. Treat aubio-rs as a technically attractive but licence-gated option.**

Reasons:

1. **bpm-analyzer is not a fit.** It has no offline API (its own open issue #1 asks for one), forces `cpal` (and ALSA headers on Linux), has 511 downloads and no activity after its two releases on 2026-01-20, and in my adapted trial landed within 5% of the reference on 0 of 5 tracks (mostly 3:2 or octave errors) and agreed with the others on 2 of 10. The adaptation may be a poor way to drive a live-capture library, but there is no better way in its public API.
2. **aubio is the most accurate here and far lighter, but GPL-3.0.** On the five references it was within 5% on 4 (stratum 3), it was about 10-18x faster (about 0.07 s vs 0.7-1.3 s) and used about 12 MB instead of about 296 MB, and it agreed with stratum within 5% on 9 of 10 other tracks. It does not give key. Costs: (a) GPL-3.0 means, per section 5(c) of the licence text, that a distributed Tanqi binary containing it would have to be licensed as a whole under GPL-3.0, so it conflicts with an MIT-only release (a licence decision, not an engineering one); (b) Android needs the `bindgen` feature, libclang and C compilation in the Android build (effort: moderate, untested at link/run time); (c) aubio-rs is unmaintained since 2021 with an open unsoundness report (#29) on its `CVec` type (my wrapper code would use only `Tempo`; I did not check whether `Tempo` is affected); (d) the running estimate wobbles, so the app would need a rule such as the median over the second half; (e) a systematic +1.5-2 BPM offset on three references.
3. **stratum-dsp is the safe baseline but only 3/5 on these five.** It is pure Rust, MIT OR Apache-2.0, already builds on Android, returns key as well (Tanqi uses it), and its own README documents about 12% metrical-level errors, which matches what the trial shows (Aerials octave, Schism 3:2). Cost: roughly 0.7-1.3 s and about 300 MB per 60 s excerpt (more for a 90 s excerpt, not measured here), and only one release from a young project.

If the licence is acceptable, the lowest-effort accuracy gain is likely a hybrid: keep stratum for key and beat grid, and use aubio only to sanity-check the tempo (for example, prefer aubio when the two differ by a non-octave ratio). That is my inference from five references and ten unlabelled tracks, not a measured result; I did not build it. If the licence is not acceptable, the remaining non-code lever for stratum is the metrical-level choice (octave folding to a preferred range), which I did not test.

What I could not verify: the licence's application to Tanqi (legal question); behaviour on a real Android device (only `cargo check`); the stratum crate's own description of why Schism comes out at 71.5; whether a different aubio window/hop (I used 1024/512 only), onset mode, or `silence`/`threshold` settings change the accuracy; any accuracy figure for aubio or bpm-analyzer from their authors (none is documented); a larger or labelled test set (five references and ten tracks is too few to rank the libraries beyond "bpm-analyzer is clearly worse here"). All timings are single runs on a 12-thread desktop CPU.
