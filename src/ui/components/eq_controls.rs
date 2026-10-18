use crate::ui::{
    components::icons::{CloseIcon, EditIcon, TrashIcon},
    player::use_player,
};
use dioxus::prelude::*;

/// Every EQ slider's gain is clamped to this range on the backend (see
/// `audio::MAX_SUGGESTED_GAIN_DB`) - used here just to size the sliders'
/// tracks consistently, not to re-derive or re-clamp the values.
const MAX_GAIN_DB: f32 = 12.0;

/// "+1.5" / "-3.0" / "0.0" (never "-0.0", which JS's toFixed never prints).
fn gain_text(db: f32) -> String {
    let t = format!("{db:.1}");
    if t == "-0.0" {
        "0.0".into()
    } else if db > 0.0 {
        format!("+{t}")
    } else {
        t
    }
}

/// One interactive vertical slider column for a single band - its own
/// frequency is editable (a plain number input, not a fixed label) since
/// bands are no longer pinned to a fixed 10-band ISO layout, and it can be
/// removed entirely. Built from a plain horizontal `<input type="range">`
/// rotated -90deg rather than relying on `-webkit-appearance:
/// slider-vertical` or `writing-mode` tricks, since this app has already
/// hit a WebKitGTK-specific Web Audio gap once and a normal range input
/// rotated in CSS is the one approach guaranteed to behave the same on every
/// platform.
#[component]
fn EqBandSlider(
    freq: f32,
    gain: f32,
    on_freq_change: EventHandler<f32>,
    on_gain_change: EventHandler<f32>,
    on_remove: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "eq-band",
            input {
                r#type: "number",
                min: "1",
                value: "{freq}",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f32>() {
                        on_freq_change.call(v);
                    }
                },
                "aria-label": "Band frequency in Hz",
                class: "eq-band__freq",
            }
            div { class: "eq-band__track",
                input {
                    r#type: "range",
                    min: "{-MAX_GAIN_DB}",
                    max: "{MAX_GAIN_DB}",
                    step: "0.5",
                    value: "{gain}",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<f32>() {
                            on_gain_change.call(v);
                        }
                    },
                    "aria-label": "Gain at {freq} Hz",
                    style: "width: 5rem; transform: rotate(-90deg)",
                }
            }
            span { class: "eq-band__gain",
                "{gain_text(gain)}"
            }
            button {
                onclick: move |_| on_remove.call(()),
                title: "Remove band",
                "aria-label": "Remove band",
                class: "eq-band__remove",
                "×"
            }
        }
    }
}

#[component]
fn PreampSlider(value: f32, on_change: EventHandler<f32>) -> Element {
    rsx! {
        div { class: "eq-band",
            span { class: "eq-band__pre-label", "Pre" }
            div { class: "eq-band__track",
                input {
                    r#type: "range",
                    min: "{-MAX_GAIN_DB}",
                    max: "{MAX_GAIN_DB}",
                    step: "0.5",
                    value: "{value}",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<f32>() {
                            on_change.call(v);
                        }
                    },
                    "aria-label": "Preamp",
                    style: "width: 5rem; transform: rotate(-90deg)",
                }
            }
            span { class: "eq-band__gain",
                "{gain_text(value)}"
            }
        }
    }
}

