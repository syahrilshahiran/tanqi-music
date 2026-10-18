use crate::ui::components::{confirm_modal::ConfirmModal, dropdown::Dropdown, icons::{CheckIcon, TrashIcon}};
use crate::ui::util::format_bytes;
use crate::ui::settings::{save_settings, use_settings};
use crate::ui::api;
use crate::ui::player::use_player;
use app_lib::models::{clean_ai_url, AiProvider, AppSettings, DataPart, OPENAI_COMPATIBLE_PRESETS, DataSize, DEFAULT_ANALYSIS_GUIDANCE, DEFAULT_TRANSLATION_GUIDANCE, MAX_GUIDANCE_CHARS};
use dioxus::prelude::*;
use std::collections::HashSet;
use std::time::Duration;

#[derive(Clone, PartialEq)]
enum ConnectionTest {
    Idle,
    Loading,
    Success(String),
    Warning(String),
    Error(String),
}

/// What the Danger zone can delete, with how each is described (your music files are never among them).
const DELETE_CHOICES: [(DataPart, &str, &str); 6] = [
    (DataPart::Music, "Music library", "Your Library folders and every Track in the index. Add a folder again to rebuild."),
    (DataPart::AudioAnalyses, "Audio analyses", "Loudness, spectrum, tempo, key and the suggested EQ of every analysed Track."),
    (DataPart::LyricsData, "Lyrics data", "Saved lyrics analyses and translations."),
    (DataPart::ArtistPictures, "Artist pictures", "The pictures fetched for artists."),
    (DataPart::HeadphoneCorrections, "Headphone corrections", "Your saved, named headphone corrections."),
    (DataPart::FavoritesAndPlaylists, "Favorites and playlists", "Every heart and every playlist."),
];

