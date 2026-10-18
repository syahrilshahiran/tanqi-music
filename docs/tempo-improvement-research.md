# Improving tempo (BPM) detection in Tanqi: research notes

Status: research and a scratch harness, no project code changed (`src/` and `Cargo.toml` untouched). Harness: `/tmp/claude-1000/-home-syasha-Project-tanqi-music/b2350044-f570-40d8-928f-9aba83dea26e/scratchpad/tprobe/` (`src/main.rs`; raw outputs `tprobe_out.txt`, `ranges_out.txt`). It reuses the earlier trial's decoded tracks (`raw/*.raw`, mono 44.1 kHz f32, made by ffmpeg from `~/Music`, which was only read). Follows on from [tempo-library-comparison.md](tempo-library-comparison.md).

Evidence labels: **[opened]** I read the page or PDF text. **[abstract]** only an abstract or search snippet was readable. **[secondary]** not a primary source; the claim is unverified and not used for any conclusion. **[measured]** I ran it here (AMD Ryzen 5 8600G, desktop Linux, release build, stratum-dsp 1.0.0, single runs). **[inferred]** my reasoning, not stated by a source.

## Summary

**What the five references showed [measured].** The current detector (stratum-dsp, `AnalysisConfig::default()`, middle 60 s) makes three kinds of error here, and they have different causes:

| Track | Ref | Now (60 s / 90 s) | Error | Is the right answer among stratum's own candidates? |
|---|---|---|---|---|
| Aerials | 81 | 159.0 / 159.0 | octave (2x) | Yes: 79.8 / 78.9 are candidates #2 and #4, but score 0.80 vs 1.17 for 159 |
| A Certain Romance | 139 | 135.0 / 135.0 | near (-2.9%) | 138.4 and 137.5 are candidates #2 and #4; 135.0 narrowly wins (score 1.00 vs 0.97) |
| Phoenix | 134 | 134.0 / 134.0 | none | n/a |
| Schism | 109 | 71.5 / 72.0 | 3:2 (x2/3) | Yes: 107.2, 106.3, 108.0 are candidates, but the top-scored candidate is 217 (about 2x of 109) and multi-resolution escalation then picked 71.5 |
| Feel Good Inc. | 139 | 138.8 / 138.8 | none | n/a |

- **Octave (Aerials) is a convention question as much as an error.** Both readings are real pulses. A tempo prior or a half/double control addresses it; no detector can know which level the user means.
- **3:2 (Schism) is a different failure.** A prior toward 120 cannot fix it (71.5 and 107 are both inside any sensible range) and the standard ACC2 metric does not even count 3:2 as acceptable (section 1).
- **stratum's own confidence does not separate right from wrong.** `bpm_confidence` was 0.31 (Aerials), 0.04 (A Certain Romance), 0.14 (Phoenix), **0.65 (Schism, wrong)**, 0.04 (Feel Good, right). Do not gate on it.
- **The beat grid is no use as a tempo check.** The median beat interval of `beat_grid.beats` implies 175, 152, 160, 39 and 163 BPM for the five. Treat the grid as unreliable.
- **Voting over several shorter excerpts made things worse, not better** (section 3.4).
- **Plain candidate-range restriction (`min_bpm`/`max_bpm` 70-150) fixes both Aerials (78.9, near) and Schism (107.0, close), and leaves the other three unchanged: 5/5 within 5%.** But it is tuned to these five (the edge at 150 sits right beside the 139-140 tracks), and on ten unlabelled tracks it changed two answers in ways that look like regressions (section 3.3). It is not safe to ship on five references.
- **An independent, textbook onset-autocorrelation estimator (Ellis 2007, about 80 lines, 48 ms for 60 s, no extra dependency beyond `rustfft`, which stratum already uses) got Schism right (107.7) and four of five within 5% with its published parameters**, but also reproduced the Aerials octave and disagreed with stratum on 3 of 10 other tracks (section 3.5).

**Recommended plan, cheapest and safest first** (details in section 4):

1. **Build the measuring stick first:** `docs/tempo-references.tsv` plus a probe that scores ACC1, ACC2, a 3:2-aware error class and the OE1 log2 ratio, plus synthetic click tracks. No behaviour change. Without it nothing below can be trusted.
2. **Give the user control, no algorithm change:** a per-track manual BPM override and half/double buttons (and tap tempo). This is the only remedy for the octave convention problem, it costs no CPU, and the MIREX task itself treats two tempi per excerpt as the ground truth for ambiguous music.
3. **Add a post-hoc octave fold into about [75, 150) on stratum's output** (not a change to stratum's search range). It fixes Aerials (159 to 79.5) without altering candidate generation, so it cannot cause the hijab-type regression. The window edge is the tuned part; choose it from the labelled set once it has about 100 tracks.
4. **Run the Ellis-style autocorrelation as a second opinion in shadow mode** (log, do not override) and use it only to flag disagreement; this flagged Schism (the 3:2 case) on the references. Promote it to an override only if the labelled set shows it wins.
5. **Do not:** use GPL aubio, ship madmom or TempoCNN models, vote across short excerpts, trust `bpm_confidence`, or retune stratum's internal weights on five tracks.

## 0. How Tanqi detects tempo today (read from the code)

`src/audio/tempo.rs`, `detect(samples, sample_rate, max_seconds)`:

1. Reject audio shorter than 5 s (`MIN_SECONDS_FOR_TEMPO`).
2. Take the middle `max_seconds` of the track (`excerpt()`, start = (len - max)/2). The callers in `src/audio/mod.rs` pass `MAX_SECONDS_FOR_TEMPO = 90.0` (lines 126 and 132), a cap added because stratum's memory grows with its input and a phone ran out of memory.
3. Gate on loudness variation (`has_rhythmic_energy_variation`, coefficient of variation of 256-sample RMS must be at least 0.15).
4. `stratum_dsp::analyze_audio(samples, sample_rate, AnalysisConfig::default())`; take `result.bpm` if positive. The same call also yields the key (kept if `key_confidence >= 0.3`).
5. Nothing else: no prior, no octave handling, no second opinion, no stored confidence, and no way for the user to correct a wrong value.

