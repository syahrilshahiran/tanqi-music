use crate::ui::{
    api,
    components::{
        icons::{CloseIcon, PauseIcon, PlayIcon, PulseIcon},
        spectrum_chart::SpectrumChart,
    },
    jobs::{audio_analysis_job, use_jobs, wait_for_job},
    player::use_player,
    util::{file_url, format_seconds, time_ago},
};
use app_lib::models::{AudioAnalysis, TrackSummary};
use crate::ui::suggestion::Suggested;
use dioxus::prelude::*;
use serde_json::{json, Value};

/// Every EQ slider's gain is clamped to this range on the backend (see
/// `audio::MAX_SUGGESTED_GAIN_DB`) - used here just to size the sliders'
/// tracks consistently, not to re-derive or re-clamp the values.
const MAX_GAIN_DB: f32 = 12.0;

/// `AudioAnalysis` only snapshots the fields it needs (title/artist/
/// thumbnail) - reopening `AudioAnalysisModal` needs a `TrackSummary` shape,
/// so the rest is filled in with nulls/defaults the modal doesn't use once an
/// `initial_analysis` is already in hand. Shared by the EQ browser page and
/// the local-file status indicator, which both reopen the modal from an
/// already-computed `AudioAnalysis`.
pub fn track_from_audio_analysis(analysis: &AudioAnalysis) -> TrackSummary {
    TrackSummary {
        id: analysis.track_id.clone(),
        title: analysis.title.clone(),
        artist: analysis.artist.clone(),
        album: None,
        duration_ms: None,
        thumbnail_url: analysis.thumbnail_url.clone(),
    }
}

/// A band's frequency as a label ("31 Hz" ... "16 kHz").
fn band_label(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{} kHz", hz / 1000.0)
    } else {
        format!("{hz} Hz")
    }
}

/// Rough, unscientific heuristic for turning relative band levels into a
/// plain-language EQ pointer - not derived from any loudness standard, just
/// "which band sticks out from the other two, and by how much." Thresholds
/// are arbitrary starting points to tune once there's real usage to compare
/// against.
fn eq_summary(a: &AudioAnalysis) -> String {
    // An analysis with a stored spectrum is read against the reference; an older one by the heuristic below.
    match app_lib::audio::verdict(&a.spectrum_db) {
        Some(v) => v.long,
        None => legacy_eq_summary(a).to_string(),
    }
}

fn legacy_eq_summary(a: &AudioAnalysis) -> &'static str {
    let (bass, mid, treble) = (a.bass_db, a.mid_db, a.treble_db);
    let bass_vs_treble = bass - treble;
    if bass_vs_treble > 15.0 {
        return "Very bass-heavy - try cutting bass or boosting treble.";
    }
    if bass_vs_treble > 8.0 {
        return "Leans bass-heavy - a slight bass cut or treble boost may balance it out.";
    }
    if bass_vs_treble < -15.0 {
        return "Very bright/treble-heavy - try cutting treble or boosting bass.";
    }
    if bass_vs_treble < -8.0 {
        return "Leans bright - a slight treble cut or bass boost may help.";
    }
    if (mid - (bass + treble) / 2.0).abs() > 10.0 {
        return if mid > (bass + treble) / 2.0 {
            "Mid-forward (vocals/lead instruments dominate) - a gentle mid scoop can open it up."
        } else {
            "Scooped mids - a mid boost can help vocals and leads cut through."
        };
    }
    "Fairly balanced across bass, mid, and treble - a flat EQ should suit it well."
}

/// One vertical slider column, styled to read like a real EQ: label on
/// top, a track with a center-zero fill, the value below.
#[component]
fn EqSlider(label: String, value: f32, range: f32) -> Element {
    let clamped = value.clamp(-range, range);
    let fill_pct = clamped.abs() / range * 50.0;
    let (fill_class, fill_style) = if clamped >= 0.0 {
        ("eq-slider__fill eq-slider__fill--boost", format!("bottom: 50%; height: {fill_pct}%"))
    } else {
        ("eq-slider__fill eq-slider__fill--cut", format!("top: 50%; height: {fill_pct}%"))
    };
    let sign = if value > 0.0 { "+" } else { "" };
    // JS toFixed(0) rounds half away from zero (Rust's `{:.0}` doesn't).
    let rounded = value.round() as i32;
    rsx! {
        div { class: "eq-slider",
            span { class: "eq-slider__label", "{label}" }
            div { class: "eq-slider__track",
                div { class: "eq-slider__zero" }
                div { class: "{fill_class}", style: "{fill_style}" }
            }
            span { class: "eq-slider__value", "{sign}{rounded}" }
        }
    }
}