#[component]
pub fn Settings() -> Element {
    let player = use_player();
    let mut settings = use_settings();
    let mut loading = use_signal(|| true);
    let mut saving = use_signal(|| false);
    let mut saved = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut connection_test = use_signal(|| ConnectionTest::Idle);
    let mut available_models = use_signal(Vec::<String>::new);
    let mut confirm_delete = use_signal(|| false);
    // The address field, edited freely and only saved (cleaned up) by its Save button.
    let mut url_draft = use_signal(|| None::<String>);
    // The model name and the API key being typed; the key is only ever held here until saved, never shown back.
    let mut model_draft = use_signal(|| None::<String>);
    let mut key_draft = use_signal(String::new);
    let mut key_message = use_signal(|| None::<String>);
    let mut key_saved = use_resource(move || async move { api::ai_key_saved(settings().ai_provider).await });
    let mut delete_status = use_signal(|| None::<String>);
    // The kinds of data ticked for deletion.
    let mut delete_parts = use_signal(HashSet::<DataPart>::new);
    let favorites = crate::ui::favorites::use_favorites();
    let playlists = crate::ui::playlists::use_playlists();
    let mut home = use_context::<crate::ui::HomeTick>().0;
    let mut data_size = use_signal(|| None::<DataSize>);
    // Bumped per save so an older "Saved." timer can't hide a newer one.
    let mut saved_gen = use_signal(|| 0u32);

    use_future(move || async move {
        data_size.set(Some(api::get_data_size().await));
    });

    use_future(move || async move {
        match api::get_settings().await {
            Ok(s) => settings.set(s),
            Err(e) => error.set(Some(e)),
        }
        loading.set(false);
    });

    let mut save = move |next: AppSettings| {
        settings.set(next.clone());
        saving.set(true);
        saved.set(false);
        error.set(None);
        spawn(async move {
            match save_settings(settings, next).await {
                Ok(()) => {
                    saved.set(true);
                    saved_gen += 1;
                    let gen = saved_gen();
                    spawn(async move {
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        if saved_gen() == gen {
                            saved.set(false);
                        }
                    });
                }
                Err(e) => error.set(Some(e)),
            }
            saving.set(false);
        });
    };

    let handle_test_connection = move |_| {
        connection_test.set(ConnectionTest::Loading);
        let AppSettings { ai_model: model, ai_provider: provider, .. } = settings();
        spawn(async move {
            match app_lib::with_analyzer(|a| async move { a.test_connection().await }).await {
                Ok(result) => {
                    let n = result.models.len();
                    available_models.set(result.models);
                    let plural = if n == 1 { "" } else { "s" };
                    connection_test.set(if result.model_found {
                        ConnectionTest::Success(match provider {
                            AiProvider::Ollama => format!("Connected - \"{model}\" is pulled ({n} model{plural} total)."),
                            _ => format!("Connected - \"{model}\" is available ({n} model{plural} listed)."),
                        })
                    } else {
                        ConnectionTest::Warning(match provider {
                            AiProvider::Ollama => format!("Connected, but \"{model}\" isn't pulled yet - run: ollama pull {model}"),
                            _ if n == 0 => "Connected. This provider didn't list its models, so the model name can't be checked - type it exactly.".to_string(),
                            _ => format!("Connected, but \"{model}\" isn't in the provider's list - pick one below or check the name."),
                        })
                    });
                }
                Err(e) => {
                    available_models.set(vec![]);
                    connection_test.set(ConnectionTest::Error(e));
                }
            }
        });
    };

    let mut handle_pick_model = move |model: String| {
        let s = settings();
        if !model.is_empty() && model != s.ai_model {
            model_draft.set(None);
            save(AppSettings { ai_model: model, ..s });
            connection_test.set(ConnectionTest::Idle);
        }
    };

    // A different provider starts from its own address and a model that suits it; the old ones don't carry over.
    let mut handle_pick_provider = move |id: String| {
        let Some(provider) = AiProvider::from_id(&id) else { return };
        let s = settings();
        if provider == s.ai_provider {
            return;
        }
        url_draft.set(None);
        model_draft.set(None);
        key_draft.set(String::new());
        key_message.set(None);
        connection_test.set(ConnectionTest::Idle);
        available_models.set(vec![]);
        save(AppSettings {
            ai_provider: provider,
            ai_url: provider.default_url().to_string(),
            ai_model: provider.default_model().to_string(),
            ..s
        });
        key_saved.restart();
    };

    let mut handle_pick_preset = move |name: String| {
        let Some(&(_, url, model)) = OPENAI_COMPATIBLE_PRESETS.iter().find(|p| p.0 == name) else { return };
        url_draft.set(None);
        model_draft.set(None);
        connection_test.set(ConnectionTest::Idle);
        available_models.set(vec![]);
        let s = settings();
        let ai_model = if model.is_empty() { s.ai_model.clone() } else { model.to_string() };
        save(AppSettings { ai_url: url.to_string(), ai_model, ..s });
    };

    let mut handle_save_key = move |key: String| {
        let provider = settings().ai_provider;
        key_draft.set(String::new());
        spawn(async move {
            key_message.set(Some(match api::set_ai_key(provider, key).await {
                Ok(m) => m,
                Err(e) => e,
            }));
            key_saved.restart();
        });
    };

    // The Danger zone's button opens a confirmation; this runs once it is accepted.
    let mut do_delete = move || {
        confirm_delete.set(false);
        let parts: Vec<DataPart> = DELETE_CHOICES.iter().map(|c| c.0).filter(|p| delete_parts.peek().contains(p)).collect();
        delete_parts.set(HashSet::new());
        spawn(async move {
            let result = api::delete_data(parts.clone()).await;
            if result.is_ok() {
                // What is on screen mirrors what was stored: refresh the parts that were deleted.
                if parts.contains(&DataPart::FavoritesAndPlaylists) {
                    favorites.clear_local();
                    playlists.reload().await;
                }
                if parts.contains(&DataPart::HeadphoneCorrections) {
                    player.forget_headphones();
                }
                if parts.contains(&DataPart::Music) || parts.contains(&DataPart::ArtistPictures) {
                    home += 1;
                }
            }
            delete_status.set(Some(result.map(|_| "Deleted.".to_string()).unwrap_or_else(|e| e)));
            data_size.set(Some(api::get_data_size().await));
        });
    };

    if loading() {
        return rsx! { p { class: "text-muted-sm", "Loading…" } };
    }

    let s = settings();
    let test = connection_test();
    let models = available_models();
    let model_select_value = if models.contains(&s.ai_model) { s.ai_model.clone() } else { String::new() };
    let provider = s.ai_provider;
    let key_is_saved = key_saved().unwrap_or(false);

    rsx! {
        div { class: "settings",
            h1 { class: "settings__title", "Settings" }

            if let Some(err) = error() {
                div { class: "alert-error settings__alert",
                    "{err}"
                }
            }

            div { class: "settings__sections",
                Toggle {
                    label: "Audio analysis and suggested EQ",
                    description: "Measures each song's loudness and tone, suggests an EQ and levels playback. Off: a plain player, with your headphone correction behind the EQ button.",
                    checked: s.audio_analysis_enabled,
                    on_change: move |v| {
                        save(AppSettings { audio_analysis_enabled: v, ..settings() });
                        if !v {
                            player.audio_analysis_switched_off();
                        }
                    },
                }

                Toggle {
                    label: "AI lyrics analysis",
                    description: "Lets an AI model (Ollama on your machine, or a provider you give a key to) write a deep dive of the lyrics and translate them. Off: no deep dive, translation or Lyrics page; the lyrics themselves still show and follow along.",
                    checked: s.lyrics_ai_enabled,
                    on_change: move |v| save(AppSettings { lyrics_ai_enabled: v, ..settings() }),
                }

                Toggle {
                    label: "Desktop notifications",
                    description: "Notify me when a background analysis job finishes.",
                    checked: s.notifications_enabled,
                    on_change: move |v| save(AppSettings { notifications_enabled: v, ..settings() }),
                }

                if s.audio_analysis_enabled {
                div {
                    label { class: "settings__label", "Suggested EQ strength" }
                    p { class: "settings__hint",
                        "How much of each song's suggested EQ to apply. Below 100% it eases every boost and cut, which is gentler on songs that already sound right."
                    }
                    div { class: "eq__strength",
                        input {
                            r#type: "range",
                            min: "0",
                            max: "100",
                            step: "5",
                            value: "{s.eq_strength_percent}",
                            style: "--pct: {s.eq_strength_percent}%",
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
                            class: "dev-range",
                        }
                        span { class: "eq__strength-value", "{s.eq_strength_percent}%" }
                    }
                }
                }

                if s.lyrics_ai_enabled {
                div {
                    label { class: "settings__label", "AI provider" }
                    p { class: "settings__hint",
                        "What reads the lyrics for the [AI] analysis and translation. Ollama runs on your own machine (or one on your network) and nothing leaves it. A hosted provider (Claude, OpenAI, Gemini or any OpenAI-compatible service) is reached with your own API key, and the lyrics of the songs you analyse are sent to it."
                    }
                    Dropdown {
                        class: "settings__provider",
                        value: provider.id().to_string(),
                        options: AiProvider::ALL.iter().map(|p| (p.id().to_string(), p.label().to_string())).collect::<Vec<_>>(),
                        on_change: move |id: String| handle_pick_provider(id),
                    }
                    if provider == AiProvider::OpenAi {
                        Dropdown {
                            class: "settings__model-pick",
                            // The service whose address is set now; none of them means a custom address.
                            value: OPENAI_COMPATIBLE_PRESETS.iter().find(|p| p.1 == s.ai_url).map(|p| p.0.to_string()).unwrap_or_default(),
                            placeholder: "Choose a service…".to_string(),
                            options: OPENAI_COMPATIBLE_PRESETS.iter().map(|p| (p.0.to_string(), p.0.to_string())).collect::<Vec<_>>(),
                            on_change: move |name: String| handle_pick_preset(name),
                        }
                    }
                }

                div {
                    label { class: "settings__label", if provider == AiProvider::Ollama { "Ollama address" } else { "API address" } }
                    p { class: "settings__hint",
                        match provider {
                            AiProvider::Ollama => rsx! {
                                "Where Ollama runs. Leave it as this machine, or point it at another one on your network, for example your PC from your phone: "
                                code { class: "settings__mono", "192.168.1.20" }
                                ". On that machine start Ollama so it accepts network connections ("
                                code { class: "settings__mono", "OLLAMA_HOST=0.0.0.0 ollama serve" }
                                ")."
                            },
                            AiProvider::OpenAi => rsx! {
                                "The service's API address, usually ending in "
                                code { class: "settings__mono", "/v1" }
                                " - requests go to "
                                code { class: "settings__mono", "<address>/chat/completions" }
                                "."
                            },
                            AiProvider::Anthropic => rsx! { "Anthropic's API address. Leave it unless you use a proxy." },
                        }
                    }
                    div { class: "settings__url-row",
                        input {
                            class: "settings__url",
                            r#type: "text",
                            inputmode: "url",
                            autocapitalize: "off",
                            autocomplete: "off",
                            spellcheck: "false",
                            placeholder: "{provider.default_url()}",
                            value: "{url_draft().unwrap_or_else(|| s.ai_url.clone())}",
                            oninput: move |e| url_draft.set(Some(e.value())),
                        }
                        button {
                            class: "settings__test",
                            disabled: url_draft().is_none(),
                            onclick: move |_| {
                                if let Some(raw) = url_draft() {
                                    let url = clean_ai_url(settings().ai_provider, &raw);
                                    url_draft.set(None);
                                    connection_test.set(ConnectionTest::Idle);
                                    available_models.set(vec![]);
                                    save(AppSettings { ai_url: url, ..settings() });
                                }
                            },
                            title: "Save the address",
                            "aria-label": "Save",
                            CheckIcon { size: 18 }
                        }
                    }
                }

                if provider.needs_key() {
                div {
                    label { class: "settings__label", "API key" }
                    p { class: "settings__hint",
                        "Kept in your system keychain, not in the app's settings, and sent only to the address above. It is never shown here again; paste a new one to replace it."
                    }
                    p { class: "settings__model settings__mono",
                        if key_is_saved { "A key is saved." } else { "No key saved." }
                    }
                    div { class: "settings__url-row",
                        input {
                            class: "settings__url",
                            r#type: "password",
                            autocomplete: "off",
                            spellcheck: "false",
                            placeholder: "Paste your API key",
                            value: "{key_draft}",
                            oninput: move |e| key_draft.set(e.value()),
                        }
                        button {
                            class: "settings__test",
                            disabled: key_draft().trim().is_empty(),
                            onclick: move |_| handle_save_key(key_draft()),
                            title: "Save the key",
                            "aria-label": "Save key",
                            CheckIcon { size: 18 }
                        }
                        if key_is_saved {
                            button {
                                class: "settings__test",
                                onclick: move |_| handle_save_key(String::new()),
                                title: "Remove the saved key",
                                "aria-label": "Remove key",
                                TrashIcon { size: 18 }
                            }
                        }
                    }
                    if let Some(m) = key_message() {
                        p { class: "settings__result", "{m}" }
                    }
                }
                }

                div {
                    label { class: "settings__label", "Model" }
                    p { class: "settings__hint",
                        match provider {
                            AiProvider::Ollama => rsx! {
                                "Model tag used for the [AI] lyrics analysis and translation, run against the Ollama above ("
                                code { class: "settings__mono", "ollama serve" }
                                "). Test the connection to pick from models you've already pulled."
                            },
                            _ => rsx! { "The model's id, as the provider names it. Test the connection to pick from the ones it lists." },
                        }
                    }
                    div { class: "settings__url-row",
                        input {
                            class: "settings__url",
                            r#type: "text",
                            autocapitalize: "off",
                            autocomplete: "off",
                            spellcheck: "false",
                            placeholder: "{provider.default_model()}",
                            value: "{model_draft().unwrap_or_else(|| s.ai_model.clone())}",
                            oninput: move |e| model_draft.set(Some(e.value())),
                        }
                        button {
                            class: "settings__test",
                            disabled: model_draft().is_none_or(|m| m.trim().is_empty()),
                            onclick: move |_| {
                                if let Some(m) = model_draft() {
                                    handle_pick_model(m.trim().to_string());
                                }
                            },
                            title: "Save the model",
                            "aria-label": "Save model",
                            CheckIcon { size: 18 }
                        }
                    }
                    button {
                        onclick: handle_test_connection,
                        disabled: test == ConnectionTest::Loading,
                        class: "settings__test",
                        if test == ConnectionTest::Loading { "Testing…" } else { "Test connection" }
                    }
                    match &test {
                        ConnectionTest::Success(m) => rsx! { p { class: "settings__result settings__result--ok", "{m}" } },
                        ConnectionTest::Warning(m) => rsx! { p { class: "settings__result settings__result--warn", "{m}" } },
                        ConnectionTest::Error(m) => rsx! { p { class: "settings__result settings__result--err", "{m}" } },
                        _ => rsx! {},
                    }
                    if !models.is_empty() {
                        Dropdown {
                            class: "settings__model-pick",
                            value: model_select_value.clone(),
                            placeholder: "Select a model…".to_string(),
                            options: models.iter().map(|m| (m.clone(), m.clone())).collect::<Vec<_>>(),
                            on_change: move |m: String| handle_pick_model(m),
                        }
                    }
                }

                PromptEditor {
                    label: "Analysis prompt",
                    hint: "Sets the role and style of the [AI] deep dive. The reply format, the field names and how lines are numbered are fixed by the app and can't be changed here.",
                    value: s.analysis_guidance.clone(),
                    default_text: DEFAULT_ANALYSIS_GUIDANCE,
                    on_save: move |text: String| save(AppSettings { analysis_guidance: text, ..settings() }),
                }

                PromptEditor {
                    label: "Translation prompt",
                    hint: "Sets how the lyrics are translated, for example the tone or how to treat slang. The reply format and line numbering are fixed by the app.",
                    value: s.translation_guidance.clone(),
                    default_text: DEFAULT_TRANSLATION_GUIDANCE,
                    on_save: move |text: String| save(AppSettings { translation_guidance: text, ..settings() }),
                }
                }

                p { class: "settings__status",
                    if saving() { "Saving…" } else if saved() { "Saved." }
                }

                div { class: "danger-zone",
                    p { class: "danger-zone__title", "Danger zone" }
                    if let Some(size) = data_size() {
                        div { class: "danger-zone__row danger-zone__row--first",
                            div { class: "danger-zone__text",
                                p { class: "danger-zone__label", "Library data" }
                                p { class: "danger-zone__hint",
                                    "Size on disk: {format_bytes(size.database_bytes + size.covers_bytes)} · database {format_bytes(size.database_bytes)} · covers {format_bytes(size.covers_bytes)}"
                                }
                            }
                        }
                        div { class: "danger-zone__row",
                            div { class: "danger-zone__text",
                                p { class: "danger-zone__label", "Remembered lookups: {size.cache_entries}" }
                                p { class: "danger-zone__hint",
                                    "Track info and lyrics are kept for 1 to 12 weeks so they don't need the internet again. Clearing them only means looking them up again."
                                }
                            }
                            button {
                                class: "danger-zone__btn",
                                disabled: size.cache_entries == 0,
                                onclick: move |_| {
                                    spawn(async move {
                                        let _ = api::clear_api_cache().await;
                                        data_size.set(Some(api::get_data_size().await));
                                    });
                                },
                                TrashIcon { size: 16 }
                                "Clear lookups"
                            }
                        }
                    }
                    div { class: "danger-zone__row danger-zone__row--stack",
                        div { class: "danger-zone__text",
                            p { class: "danger-zone__label", "Delete data" }
                            p { class: "danger-zone__hint",
                                "Tick what to delete. Your music files are never touched."
                            }
                        }
                        div { class: "danger-zone__choices",
                            for (part, name, hint) in DELETE_CHOICES {
                                label { key: "{name}", class: "danger-zone__choice",
                                    input {
                                        r#type: "checkbox",
                                        checked: delete_parts().contains(&part),
                                        onchange: move |e| {
                                            let mut next = delete_parts.peek().clone();
                                            if e.checked() {
                                                next.insert(part);
                                            } else {
                                                next.remove(&part);
                                            }
                                            delete_parts.set(next);
                                        },
                                    }
                                    span {
                                        span { class: "danger-zone__choice-name", "{name}" }
                                        span { class: "danger-zone__hint danger-zone__choice-hint", "{hint}" }
                                    }
                                }
                            }
                        }
                        if let Some(message) = delete_status() {
                            p { class: "danger-zone__hint", "{message}" }
                        }
                        button {
                            class: "danger-zone__btn",
                            disabled: delete_parts().is_empty(),
                            onclick: move |_| confirm_delete.set(true),
                            TrashIcon { size: 16 }
                            "Delete selected"
                        }
                    }
                }
            }
            if confirm_delete() {
                ConfirmModal {
                    title: "Delete the selected data?".to_string(),
                    lines: {
                        let chosen = delete_parts();
                        let names: Vec<&str> = DELETE_CHOICES.iter().filter(|c| chosen.contains(&c.0)).map(|c| c.1).collect();
                        vec![
                            format!("This deletes: {}. It can't be undone.", names.join(", ")),
                            "Your music files stay where they are, and anything you did not tick is kept.".to_string(),
                        ]
                    },
                    confirm_label: "Delete".to_string(),
                    danger: true,
                    on_cancel: move |_| confirm_delete.set(false),
                    on_confirm: move |_| do_delete(),
                }
            }
        }
    }
}