The earlier comparison measured the five references on the middle 60 s; I also ran 90 s here because that is what the app does (table above: identical answers, except Schism 72.0 instead of 71.5).

## 1. What the literature and evaluation practice say about octave and 3:2 errors

### 1.1 Accuracy definitions

- **ACC1 / ACC2 (ISMIR 2004 contest, Gouyon et al.).** The contest's evaluation page: "Accuracy 1: The percentage of correct tempi, in BPM, within 4% accuracy." "Accuracy 2" is the same with "half, double, three times and 1/3 of ground-truth tempo as correct." [opened] https://mtg.upf.edu/ismir2004/contest/tempoContest/node7.html (the contest page, not the TASLP paper). The TASLP paper (Gouyon, Klapuri, Dixon, et al., IEEE TASLP 14(5):1832-1844, 2006, DOI 10.1109/TSA.2005.858509) reports 12 entries, 3,199 test instances, "No training data was provided", and that algorithms "can reach over 80% accuracy for music with a constant tempo, if we do not insist on finding a specific metrical level" [abstract] https://mtg.upf.edu/node/415. I did not open the paper's full text. It also proposes a "redundant" approach "which combines multiple simpler methods using a voting mechanism" (same abstract) - this is the literature behind candidate (e).
- **Consequence [inferred from the definitions].** ACC2's allowed factors are 2, 1/2, 3, 1/3. A 3:2 or 2:3 error, like Schism's 71.5 vs 109, is not covered by ACC2 at all. It counts as wrong under both metrics, so the standard "octave-tolerant" score understates how bad it is. My harness reports 3:2 and 2:3 as their own classes for this reason.
- **Critique of the metrics.** Schreiber, Urbano, Müller, "Music Tempo Estimation: Are We Done Yet?", TISMIR 3(1), 2020: ACC2 "counts some perceptually erroneous estimates as correct", is not tied to application needs, and they propose a formal octave error OE1 = log2(estimate / reference) so that errors by 2, 1/2, 3, 1/3 appear as clusters at +1, -1, +1.58, -1.58 [opened] https://www.audiolabs-erlangen.com/content/05_fau/professor/00_mueller/03_publications/2020_SchreiberUM_MusicTempo_TISMIR_ePrint.pdf (section 3 and section 5 "Formal Octave Error"). The same paper's survey found that nobody chose the 8% MIREX tolerance as their preferred one. Their released evaluation code implements ACC1, ACC2 and P-Score [opened, same PDF, section 7 / code note]. I use OE1 in the probe.

### 1.2 MIREX audio tempo estimation

The task description defines the ground truth as two tempi (T1 slower, T2 faster) with a relative salience ST1, taken from the two highest peaks of listeners' tapping distributions, and scores P = ST1 * TT1 + (1 - ST1) * TT2, with tolerance 4% (changed from 8% in 2018); "no credit will be given for tempi other than T1 and T2" [opened] https://music-ir.org/mirex/wiki/2018:Audio_Tempo_Estimation (the `www.` host fails its certificate; I read the same page without `www.`). It also says "The dominant perceived tempo is sometimes the same as the notated tempo but not always", i.e. one level higher or lower. The GiantSteps annotations from this lineage "must not be used for training" (same page).

### 1.3 Tempo perception and ambiguity (Moelants and McKinney)

- Ellis cites McKinney and Moelants for "a bias towards 120 BPM" in human tempo perception, and describes their tapping data (MIREX-06 beat-tracking training set) as clusterable "into two groups ... separated by a ratio of 2 or 3" in every one of the 20 training excerpts [opened] https://www.ee.columbia.edu/~dpwe/pubs/Ellis07-beattrack.pdf, section 3.2. That is a primary statement about the data, via Ellis.
- McKinney and Moelants' own abstract (conference version) says most excerpts show at least two perceived beat periods with varying strength, that for conventional styles the dominant period usually matches a resonance model, but that for some excerpts "very slow or very fast periods" dominate and the distribution of dynamic accents appears to be a key factor [abstract; reached only as a search snippet of https://cspeech.ucd.ie/rppw/rppw10/mckinney/abstract.html, the page itself refused my connection]. The journal version is McKinney and Moelants, "Ambiguity in tempo perception: What draws listeners to different metrical levels?", Music Perception 24(2), 2006 [abstract, via search snippet only; not opened].
- I could not reach Moelants' "Preferred tempo reconsidered" (ICMPC 2002); the figures 125 BPM peak and 67-150 BPM ranges that came back from search belong to [secondary] sources and are not used here.
- **Meaning for Tanqi [inferred].** For ambiguous music there are two defensible tempi, so a single number has no stable ground truth; the user's own convention (e.g. 81 for Aerials) is what matters, which supports a user-facing half/double control.

### 1.4 Ellis 2007, "Beat tracking by dynamic programming"

[opened] https://www.ee.columbia.edu/~dpwe/pubs/Ellis07-beattrack.pdf (downloaded and read as text), section 3.2 "Global Tempo Estimate":

- Autocorrelation of the onset-strength envelope peaks at every multiple of the period, so "it can be difficult to choose a single best peak among many correlation peaks of comparable magnitude".
- Remedy: multiply the autocorrelation by a Gaussian window on a log-period axis, `W(tau) = exp(-0.5 (log2(tau/tau0)/sigma_tau)^2)`.
- Parameters were fitted on the 20 MIREX-06 training excerpts (30 s each) and gave 77% agreement with tau0 = 0.5 s (120 BPM) and sigma = 1.4 octaves. A refinement adds the TPS resampled to 1/2 and 1/3 length to itself, which "finds the tempo that maximizes the sum of the TPS values at both metrical levels" (duple or triple), scoring 84% with sigma = 0.9 octaves. These figures are on the development data, not a held-out set (the paper says the parameters were adjusted to maximise agreement with it).
- The method reports a secondary tempo "chosen from among (0.33, 0.5, 2, 3) x the primary period", and the ratio of their strengths as relative weights. Note 3:2 is not among those ratios either.
- **Training data required?** Only for two parameters (tau0, sigma). No model training.

### 1.5 Davies and Plumbley