/// Drives the browser-side preview engine (`assets/preview.js`).
fn pcmd(name: &str, args: Value) {
    document::eval(&format!("tqPreview.cmd({}, {})", json!(name), args));
}

#[component]
pub fn AudioAnalysisModal(
    track: TrackSummary,
    /// Pre-loaded analysis (e.g. reopened from the EQ browser page) - skips
    /// the initial fetch.
    initial_analysis: Option<AudioAnalysis>,
    /// Set for a local file analyzed for convenience (never saved to the DB):
    /// playback reads this path directly and "Re-analyze" is hidden.
    local_file_path: Option<String>,
    on_close: EventHandler<()>,
) -> Element {
    let player = use_player();
    let jobs = use_jobs();
    let has_initial = initial_analysis.is_some();
    let mut analysis = use_signal(|| initial_analysis.clone());
    let mut loading = use_signal(|| !has_initial);
    let mut error = use_signal(|| None::<String>);

    let mut audio_loading = use_signal(|| false);
    let mut audio_error = use_signal(|| None::<String>);
    let mut is_playing = use_signal(|| false);
    let mut current_time = use_signal(|| 0.0_f64);
    let mut duration = use_signal(|| 0.0_f64);
    let mut loaded = use_signal(|| false);
    let mut eq_enabled = use_signal(|| true);
    // True for the duration of a seek-bar drag/keypress gesture.
    let mut seeking = use_signal(|| false);


    // (Re-)runs the analysis; shared by the initial load and "Re-analyze".
    let run = {
        let track = track.clone();
        move |force: bool| {
            let track = track.clone();
            async move {
                loading.set(true);
                error.set(None);
                // Queued like the other background jobs: it waits its turn behind
                // them, shows in the jobs list, and closing this window doesn't stop it.
                let id = track.id.clone();
                let queued = jobs.enqueue(audio_analysis_job(track, force));
                match wait_for_job(jobs, queued).await {
                    Ok(()) => match api::get_audio_analysis(id).await {
                        Ok(Some(a)) => analysis.set(Some(a)),
                        Ok(None) => error.set(Some("The analysis was not saved".to_string())),
                        Err(e) => error.set(Some(e)),
                    },
                    Err(e) => error.set(Some(e)),
                }
                loading.set(false);
            }
        }
    };

    // Initial load: already have it, or nothing to fetch remotely -> skip.
    // Otherwise show a cached analysis if there is one; if not, wait for the Run button (like the lyrics
    // deep dive, nothing heavy starts just because a window was opened).
    {
        let id = track.id.clone();
        let skip = has_initial || local_file_path.is_some();
        use_future(move || {
            let id = id.clone();
            async move {
                if skip {
                    return;
                }
                match api::get_audio_analysis(id).await {
                    Ok(Some(cached)) => {
                        analysis.set(Some(cached));
                        loading.set(false);
                    }
                    Ok(None) => loading.set(false),
                    Err(e) => {
                        error.set(Some(e));
                        loading.set(false);
                    }
                }
            }
        });
    }

    // Events from the preview engine.
    use_future(move || async move {
        let mut ev = document::eval("tqPreview.setEmitter((e) => dioxus.send(e)); await new Promise(() => {});");
        while let Ok(v) = ev.recv::<Value>().await {
            match v["ev"].as_str().unwrap_or("") {
                "loaded" => {
                    duration.set(v["duration"].as_f64().unwrap_or(0.0));
                    loaded.set(true);
                    audio_loading.set(false);
                    pcmd("play", json!({"offset": 0}));
                }
                "error" => {
                    audio_error.set(Some(v["message"].as_str().unwrap_or("playback failed").to_string()));
                    audio_loading.set(false);
                }
                "state" => {
                    is_playing.set(v["playing"].as_bool().unwrap_or(false));
                    if !*is_playing.peek() {
                        current_time.set(v["t"].as_f64().unwrap_or(0.0));
                    }
                }
                // While the user is dragging (or holding an arrow key on) the
                // seek bar, leave `current_time` alone - otherwise the
                // still-actually-playing position would overwrite it every tick,
                // fighting the drag and making it look like nothing responds.
                "time" => {
                    if !*seeking.peek() {
                        current_time.set(v["t"].as_f64().unwrap_or(0.0));
                    }
                }
                "ended" => {
                    is_playing.set(false);
                    current_time.set(v["duration"].as_f64().unwrap_or(0.0));
                }
                _ => {}
            }
        }
    });

    // This preview is a second, self-contained audio graph, so it doesn't
    // inherit the main player's master volume; it follows the shared value
    // live so changing it in the player bar (or muting) applies mid-playback.
    use_effect(move || {
        let (v, m) = ((player.volume)(), (player.muted)());
        let gain = if m { 0.0 } else { v.clamp(0.0, 1.0) * v.clamp(0.0, 1.0) };
        pcmd("setVolume", json!({"gain": gain}));
    });

    // Live-toggles the EQ on/off for A/B comparison without rebuilding the
    // graph (the engine only rebuilds when the band layout changes).
    use_effect(move || {
        let enabled = eq_enabled();
        let Some(a) = analysis() else { return };
        pcmd("setEq", Suggested::of(&a, (player.eq_strength)()).engine_json(enabled));
    });

    // Closing tears down the engine's context. Done from a root-scoped task:
    // the modal's own scope is already gone.
    use_drop(move || {
        dioxus::core::spawn_forever(async move {
            document::eval("tqPreview.cmd('close', {})");
        });
    });

    let handle_play_pause = {
        let track = track.clone();
        let local_file_path = local_file_path.clone();
        move |_| {
            if is_playing() {
                pcmd("pause", json!({}));
                return;
            }
            if loaded() {
                let t = current_time();
                pcmd("play", json!({"offset": if t >= duration() { 0.0 } else { t }}));
                return;
            }

            audio_error.set(None);
            audio_loading.set(true);
            let track = track.clone();
            let local_file_path = local_file_path.clone();
            spawn(async move {
                // Both "local" cases read straight from disk: `local_file_path` is
                // the ephemeral one-off upload flow (EQ page's "Upload local file"),
                // a `local:`-prefixed `id` is a persisted library track.
                let local = local_file_path.or_else(|| app_lib::local_ref::decode(&track.id).map(str::to_string));
                let Some(path) = local else {
                    audio_error.set(Some("only local files can be analyzed".to_string()));
                    audio_loading.set(false);
                    return;
                };
                let url = file_url(&path);
                // Fetched and decoded in the webview; the engine answers with
                // `loaded` (which starts playback) or `error`.
                pcmd("load", json!({"url": url}));
            });
        }
    };

    // The seek bar: `oninput` fires continuously while dragging - only the
    // displayed position updates there, with no audio engine involvement. The
    // actual seek only commits once, on release (`onchange`/mouseup/keyup);
    // committing per tick restarted playback that many times, which also made
    // dragging fast flip the button to "Play" while audio kept playing.
    let mut commit_seek = move |t: f64| {
        seeking.set(false);
        current_time.set(t);
        pcmd("seek", json!({"t": t}));
    };
    // `change` only fires when the value moved; a press-and-release in place
    // still has to end the gesture.
    let mut release_seek = move || {
        if *seeking.peek() {
            let t = *current_time.peek();
            commit_seek(t);
        }
    };

    let analysis_now = analysis();
    let strength = (player.eq_strength)();
    let loudness_title = analysis_now
        .as_ref()
        .map(|a| {
            let peak = a.true_peak_db.map(|p| format!(", true peak {p:.1} dBTP")).unwrap_or_default();
            format!("Integrated loudness (ITU-R BS.1770 / EBU R128){peak}. Playback is levelled toward -14 LUFS, and a track's peak is never boosted past -1 dBTP.")
        })
        .unwrap_or_default();
    // What is heard and shown: the suggestion at the chosen strength.
    let Suggested { bands: shown_bands, preamp: shown_preamp } =
        analysis_now.as_ref().map(|a| Suggested::of(a, strength)).unwrap_or_default();
    // The chart: this track against the reference, with the suggested EQ (at the strength in use) over it.
    let chart = analysis_now
        .as_ref()
        .and_then(|a| app_lib::audio::chart_data(&a.spectrum_db, &shown_bands, a.lowpass_hz));
    let dur = duration();
    let range_value = current_time().min(dur);
    let seek_pct = if dur > 0.0 { range_value / dur * 100.0 } else { 0.0 };
    let play_label = if audio_loading() {
        "…"
    } else if is_playing() {
        "Pause"
    } else {
        "Play"
    };
    rsx! {
        // Focused on mount so Escape reaches `onkeydown` here; the click/key
        // stopPropagation keeps this from also closing the modal it was
        // opened from (SongInfoModal's own backdrop).
        div {
            class: "modal-scrim aam__scrim",
            tabindex: "0",
            onmounted: move |e| async move {
                let _ = e.set_focus(true).await;
            },
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    e.stop_propagation();
                    on_close.call(());
                }
            },
            onclick: move |e| {
                e.stop_propagation();
                on_close.call(());
            },
            div {
                class: "aam",
                onclick: move |e| e.stop_propagation(),
                div { class: "aam__head",
                    div { class: "aam__head-text",
                        p { class: "aam__eyebrow", "Audio Analysis" }
                        p { class: "aam__title line-clamp-2",
                            "{track.title} "
                            span { class: "aam__artist", "· {track.artist}" }
                        }
                    }
                    button {
                        onclick: move |_| on_close.call(()),
                        class: "aam__btn btn--icon",
                        title: "Close",
                        "aria-label": "Close",
                        CloseIcon { size: 18 }
                    }
                }

                // Re-analyzing keeps the current result on show, dimmed, under the loading line.
                div { class: if loading() && analysis_now.is_some() { "aam__body aam__body--busy" } else { "aam__body" },
                    if let Some(e) = error() {
                        div { class: "alert-error aam__error", "{e}" }
                    }

                    if loading() {
                        div { class: "aam__loading",
                            span { if analysis_now.is_some() { "Re-analyzing - it starts when earlier jobs finish, and can take a minute or two" } else { "Analyzing audio - it starts when earlier jobs finish, and can take a minute or two" } }
                            span { class: "dots aam__dots", "aria-hidden": "true",
                                span {}
                                span {}
                                span {}
                            }
                        }
                    }

                    if analysis_now.is_none() && !loading() && local_file_path.is_none() {
                        div { class: "aam__intro",
                            h3 { class: "section-label lyrics-modal__heading-label", "Audio analysis" }
                            p { class: "lyrics-modal__vibe",
                                "This listens to the track on your own machine and measures its loudness, tempo, key and frequency balance, then suggests an EQ to even it out. Nothing is uploaded."
                            }
                            p { class: "lyrics-modal__none",
                                "It starts when earlier jobs finish and can take a minute or two. The result is saved, so it only needs to run once per track."
                            }
                            button {
                                onclick: {
                                    let run = run.clone();
                                    move |_| {
                                        let run = run.clone();
                                        spawn(async move { run(false).await });
                                    }
                                },
                                class: "lyrics-modal__btn lyrics-modal__btn--wide lyrics-modal__run",
                                "Run audio analysis"
                            }
                        }
                    }

                    if let Some(a) = &analysis_now {
                        div { class: "aam__summary-row",
                            p { class: "aam__summary", "{eq_summary(a)}" }
                            span { class: "aam__chips",
                                if let Some(key) = &a.key {
                                    span {
                                        title: "Detected musical key - only shown when detection was confident enough; key is markedly less reliable than tempo on weakly tonal or heavily percussive tracks",
                                        class: "aam__chip",
                                        "{key}"
                                    }
                                }
                                if let Some(lufs) = a.loudness_lufs {
                                    span {
                                        title: "{loudness_title}",
                                        class: "aam__chip aam__chip--num",
                                        "{lufs:.1} LUFS"
                                    }
                                }
                                if let Some(bpm) = a.bpm {
                                    span {
                                        title: "Estimated tempo via stratum-dsp's beat-tracking engine - validated at ~88% accuracy within ±2 BPM on real tracks",
                                        class: "aam__chip aam__chip--num",
                                        "{bpm.round() as i32} BPM"
                                    }
                                }
                            }
                        }

                        if let Some(data) = chart.clone() {
                            SpectrumChart { data, strength_pct: (strength * 100.0).round() as u32 }
                        }

                        div { class: "aam__section-row",
                            p { class: "section-label", "Suggested EQ" }
                        }
                        if shown_bands.is_empty() || strength <= 0.0 {
                            p { class: "aam__empty",
                                if strength <= 0.0 && !shown_bands.is_empty() {
                                    "The suggested EQ is switched off at 0% strength."
                                } else {
                                    "Nothing to suggest - this track sits within the usual range, so it needs no EQ."
                                }
                            }
                        } else {
                            div { class: "aam__sliders",
                                EqSlider { label: "Preamp", value: shown_preamp, range: MAX_GAIN_DB }
                                for (i, band) in shown_bands.iter().enumerate() {
                                    EqSlider {
                                        key: "{i}",
                                        label: band_label(band.hz),
                                        value: band.suggested_gain_db,
                                        range: MAX_GAIN_DB,
                                    }
                                }
                            }
                        }


                        if strength < 1.0 && !shown_bands.is_empty() {
                            p { class: "aam__note aam__note--tight",
                                "Shown and heard at {(strength * 100.0).round()}% strength - change it in the EQ panel or in Settings."
                            }
                        }
                        p { class: "aam__note",
                            "Compared with how music usually sounds: a band is suggested only where this track strays beyond the normal range, and only part of the way back. The reference was measured from a small library, so treat it as a hint and fine-tune by ear. A headphone correction you have chosen is applied on top and is not taken into account here."
                        }

                        div { class: "aam__listen",
                            div { class: "aam__section-row",
                                p { class: "section-label", "Listen" }
                                button {
                                    onclick: move |_| eq_enabled.set(!eq_enabled()),
                                    title: if eq_enabled() { "Suggested EQ on - click to hear the song untouched" } else { "Suggested EQ off - click to apply it" },
                                    "aria-pressed": "{eq_enabled()}",
                                    class: "dev-tag aam__eqtag",
                                    PulseIcon { size: 14 }
                                    "Suggested EQ"
                                }
                            }

                            if let Some(e) = audio_error() {
                                div { class: "alert-error aam__error aam__error--small aam__error--listen", "{e}" }
                            }

                            div { class: "aam__player",
                                button {
                                    onclick: handle_play_pause,
                                    disabled: audio_loading(),
                                    title: "{play_label}",
                                    "aria-label": "{play_label}",
                                    class: if audio_loading() { "dev-key aam__play aam__play--busy" } else { "dev-key aam__play" },
                                    if is_playing() {
                                        PauseIcon { size: 18 }
                                    } else {
                                        PlayIcon { size: 18 }
                                    }
                                }
                                span { class: "aam__time aam__time--elapsed",
                                    "{format_seconds(current_time())}"
                                }
                                input {
                                    r#type: "range",
                                    min: "0",
                                    max: "{dur}",
                                    step: "0.1",
                                    value: "{range_value}",
                                    style: "--pct: {seek_pct}%",
                                    onmousedown: move |_| seeking.set(true),
                                    ontouchstart: move |_| seeking.set(true),
                                    onkeydown: move |_| seeking.set(true),
                                    oninput: move |e| {
                                        if let Ok(t) = e.value().parse::<f64>() {
                                            seeking.set(true);
                                            current_time.set(t);
                                        }
                                    },
                                    onchange: move |e| {
                                        if let Ok(t) = e.value().parse::<f64>() {
                                            commit_seek(t);
                                        }
                                    },
                                    onmouseup: move |_| release_seek(),
                                    ontouchend: move |_| release_seek(),
                                    onkeyup: move |_| release_seek(),
                                    disabled: dur == 0.0,
                                    class: "aam__seek",
                                }
                                span { class: "aam__time",
                                    "{format_seconds(dur)}"
                                }
                            }
                        }
                    }
                }

                if let Some(a) = &analysis_now {
                    div { class: "aam__foot",
                        span { class: "truncate",
                            if local_file_path.is_some() {
                                "Local file - not saved"
                            } else {
                                "{time_ago(&a.created_at)}"
                            }
                        }
                        if local_file_path.is_none() {
                            button {
                                onclick: {
                                    let run = run.clone();
                                    move |_| {
                                        let run = run.clone();
                                        spawn(async move { run(true).await });
                                    }
                                },
                                disabled: loading(),
                                class: "aam__btn",
                                "Re-analyze"
                            }
                        }
                    }
                }
            }
        }
    }
}