#[component]
fn Toggle(label: String, description: String, checked: bool, on_change: EventHandler<bool>) -> Element {
    let track = if checked { " toggle--on" } else { "" };
    rsx! {
        div { class: "toggle-row",
            div {
                p { class: "toggle-row__label", "{label}" }
                p { class: "toggle-row__desc", "{description}" }
            }
            button {
                role: "switch",
                "aria-checked": "{checked}",
                onclick: move |_| on_change.call(!checked),
                class: "toggle{track}",
                span { class: "toggle__knob" }
            }
        }
    }
}

/// One editable prompt: only the role/style text. Empty means the default.
/// Saving and resetting are explicit, so a half-typed prompt is never used.
#[component]
fn PromptEditor(
    label: &'static str,
    hint: &'static str,
    /// What is saved now ("" when the default is in use).
    value: String,
    default_text: &'static str,
    on_save: EventHandler<String>,
) -> Element {
    let shown = if value.trim().is_empty() { default_text.to_string() } else { value.clone() };
    let mut draft = use_signal(|| shown.clone());
    let using_default = value.trim().is_empty();
    let changed = draft().trim() != shown.trim();
    let count = draft().chars().count();
    rsx! {
        div {
            label { class: "settings__label", "{label}" }
            p { class: "settings__hint", "{hint}" }
            textarea {
                class: "settings__prompt",
                rows: "4",
                maxlength: "{MAX_GUIDANCE_CHARS}",
                value: "{draft}",
                oninput: move |e| draft.set(e.value()),
            }
            div { class: "settings__prompt-row",
                span { class: "settings__prompt-count", "{count}/{MAX_GUIDANCE_CHARS}" }
                if using_default {
                    span { class: "settings__prompt-count", "Default" }
                }
                button {
                    class: "settings__test",
                    disabled: !changed,
                    onclick: {
                        let default_text = default_text.to_string();
                        move |_| {
                            let t = draft().trim().to_string();
                            // Saving the default text is the same as using the default.
                            on_save.call(if t == default_text { String::new() } else { t });
                        }
                    },
                    CheckIcon { size: 14 }
                    "Save"
                }
                button {
                    class: "settings__test",
                    disabled: using_default && !changed,
                    onclick: move |_| {
                        draft.set(default_text.to_string());
                        on_save.call(String::new());
                    },
                    "Reset to default"
                }
            }
        }
    }
}