#[component]
pub fn EqControls(on_close: EventHandler<()>) -> Element {
    let player = use_player();
    let features = crate::ui::settings::use_features();
    let analysis = (player.analysis)();
    let bands = (player.eq_bands)();
    let preamp = (player.eq_preamp)();
    let strength_pct = ((player.eq_strength)() * 100.0).round() as u32;
    let correction = (player.correction_profile)();
    let correction_enabled = (player.correction_enabled)();
    let headphones = (player.headphones)();
    let active = (player.active_headphone)();
    let mut show_import = use_signal(|| false);
    let mut import_name = use_signal(String::new);
    let mut import_text = use_signal(String::new);
    let mut import_error = use_signal(|| None::<String>);
    // The saved correction being edited, if the form is for editing rather than adding.
    let mut editing = use_signal(|| None::<i64>);
    let analyzed = analysis.is_some();

    let handle_import = move |_| {
        let (name, text) = (import_name(), import_text());
        let edit_id = editing();
        spawn(async move {
            let result = match edit_id {
                Some(id) => player.update_headphone(id, name, text).await,
                None => player.save_headphone(name, text).await,
            };
            match result {
                Ok(()) => {
                    editing.set(None);
                    import_error.set(None);
                    import_name.set(String::new());
                    import_text.set(String::new());
                    show_import.set(false);
                }
                Err(e) => import_error.set(Some(e)),
            }
        });
    };

    let btn = "eq__btn";

    let correction_section = rsx! {
            div { class: "eq__correction",
                div { class: "eq__head eq__head--tight",
                    p { class: "section-label", "Headphone correction" }
                    if !show_import() {
                        button {
                            onclick: move |_| {
                                editing.set(None);
                                import_name.set(String::new());
                                import_text.set(String::new());
                                show_import.set(true);
                            },
                            class: "{btn}",
                            "Add"
                        }
                    }
                }

                if show_import() {
                    div { class: "eq__import",
                        input {
                            value: "{import_name}",
                            oninput: move |e| import_name.set(e.value()),
                            placeholder: "Name, e.g. Sony WH-1000XM4",
                            maxlength: "60",
                            class: "eq__import-name",
                        }
                        textarea {
                            value: "{import_text}",
                            oninput: move |e| import_text.set(e.value()),
                            placeholder: "Preamp: -7.2 dB\nFilter 1: ON PK Fc 200 Hz Gain -3.0 dB Q 2.000\n…",
                            rows: "4",
                            class: "eq__import-text",
                        }
                        if let Some(err) = import_error() {
                            p { class: "eq__error", "{err}" }
                        }
                        div { class: "eq__row",
                            button {
                                onclick: handle_import,
                                disabled: import_text().trim().is_empty() || import_name().trim().is_empty(),
                                class: "eq__btn",
                                "Save"
                            }
                            button {
                                onclick: move |_| {
                                    editing.set(None);
                                    show_import.set(false);
                                    import_error.set(None);
                                },
                                class: "{btn}",
                                "Cancel"
                            }
                        }
                        p { class: "eq__note",
                            "Paste an AutoEQ/PEACE-style parametric EQ export made for your earphone or headphone (or edit the one shown). It is saved under that name so you can switch between them later; it stacks under the tone EQ above and doesn't change per song."
                        }
                    }
                } else if headphones.is_empty() {
                    p { class: "eq__note",
                        "None saved - add the correction for your earphone or headphone and name it. Unlike the tone EQ above, it is fixed for that device, not something that changes per song."
                    }
                } else {
                    div { class: "eq__saved",
                        for h in headphones.iter() {
                            {
                                let id = h.id;
                                let is_active = active == Some(id);
                                let bands = crate::ui::player::parse_parametric_eq(&h.preset).bands.len();
                                rsx! {
                                    div { key: "{id}", class: "eq__hp",
                                        button {
                                            onclick: move |_| player.select_headphone(if is_active { None } else { Some(id) }),
                                            title: if is_active { "In use - click to stop using it" } else { "Use this correction" },
                                            "aria-pressed": "{is_active}",
                                            class: "eq__hp-pick",
                                            span { class: "eq__hp-name", "{h.name}" }
                                            span { class: "eq__hp-meta", "{bands} bands" }
                                        }
                                        button {
                                            onclick: {
                                                let (name, preset) = (h.name.clone(), h.preset.clone());
                                                move |_| {
                                                    editing.set(Some(id));
                                                    import_name.set(name.clone());
                                                    import_text.set(preset.clone());
                                                    import_error.set(None);
                                                    show_import.set(true);
                                                }
                                            },
                                            title: "Edit the name or filters",
                                            "aria-label": "Edit {h.name}",
                                            class: "{btn} btn--icon",
                                            EditIcon { size: 14 }
                                        }
                                        button {
                                            onclick: move |_| player.delete_headphone(id),
                                            title: "Delete this saved correction",
                                            "aria-label": "Delete {h.name}",
                                            class: "{btn} btn--icon",
                                            TrashIcon { size: 14 }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(profile) = correction {
                        label { class: "eq__toggle",
                            input {
                                r#type: "checkbox",
                                checked: correction_enabled,
                                onchange: move |e| player.set_correction_enabled(e.checked()),
                            }
                            "Apply it (preamp {profile.preamp_db:.1} dB)"
                        }
                    }
                }
            }
    };

    rsx! {
        div { class: "eq",
            div { class: "eq__head",
                p { class: "section-label", "Equalizer" }
                div { class: "eq__actions",
                    button {
                        onclick: move |_| player.reset_eq_to_suggested(),
                        title: if analyzed && features.audio { "Copy this track's suggested EQ back onto the sliders" } else { "Flatten back to the default bands" },
                        class: "{btn}",
                        if analyzed && features.audio { "Use suggested" } else { "Reset" }
                    }
                    button { onclick: move |_| on_close.call(()), class: "{btn} btn--icon", title: "Close", "aria-label": "Close", CloseIcon { size: 16 } }
                }
            }

            // With Audio analysis off the Headphone correction is the main thing here, so it comes first.
            if !features.audio {
                {correction_section.clone()}
            }

            if features.audio {
            div { class: "eq__strength",
                span { class: "eq__strength-label", "Strength" }
                input {
                    r#type: "range",
                    min: "0",
                    max: "100",
                    step: "5",
                    value: "{strength_pct}",
                    style: "--pct: {strength_pct}%",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<u8>() {
                            player.set_eq_strength(v, false);
                        }
                    },
                    onchange: move |e| {
                        if let Ok(v) = e.value().parse::<u8>() {
                            player.set_eq_strength(v, true);
                        }
                    },
                    title: "How much of each song's suggested EQ to apply",
                    class: "dev-range",
                }
                span { class: "eq__strength-value", "{strength_pct}%" }
            }
            }

            div { class: "eq__bands",
                PreampSlider { value: preamp, on_change: move |db| player.set_preamp_gain(db) }
                for (i, band) in bands.iter().enumerate() {
                    EqBandSlider {
                        key: "{i}",
                        freq: band.freq,
                        gain: band.gain,
                        on_freq_change: move |hz| player.set_band_freq(i, hz),
                        on_gain_change: move |db| player.set_band_gain(i, db),
                        on_remove: move |_| player.remove_band(i),
                    }
                }
                button {
                    onclick: move |_| player.add_band(),
                    title: "Add a band",
                    class: "eq__add",
                    "+ Band"
                }
            }

            p { class: "eq__note eq__note--spaced",
                if !features.audio {
                    "Shape it by ear: drag gains, retype frequencies, add or remove bands. It stays across tracks until you reset it."
                } else if analyzed {
                    "Seeded from this track's suggested EQ - drag a gain, retype a frequency, or add/remove bands to fine-tune. Sticks across tracks until you hit “Use suggested” again."
                } else {
                    "Not analyzed yet, so nothing to suggest - shape it by ear: drag gains, retype frequencies, add/remove bands."
                }
            }

            if features.audio {
                {correction_section.clone()}
            }
        }
    }
}