M. E. P. Davies and M. D. Plumbley, "Context-dependent beat tracking of musical audio", IEEE TASLP 15(3):1009-1020, 2007. [abstract-level only] I could confirm the citation (Surrey repository listing via search) but the PDF host failed certificate verification here, so I did not read the text. What I can say [opened] from the code that cites it: aubio's `beattracking.c`, authored by Davies and Brossier, applies a "Rayleigh weight" to the autocorrelation with the comment "sets preferred tempo to 120bpm" (read previously, see tempo-library-comparison.md section 1.3; aubio is GPL so only the idea is usable). The claim that the 2007 paper itself uses a Rayleigh prior is therefore **unverified from the paper**.

### 1.6 Böck, Krebs, Widmer (madmom tempo)

"Accurate Tempo Estimation based on Recurrent Neural Networks and Resonating Comb Filters", ISMIR 2015 [opened] https://www.ismir2015.uma.es/articles/196_Paper.pdf:

- A learned beat activation function (a recurrent network) feeds a bank of resonating comb filters; "simply reporting the highest resonator's histogram peak" gives state of the art on nine of ten datasets [opened, abstract], with no "multi-scale tracking", "metrical modelling" or slow/fast categorisation.
- It was trained with 8-fold cross validation on the evaluation sets (section 3.2.1 and 4). **It needs training data and a trained network.**
- Honest limitation in section 4.3: where drum-and-bass puts kicks off the beat, "the algorithm's histogram usually has a peak at the correct tempo but that this peak is not the highest" and, without "intelligent clustering of the histogram peaks", the reported tempo is often a 3:2-type ratio outside ACC2. This is the same shape as Schism: the right value is in the candidate set, not at the top.
- Its introduction cites Percival and Tzanetakis as using "a support vector machine (SVM) to decide which octave the tempo should be in" (section 2), i.e. a trained octave classifier.
- Licence [opened] https://raw.githubusercontent.com/CPJKU/madmom/main/README.rst: code BSD, but model and data files are CC BY-NC-SA 4.0 and commercial use needs contacting the authors. The models are what makes it work, so the BSD code alone is not a usable detector.

### 1.7 Schreiber and Müller (TempoCNN)

"A Single-Step Approach to Musical Tempo Estimation Using a Convolutional Neural Network", ISMIR 2018 [opened] https://ismir2018.ismir.net/doc/pdfs/141_Paper.pdf (paper CC BY 4.0):

- A CNN on a mel spectrogram classifies tempo directly (256 classes), trained on 8,596 tracks (tempi 44-216 BPM, "sweet octave" 77-154 BPM covering 84.4% of items, shortest interval covering 99%: 65-204 BPM; section 2.4), with scale-and-crop augmentation. About 2.9 million trainable parameters (section 3.2).
- Reported Accuracy1 74.2% on their combined test set, Accuracy2 92.1% versus 93.6% for madmom: it is better at choosing the octave, slightly worse when the octave is ignored (section 4). They attribute madmom's and their earlier method's octave trouble to "traditional periodicity analysis" (DFT and comb filters) (section 4).
- **Needs training data and a trained network.** Licence of the reference code [opened]: `hendriks73/tempo-cnn` repository `LICENSE` is GNU AGPL v3 (https://raw.githubusercontent.com/hendriks73/tempo-cnn/master/LICENSE). Essentia's TempoCNN models fall under "All the models created by the MTG are licensed under CC BY-NC-SA 4.0" with proprietary licences on request (https://essentia.upf.edu/models.html, I read only the first 100k characters and did not find the TempoCNN entry itself, so its per-model licence is [inferred] from that blanket statement). Not usable for an MIT app without a separate licence.
- The 2018 paper's useful fact for us: the tempo distribution of real music is concentrated: sweet octave 77-154 BPM. That is the evidence behind a [75, 150) style window.

### 1.8 What stratum-dsp itself documents

[opened] README and source of stratum-dsp 1.0.0 (`~/.cargo/registry/src/*/stratum-dsp-1.0.0/src/config.rs`, `lib.rs`, `analysis/result.rs`; README https://raw.githubusercontent.com/HLLMR/stratum-dsp/main/README.md lines 127-130):

- Own admission: "~12% of tracks show octave/harmonic confusions (1/2x, 2x, 2/3x ratios)". The README says optional ML refinement is "Phase 2", not present.
- `AnalysisConfig` already has `min_bpm` / `max_bpm` (defaults **40.0 / 240.0**, comments say they were widened from 60 / 180), `enable_tempogram_multi_resolution` (default true; hops 256/512/1024, "intended to reduce metrical-level (T vs 2T vs T/2) errors"), `tempogram_multi_res_use_human_prior` (default **false**; "gentle human-tempo prior as a tie-breaker", applied only for margin < 0.05 and result in 70-180 per `multi_resolution.rs` line 458), `enable_bpm_fusion` (default false), `enable_tempogram_percussive_fallback`, `enable_tempogram_band_fusion`, and `emit_tempogram_candidates` + `tempogram_candidates_top_n`.
- **More than one candidate is available**: with `emit_tempogram_candidates = true`, `result.metadata.tempogram_candidates` is a `Vec<TempoCandidateDebug { bpm, score, fft_norm, autocorr_norm, selected }>`. It is documented as "for diagnostics", so it may change between versions; Tanqi pins `stratum-dsp = "1"`. [measured] it returned 25 entries with `tempogram_candidates_top_n = 10`, so the field does not mean what its name says; I did not look further.
- Also exposed: `metadata.tempogram_multi_res_triggered` (true only for Schism among the five), `tempogram_multi_res_used`, `tempogram_percussive_*`, `grid_stability`, `flags`, `confidence_warnings`.
- There is a mild built-in prior inside candidate scoring (`tempogram.rs` lines 484 and 621: "mild prior toward common DJ/tempo ranges (60-180 BPM) while preserving support for 40-240").

## 2. Techniques, what the sources say, and how realistic each is here

| | Technique | What the sources say | Training data? | Fit for Tanqi (Rust, permissive, phone) |
|---|---|---|---|---|
| a | Tempo prior / preferred range / octave folding | Ellis 2007 sec. 3.2: log-Gaussian weight, centre 120 BPM, sigma 1.4 octaves (0.9 in the refined version), fitted on 20 excerpts. Schreiber 2018 sec. 2.4: sweet octave 77-154 BPM. stratum has `min_bpm`/`max_bpm` and an off-by-default human prior. Nothing in the sources fixes the window, and the Ellis weighting is soft, not a hard clip | Two parameters fitted on a small set | Cheap (no CPU). A post-hoc fold is trivial. Parameters are the risk (3.2, 3.3) |
| b | Look at the top tempogram candidates and pick with a prior | stratum already does a version of this internally (multi-res hypotheses T, 2T, T/2; no 3:2 hypothesis, per its README ratios list only mentions 2/3 as a confusion). Böck 2015 sec. 4.3: right peak present but not highest, no "intelligent clustering" implemented | None for a rule; a rule's constants need labels | Candidates are exposed by `emit_tempogram_candidates` (one config flag, small cost). The rule on top is ours to design and validate |
| c | Several excerpts and voting | Gouyon 2006 abstract proposes voting across *methods*, not across excerpts. I found no primary source that recommends voting across excerpts for octave errors | None | Costs N analyses (stratum is 0.7-1.3 s and about 300 MB each at 60 s). Measured below: did not help |
| d | Onset-strength autocorrelation and metrical relations (2:1, 3:2, 3:1) | Ellis 2007 sec. 3.2: weight peaks, then add the TPS at 2x and 3x lag so "the sum of the TPS values at both metrical levels" decides duple vs triple. Percival and Tzanetakis 2014 (IEEE/ACM TASLP 22(12):1765-1776): autocorrelation with harmonic enhancement and pulse-train cross-correlation, then an SVM to choose the octave [reached only via Böck's description (section 2 of that paper) and Essentia's doc pages (secondary); not opened] | Ellis: two parameters. Percival: an SVM | Ellis is about 80 lines of Rust, `rustfft` already in the dependency tree, 48 ms for a 60 s excerpt (measured), tiny memory. Implementable by us from the paper; the paper is the licence-free source |
| e | Ensemble of two permissive detectors | Gouyon 2006 abstract: "redundant" voting of simpler methods inspired by perception [abstract]. Böck 2015 and Schreiber 2018 are the permissive-licence-unfriendly learned systems | None for agreement checks | stratum + our Ellis-style estimator is the only permissive pair that exists today (bpm-analyzer is live-capture only and scored 0/5, see the earlier doc; aubio is GPL-3.0). Needs a rule for disagreement |
| f | Non-algorithmic: half/double toggle, tap tempo, manual override | The MIREX task scores two tempi per excerpt, treats "dominant perceived tempo" as sometimes one metrical level away from notated, and Schreiber 2020's survey respondents want accurate values for DJ use. None of the papers claims an algorithm that resolves the convention | n/a | Zero CPU, zero risk to detection, the only complete fix for "I call it 81" |

I found no primary source that gives a recipe for 3:2 errors specifically beyond: weight or cluster the histogram/autocorrelation peaks and test duple and triple relationships (Ellis), or use a trained classifier (Percival and Tzanetakis; Schreiber and Müller).

## 3. Measurements on the five references

All via `tprobe`, middle 60 s unless stated; error classes: close = within 2 BPM, near = within 5%, 2x/1/2x, 3:2 / 2:3 = within 4% of those ratios.

### 3.1 stratum's own output fields (default config, `emit_tempogram_candidates = true`)

Candidate lists show `bpm(score, fft_norm, autocorr_norm)`, `*` = selected. First five shown per track, 60 s run.

| Track (ref) | bpm | conf | grid_stability | beats | multi-res triggered | candidates |
|---|---|---|---|---|---|---|
| Aerials (81) | 158.98 | 0.31 | 0.48 | 151 | no | 159.0(1.17,1.00,0.89)* 78.9(0.80,0.21,1.00) 158.0(0.79) 79.8(0.77,0.21,0.95) 160.0(0.75) |
| A Certain Romance (139) | 135.00 | 0.04 | 0.47 | 115 | no | 135.0(1.00,0.57,1.00)* 138.4(0.97,0.53,0.98) 135.8(0.95) 137.5(0.90) 90.0(0.89,0.39,0.98) |
| Phoenix (134) | 134.00 | 0.14 | 0.50 | 168 | no | 134.0(1.16,0.87,0.99)* 133.0(1.00) 166.5(0.73) 135.0(0.70) 66.9(0.69) |
| Schism (109) | 71.50 | 0.65 | 0.52 | 19 | **yes** | 217.0(0.83,0.81,0.87) 216.0(0.81) 107.2(0.69,0.01,1.00) 106.3(0.69) 105.5(0.68) 70.9(0.68)* 108.0(0.66) 71.7(0.65)* |
| Feel Good Inc. (139) | 138.79 | 0.04 | 0.52 | 140 | no | 138.8(0.97,0.59,0.94)* 137.9(0.93) 68.8(0.75) 92.0(0.68) 91.0(0.68) |

Observations [measured]:
- In Schism the correct 109 is represented only by `autocorr_norm` (about 1.0) and a near-zero `fft_norm` (about 0.01); the FFT tempogram supports 217 strongly. The two internal methods disagree, which is a visible sign of trouble [inferred; one track]. The selected 71.5 is two thirds of 107.2 and one third of 214-217.
- In Aerials, 159 beats 79 mainly on `fft_norm` (1.00 vs 0.21) while the autocorrelation prefers 79 (1.00 vs 0.89).
- In A Certain Romance the 3-BPM error is a near tie between 135.0 and 138.4 (score 1.00 vs 0.97): the correct value sits one candidate away. The 4% tolerance in ACC1 would accept 138.4 (0.4% off) but not 135.0 (2.9% off).
- The loudest warning sign is small for the tracks that are right: `conf` is lowest for the two best results (0.04).

### 3.2 stratum config variants (60 s)

| Variant | Aerials | A Certain Romance | Phoenix | Schism | Feel Good |
|---|---|---|---|---|---|
| default | 159.0 (2x) | 135.0 | 134.0 | 71.5 (2:3) | 138.8 |
| `tempogram_multi_res_use_human_prior = true` | 159.0 | 135.0 | 134.0 | 72.0 | 138.8 |
| `enable_bpm_fusion = true` | 159.0 | 135.0 | 134.0 | 71.5 | 138.8 |
| multi-resolution off | 159.0 | 135.0 | 134.0 | **108.0** | 138.8 |
| percussive fallback and band fusion off | 159.0 | 135.0 | 134.0 | 71.5 | 138.8 |
| HPSS onsets on | 159.0 | 135.0 | 134.0 | 71.5 | 138.8 |
| `min/max_bpm` 60-180 | 159.0 | 135.0 | 134.0 | **107.0** | 138.8 |
| 70-140 | **78.9** | 135.0 | 134.0 | **107.0** | 138.8 |
| 80-160 | 159.0 | 135.3 | 134.0 | 107.0 | 138.8 |

- The built-in human prior does nothing here, as its source predicts (the margin condition is rarely met).
- Schism's 3:2 comes from the multi-resolution escalation (Schism is the only track where it triggers), and disappears either when escalation is off or when the candidate range excludes 217. [inferred] The escalation anchors on the top-scored candidate (217), then selects the T/2-type hypothesis among 217's relatives. I did not read the fusion code to full depth, so treat the mechanism as a hypothesis.
- Aerials needs a range cut that excludes 159, i.e. a ceiling under about 158; with 80-160 it still reads 159.

### 3.3 Range restriction, on the references and on ten tracks with no reference

`min_bpm`-`max_bpm` as a stratum config (this changes candidate generation, not just the final pick). The ten extra tracks are the ones in the earlier document; reference values are not known, so the "other" columns show only what changed. 60 s / 90 s.

| Track | 40-240 (now) | 60-180 | 70-140 | 70-150 | 75-150 |
|---|---|---|---|---|---|
| Aerials (81) | 159.0 / 159.0 | 159.0 / 159.0 | 78.9 / 79.9 | 78.9 / 79.9 | 78.9 / 79.9 |
| A Certain Romance (139) | 135.0 / 135.0 | 135.0 / 135.0 | 135.0 / 135.3 | 135.0 / 135.3 | 135.3 / 135.3 |
| Phoenix (134) | 134.0 / 134.0 | same | same | same | same |
| Schism (109) | 71.5 / 72.0 | 107.0 / 107.0 | 107.0 / 107.0 | 107.0 / 107.0 | 107.0 / 107.0 |
| Feel Good (139) | 138.8 / 138.8 | same | same | same | same |
| Thundersnail | 149.7 / 149.7 | 150.0 | **112.3** / 112.3 | 148.9 | 148.9 |
| Hijab Kekasih | 99.7 / 99.7 | **124.9 / 67.0** | **124.9 / 75.0** | **124.9 / 75.0** | **124.9 / 75.0** |
| Telemachus | 159.8 / 159.6 | 160.0 | **79.9** | **79.9** | **79.9** |
| Kill Rock 'n Roll | 90.0 / 93.0 | 89.9 / 92.5 | 92.5 | 92.5 | 92.5 |
| Kitai, Atmospheric Ignition, The Bad Thing, Dare, BOOM, Kereta Malam | unchanged (within 1 BPM) | unchanged | unchanged | unchanged | unchanged |

- On the references, **70-150 gets 5/5 within 5%** (Aerials 78.9 is 2.6% under 81). That is the tempting result.
- **Hijab Kekasih** (the earlier trial had stratum 99.7 and aubio 100.9 to 101.4, so 99.7 is likely right) breaks under every restricted range, and changes between 60 s and 90 s. Its candidate list at 40-240 has 199.4 as the top candidate and escalation picked its half (99.7); with 199.4 removed, escalation anchors elsewhere (124.9, 75.0). [inferred from candidate list and results.]
- **Telemachus** and **Thundersnail** change too (80 or 112 vs 160 or 150). Without references I cannot say which is right (earlier doc: aubio 123 and bpm-analyzer 211 for Telemachus; bpm-analyzer 75 for Thundersnail).
- So restricting the range inside stratum gives a net win on the five and a loss on at least one of the ten. **That is the signature of a rule that is tuned, not principled.** Five labelled tracks cannot arbitrate.

**Post-hoc octave folding** (leave stratum at 40-240 and fold its answer by powers of 2 into `[L, 2L)`), computed from the default output:

| Window | Aerials 159.0 | A Certain Romance 135.0 | Phoenix 134.0 | Schism 71.5 | Feel Good 138.8 |
|---|---|---|---|---|---|
| [60,120) | 79.5 close | 67.5 (1/2x) | 67.0 (1/2x) | 71.5 (2:3) | 69.4 (1/2x) |
| [70,140) | 79.5 close | 135.0 | 134.0 | 71.5 (2:3) | 138.8 (2 BPM from the edge) |
| [75,150) | 79.5 close | 135.0 | 134.0 | 71.5 (2:3) | 138.8 |
| [80,160) | 159.0 (2x) | 135.0 | 134.0 | 143.0 (other) | 138.8 |
| [90,180) | 159.0 (2x) | 135.0 | 134.0 | 143.0 (other) | 138.8 |

- Folding fixes only the octave error, never the 3:2 one, and for windows with floor 80 or higher it breaks Aerials and wrecks Schism further. The window is a free parameter: [70,140) and [75,150) both give 4/5 close or near, but 139 BPM tracks are two BPM from the first window's edge, and a track at 141 would be halved to 70.5 (a constructed risk, not seen here).
- **Principled vs tuned.** The existence of a preferred-tempo region near 120 BPM is backed by the sources (Ellis; McKinney and Moelants via Ellis; Schreiber's sweet octave 77-154 BPM measured on 8,596 tracks). The exact edges, and the choice of hard fold over soft weighting, are not given by any source; picking 150 or 140 here is tuning to five tracks.

### 3.4 Voting across excerpts

Three or five excerpts centred at fixed fractions of the track, default config, median of the stratum outputs:

| Track | 3 x 30 s | 3 x 45 s | 5 x 30 s | single middle 60 s |
|---|---|---|---|---|
| Aerials | 159.0 (2x) | 159.0 (2x) | 160.0 (2x) | 159.0 |
| A Certain Romance | **92.5 (2:3)** | 137.0 | 136.0 | 135.0 |
| Phoenix | 133.7 | 133.7 | 133.7 | 134.0 |
| Schism | 71.9 (2:3) | 73.6 (2:3) | 72.3 (2:3) | 71.5 |
| Feel Good | 136.0 (near) | 138.8 | 138.8 | 138.8 |

- Voting cannot repair a systematic octave or 3:2 preference (it is the same on every excerpt) and 30 s excerpts introduce new outliers (individual runs: 91.4, 92.5, 89.2, 171.6, 142.0, 92.5). 3 x 45 s is about as good as one 60 s run and costs three analyses. **No gain on the references; not recommended.**

### 3.5 An Ellis 2007 estimator I wrote for this test

Implemented from the paper (section 3.2): log-magnitude spectral flux, 2048-point Hann STFT at hop 512 (86 frames/s), 0.5 s local mean removal, autocorrelation, log-Gaussian weight centred at 0.5 s (120 BPM), and the "duple" combination TPS(t) + TPS(2t). I did not interpolate the lag, so there is a quantisation of about 1-2 BPM at 135-140. Results on the middle 60 s; "basic" is the largest weighted peak:

| Track (ref) | stratum | Ellis basic, sigma 1.4 | Ellis basic, sigma 0.9 | Ellis duple, faster level | slower level |
|---|---|---|---|---|---|
| Aerials (81) | 159.0 (2x) | 161.5 (2x) | 161.5 (2x) | 161.5 | **80.7 close** |
| A Certain Romance (139) | 135.0 | 136.0 near | 136.0 near | 136.0 | 68.0 |
| Phoenix (134) | 134.0 | 132.5 close | 132.5 close | 132.5 | 66.3 |
| Schism (109) | 71.5 (2:3) | **107.7 close** | **107.7 close** | 215.3 (2x) | 107.7 |
| Feel Good (139) | 138.8 | **92.3 (2:3)** | 139.7 close | 139.7 | 69.8 |

- With the refined sigma 0.9, "basic" is within 5% on **4/5**; with the standard sigma 1.4 it is 3/5 and breaks Feel Good the way stratum breaks Schism. The result flips with a parameter, **again showing the sensitivity of a single-number rule**. Note that the paper's own best parameter (0.9) is the one that works here; I did not tune it.
- The weight cannot fix Aerials: 159 vs 79.5 weights are 0.96 vs 0.91 at sigma 1.4 and 0.90 vs 0.80 at sigma 0.9 [computed from the formula]; the raw autocorrelation also prefers 161 (0.51) over 79.8 (0.40). A log-Gaussian prior centred at 120 is simply too flat to override the pulse the signal prefers, which is how Ellis designed it (it only breaks near-ties).
- Cost: **48 ms** for the novelty (5,164 frames) plus autocorrelation, on 60 s of audio, single-threaded. Memory is the audio plus a few thousand floats.
- Agreement with stratum on the ten unlabelled tracks (sigma 0.9 basic): within 3% on 7 (Kitai 101.3, Hijab 99.4, The Bad Thing 110.0, Dare 120.2, BOOM 110.0, Kereta Malam 105.5, Kill Rock 92.3); different on Thundersnail (120.2 vs 149.7, ratio 0.80), Atmospheric Ignition (60.1 vs 119.9, octave), Telemachus (114.8 vs 159.8, ratio 0.72). With no labels I cannot say who is right on those three.
- **Ensemble check, the thing that matters:** "accept stratum only if an independent autocorrelation peak sits within 3% of it" would have accepted all five (Aerials 159 vs 161 is an agreed octave error, and Schism's 71.5 also sits near a 71.0 autocorrelation peak), so a simple agreement test does **not** catch the 3:2 error. What does separate Schism is a *non-octave* disagreement between the final answers of the two estimators (71.5 vs 107.7), which happened for Schism alone among the five references.

### 3.6 Synthetic click tracks (sanity check, 60 s, 44.1 kHz, no noise)

"click" = a 1 kHz click on every beat; "kit" = 60 Hz kick on beats 1 and 3, snare-like noise on 2 and 4, 8 kHz hats on every eighth.

| Pattern | True BPM | stratum | notes |
|---|---|---|---|
| click | 70 | 69.81 | close |
| click | 100 | 99.99 | close |
| click | 120 | 119.86 | close |
| click | 140 | 140.00 | close |
| kit | 100 | 99.99 | close (its top-scored candidate was 201, selected 100) |
| kit | 120 | 119.86 | close |
| kit | 75 | **149.72** | 2x; with eighth-note hats 150 is a defensible reading |
| kit | 150 | 149.72 | close |

stratum is accurate on clean material, and its octave error appears where a subdivision is as prominent as the beat: this is a property of the test signal, not evidence about real music. These are the shape of tests section 4 proposes; I have not yet built half/double/3:2 variants of each.

### 3.7 What the references can and cannot tell us

- Five tracks, three failure types, one of each type seen once. A rule fitted to them has at most five data points, and the five include the cases that motivated it.
- **Sample size [inferred from binomial arithmetic, not from a source].** To estimate an accuracy to within about +-10 percentage points at 95% confidence you need about 100 labelled tracks (1.96 * sqrt(0.25/n) = 0.10 gives n = 96). To pick between two rules that differ by 10 points on the same tracks (a paired comparison), tens of tracks that disagree are needed, which means well over 100 in total at current error rates. For any rule with a tuned window edge, reserve a held-out half: aim for **at least 100 labelled tracks, about half of them kept out of tuning**, and make sure the set includes ternary meters, slow ballads, and tracks above 150 BPM. Below that, treat any gain as anecdote. For comparison the training sets in the literature are 8,596 tracks (Schreiber 2018, sec. 2.4) and the MIREX test has 140 files (Böck 2015, section 4.4).
- **Needs training data per technique (as the sources say):** Ellis's tau0/sigma (small, 20 excerpts); Böck 2015 (trained RNN, cross validation); Percival and Tzanetakis (SVM, per Böck's description); Schreiber 2018 (CNN, 8,596 tracks). stratum's weights were tuned on its own sets (155 DJ tracks; README) and 56.7% within 2 BPM on its FMA set (see earlier doc).

## 4. Recommended plan for Tanqi

Ranked cheapest and safest first. All "changes" below are proposals; none is made.

### Step 1. Reference file and probe (no behaviour change)

Create `docs/tempo-references.tsv` (tab separated, one line per track, `#` comments allowed):

```
# artist - title <TAB> reference BPM <TAB> path relative to ~/Music <TAB> note (optional: meter, which level the user means)
System Of A Down - Aerials	81	System Of A Down - Toxicity (2001) [16B-44.1kHz]/14. Aerials.flac	half-time feel
Arctic Monkeys - A Certain Romance	139	Arctic Monkeys - Whatever People Say I Am, That_s What I_m Not (FLAC)/02-07 - A Certain Romance.flac
NMIXX - Phoenix	134	nmixx/NMIXX - Blue Valentine/NMIXX - Blue Valentine/03. Phoenix.flac
Tool - Schism	109	Tool - Lateralus (2001) FLAC/FLAC/05 - schism - lateralus  - tool.flac	odd meter
Gorillaz - Feel Good Inc.	139	2005 - Demon Days (00946 311691 2 0)/06 - Feel Good Inc..flac
```

Probe (a `#[ignore]` test or `examples/tempo_probe.rs`, reading `~/Music` read-only; never write there): for each row, decode and call `audio::tempo::detect` exactly as the app does, and print `estimate`, `ACC1` (4%), `ACC2` (4%, factors 2, 1/2, 3, 1/3), **`ACC3`** (my name for ACC2 plus 3/2 and 2/3, so 3:2 is visible rather than hidden), `OE1 = log2(est/ref)` and the error class, then totals per class. Summary lines: ACC1, ACC1 at 2%, ACC2, count per class. Compare two detector versions by running the probe on both and diffing the lines (paired comparison). Report counts, not just percentages, and print the list of tracks that changed class.

Synthetic tests (unit tests next to the existing ones; the current tests already use click tracks): for BPM in {70, 90, 100, 120, 140, 160} generate a click track, and assert `detect` is within 2 BPM, and run the same through the fold step; plus **variants**: the same pattern with every other beat removed (looks half-speed), with an added 8th-note hat (looks double), and a 3-against-2 pattern (clicks at both T and 1.5T) to see which level is reported. The expected value for ambiguous variants is a set {T, T/2, 2T}, so assert membership of the set and log which; do not fail the build on which member wins (that is a convention, not a bug).

Effect on the five: none. Risk: none. Effort: small. This is what lets every later step be judged.

### Step 2. Manual override and half/double controls (UI, stored per track)

- Store `bpm_override: Option<f32>` (and optionally `bpm_level: i8` for -1/0/+1 halving/doubling) alongside the stored analysis, and display `override.unwrap_or(detected)`. A half/double button and tap tempo (average of the last 4-8 intervals, discard outliers) write the override. Detection never overwrites an override when an analysis is recomputed.
- Effect on the five: Aerials becomes 81 (or 79.5 with half; the user's 81 by tap or typing). Schism becomes 109 by typing or tap; half/double cannot give 109 from 71.5 (needs x1.5): add a **3:2 button** or let the user type the value. A Certain Romance 139 by typing.
- Risk: only UI and storage migration. This is the one step that solves the user's convention problem for any track.
- Sources: MIREX task description (two tempi per excerpt, the dominant tempo may be a level away from the notated one); McKinney and Moelants (listeners split between two levels).
- Test: extend the probe with an `override` column and assert detection never changes a stored override.

### Step 3. Post-hoc octave fold on stratum's result

In `detect`, after the `analyze_audio` call: `fold_into(bpm, 75.0)` which doubles while below 75 and halves while at or above 150 (a loop, so 40 BPM goes to 80 and 240 BPM goes to 120). Keep `AnalysisConfig::default()` untouched.
- Expected effect on the five: Aerials 159 to 79.5 (close). The other four unchanged; Schism stays 71.5 (wrong).
- Risk: **a track that is truly at 150-160 (or below 75) gets halved or doubled.** On the ten unlabelled tracks the effect of the default 40-240 stratum output through the fold [75,150): Thundersnail 149.7 stays (but is within 0.3 of the edge and would flip at 150.5), Telemachus 159.8 becomes 79.9, the other eight are unchanged [computed from the table above]. So the fold would change one of ten, to a value with no known reference (the other detectors disagreed with each other on Telemachus: aubio 123, bpm-analyzer 211).
- Why this and not the `min_bpm`/`max_bpm` config: the config changes candidate generation and moved Hijab Kekasih from a likely-right 99.7 to 124.9 or 75.0, while the fold cannot make a result that already sits inside the window worse.
- Principled part: preference for a 77-154 region (Schreiber 2018) and for 120 (Ellis). Tuned part: the edges 75 and 150 (not from a source; chosen as the nearest round numbers to Schreiber's measured sweet octave 77-154). **Do not ship before the TSV has about 100 tracks**, or ship with the fold behind a setting that defaults to off, the override (step 2) in place.
- Test: unit tests that `fold_into(159.0)` gives 79.5, `fold_into(100.0)` is unchanged, `fold_into(37.0)` gives 74.0, and the probe's before/after class counts on the TSV.

### Step 4. Second opinion: Ellis-style autocorrelation estimator, shadow mode first

Add `fn onset_autocorr_tempo(samples, sample_rate) -> Option<f32>` in `tempo.rs` (about 80 lines: windowed FFT via `rustfft`, which stratum already depends on, spectral flux, local-mean removal, autocorrelation, log-Gaussian weight centred at 120 BPM with sigma 0.9, parabolic interpolation of the peak lag). Run it on the same excerpt (about 50 ms on desktop; **Android time not measured**). Store or log both values; do not change the displayed BPM yet.
- Disagreement rule to evaluate on the TSV: if the two estimates differ by a ratio that is *not* about 1, 2 or 1/2 (for example 1.5 or 2/3), mark the track "uncertain" in the UI (the user is prompted to confirm via step 2) and keep stratum's number. On the references it would flag Schism only (71.5 vs 107.7) and flag none of the other four; on the ten unlabelled tracks it would flag Thundersnail (ratio 0.80), Telemachus (0.72) and not the others [measured].
- Promotion rule worth testing later, **not** to be adopted on five tracks: when flagged and the Ellis value lies within 3% of one of stratum's candidates (needs `emit_tempogram_candidates = true`, which adds a candidate Vec but no extra analysis; Schism's 107.2 qualifies), prefer the Ellis value. This is a tuned rule: it makes Schism right and I cannot show it does not hurt elsewhere.
- Risk: more code to maintain, a second place that can be wrong, and a UI state ("uncertain") to design. The estimator has two fitted parameters from a 20-excerpt set (Ellis), and its result flipped on Feel Good with sigma 1.4 vs 0.9 here. License: implemented from a published algorithm, no third-party code.
- Test: synthetic clicks at 70-160 (expect within 3%), the half/double/3:2 variants (expect membership in {T, T/2, 2T}), and the probe on the TSV comparing: stratum alone, Ellis alone, flagged set, and the promotion rule's before/after class counts.

### Step 5. Only after 100+ labelled tracks

Re-evaluate: the fold edges, whether the Ellis override rule helps, the `min_bpm`/`max_bpm` question, and whether multi-resolution escalation should be disabled (Schism was right with it off, 108.0; but it exists to fix other octave cases and I did not run it off over the other tracks). Also run stratum on 90 s (what the app uses) rather than 60 s, since answers changed for Schism and Hijab.

### What I would not do

- **GPL libraries (aubio-rs and aubio, GPL-3.0).** Not acceptable under the MIT licence unless the user relicenses. It did best in the earlier trial (4/5 within 5%), which is why this needs saying: the temptation is real, the decision is theirs. Idea-level reuse is different: the Rayleigh-weight idea in aubio's code is the same family as Ellis's published log-Gaussian, which is free to implement from the paper.
- **madmom models or TempoCNN.** The madmom code is BSD but its models are CC BY-NC-SA 4.0; `tempo-cnn` code is AGPL v3 and the Essentia models are CC BY-NC-SA 4.0 by the blanket statement I read. A trained network also needs an inference runtime on Android (stratum's `ml` feature is not built by default and its README says ML refinement is a future phase). Training our own needs thousands of labelled tracks (Schreiber used 8,596).
- **`bpm-analyzer`.** Live-capture only, 0/5 on the references (earlier doc).
- **Voting across short excerpts.** Measured worse or equal (3.4), and it multiplies stratum's memory and time.
- **Gating on `bpm_confidence` or on the beat grid.** The wrong Schism had the highest confidence (0.65) and the correct tracks the lowest (0.04); the grid's beat intervals did not match the tempo on any of the five.
- **Hard-coding `min_bpm`/`max_bpm` to 70-150 because it scored 5/5.** It is the best number on the page and the least trustworthy one: it moved Hijab Kekasih away from a likely-right answer, and the five tracks that "validate" it are the five that motivated it.
- **Retuning stratum's internal weights.** There is no source and no data for it.

## 5. What I could not verify

- The full text of Gouyon et al. 2006 (only the abstract and the contest's own evaluation page), Davies and Plumbley 2007 (host certificate failure), Percival and Tzanetakis 2014 (not opened, described only through Böck 2015 and Essentia doc pages), Moelants 2002 and the McKinney and Moelants papers (abstract-level through a search snippet; the page refused connections).
- Whether `emit_tempogram_candidates` is a stable public API (it is documented for diagnostics; it returned 25 rows for `top_n = 10`).
- Android speed and memory for the Ellis estimator and for stratum with candidates enabled (desktop numbers only).
- Whether any of the ten unlabelled tracks' answers are right; I only report agreement.
- The legal reading of any licence (not legal advice).

## Sources

All opened unless marked.

- ISMIR 2004 tempo contest, evaluation page (ACC1, ACC2 definitions): https://mtg.upf.edu/ismir2004/contest/tempoContest/node7.html
- Gouyon et al., IEEE TASLP 14(5), 2006, repository record [abstract]: https://mtg.upf.edu/node/415 (DOI 10.1109/TSA.2005.858509)
- MIREX audio tempo estimation, 2018 task page (P-score, two tempi, 4% tolerance): https://music-ir.org/mirex/wiki/2018:Audio_Tempo_Estimation
- Ellis, "Beat tracking by dynamic programming", JNMR 2007: https://www.ee.columbia.edu/~dpwe/pubs/Ellis07-beattrack.pdf (section 3.2)
- McKinney and Moelants, "Ambiguity in tempo perception ..." [abstract via search snippet only]: https://cspeech.ucd.ie/rppw/rppw10/mckinney/abstract.html
- Davies and Plumbley, IEEE TASLP 15(3):1009-1020, 2007 [citation only, text not opened]: https://openresearch.surrey.ac.uk/esploro/outputs/journalArticle/Context-dependent-beat-tracking-of-musical-audio/99511418902346
- Böck, Krebs, Widmer, ISMIR 2015: https://www.ismir2015.uma.es/articles/196_Paper.pdf
- madmom README (licences): https://raw.githubusercontent.com/CPJKU/madmom/main/README.rst
- Schreiber and Müller, ISMIR 2018: https://ismir2018.ismir.net/doc/pdfs/141_Paper.pdf
- `hendriks73/tempo-cnn` LICENSE (AGPL v3): https://raw.githubusercontent.com/hendriks73/tempo-cnn/master/LICENSE
- Essentia models page (CC BY-NC-SA 4.0 statement, partial read): https://essentia.upf.edu/models.html
- Schreiber, Urbano, Müller, TISMIR 3(1), 2020: https://www.audiolabs-erlangen.com/content/05_fau/professor/00_mueller/03_publications/2020_SchreiberUM_MusicTempo_TISMIR_ePrint.pdf
- Percival and Tzanetakis, IEEE/ACM TASLP 22(12):1765-1776, 2014 [citation only; known through Böck 2015 section 2 and Essentia reference pages, secondary]
- stratum-dsp README: https://raw.githubusercontent.com/HLLMR/stratum-dsp/main/README.md, and source of stratum-dsp 1.0.0 in the local cargo registry (`config.rs`, `lib.rs`, `analysis/result.rs`, `features/period/*.rs`)
- Tanqi: `src/audio/tempo.rs`, `src/audio/mod.rs` (lines 126, 132, 161), `docs/tempo-library-comparison.md`
