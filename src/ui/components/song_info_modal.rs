use crate::ui::components::{
    artwork_modal::ArtworkModal,
    audio_analysis_modal::AudioAnalysisModal,
    icons::CloseIcon,
    lyrics_modal::{use_escape_to_close, LyricsModal},
    track_thumbnail::TrackThumbnail,
};
use crate::ui::util::format_duration;
use app_lib::models::TrackSummary;
use dioxus::prelude::*;

#[component]
pub fn SongInfoModal(
    track: TrackSummary,
    on_close: EventHandler<()>,
) -> Element {
    let mut show_lyrics = use_signal(|| false);
    let mut show_artwork = use_signal(|| false);
    let mut show_audio_analysis = use_signal(|| false);
    // Lyrics and Audio analysis are optional features (chosen in onboarding, changeable in Settings).
    let features = crate::ui::settings::use_features();

    use_escape_to_close(on_close);

    // Supplementary enrichment lookups - render nothing (not an error) if they
    // fail or find no confident match, never blocking the rest of the modal
    // which is already usable from the track's own tags alone.
    let mb_res = use_resource(use_reactive!(|(track)| async move {
        app_lib::with_metadata(move |m| async move { m.musicbrainz_track_info(&track.title, &track.artist).await })
            .await
            .ok()
            .flatten()
    }));
    let itunes_res = use_resource(use_reactive!(|(track)| async move {
        app_lib::with_metadata(move |m| async move { m.itunes_track_info(&track.title, &track.artist).await })
            .await
            .ok()
            .flatten()
    }));
    // Unlike the lookups above, a failure here is kept: "couldn't reach the
    // lyrics service" must not read as "this song has no lyrics".
    let lyrics_track = track.clone();
    let mut lyrics_res = use_resource(use_reactive!(|lyrics_track| async move {
        app_lib::with_metadata(move |m| async move { m.lyrics(&lyrics_track.title, &lyrics_track.artist, lyrics_track.duration_ms).await }).await
    }));
    let mb_info = mb_res.read().clone().flatten();
    let itunes_info = itunes_res.read().clone().flatten();
    let lyrics_state = lyrics_res.read().clone();
    let lyrics_info = lyrics_state.clone().and_then(|r| r.ok()).flatten();
    let lyrics_failed = matches!(lyrics_state, Some(Err(_)));
    let lyrics_pending = lyrics_state.is_none();

    let t_audio = track.clone();

    rsx! {
        div {
            class: "modal-scrim song-info",
            onclick: move |_| on_close.call(()),
            div {
                class: "song-info__panel",
                onclick: move |e| e.stop_propagation(),
                div { class: "song-info__header",
                    TrackThumbnail { url: track.thumbnail_url.clone(), class: "song-info__thumb" }
                    div { class: "song-info__heading",
                        p { class: "song-info__kicker", "Track info" }
                        p { class: "song-info__title", "{track.title}" }
                    }
                }

                div { class: "song-info__body",
                    div { class: "song-info__row",
                        span { class: "song-info__label song-info__label--fixed", "Artist" }
                        span { class: "song-info__value",
                            "{track.artist}"
                        }
                    }
                    if let Some(album) = track.album.clone() {
                        div { class: "song-info__row",
                            span { class: "song-info__label song-info__label--fixed", "Album" }
                            span { class: "song-info__value",
                                "{album}"
                            }
                        }
                    }
                    div { class: "song-info__row",
                        span { class: "song-info__label", "Duration" }
                        span { class: "song-info__value",
                            match track.duration_ms {
                                Some(ms) => format_duration(ms),
                                None => "unknown".to_string(),
                            }
                        }
                    }
                    if features.audio {
                    div { class: "song-info__row song-info__row--center",
                        span { class: "song-info__label", "Audio analysis" }
                        button {
                            onclick: move |_| show_audio_analysis.set(true),
                            class: "song-info__btn",
                            "View + analyze"
                        }
                    }
                    }
                    if let Some(date) = mb_info.as_ref().and_then(|m| m.first_release_date.clone()).filter(|d| !d.is_empty()) {
                        div { class: "song-info__row",
                            span { class: "song-info__label song-info__label--fixed", "First released" }
                            span { class: "song-info__value", "{date}" }
                        }
                    }
                    if let Some(mb) = mb_info.as_ref() {
                        if !mb.composers.is_empty() {
                            div { class: "song-info__row",
                                span { class: "song-info__label song-info__label--fixed", "Written by" }
                                span { class: "song-info__value", "{mb.composers.join(\", \")}" }
                            }
                        }
                        if !mb.tags.is_empty() {
                            div {
                                span { class: "song-info__label song-info__label--block", "Genres" }
                                div { class: "song-info__tags",
                                    for tag in mb.tags.iter() {
                                        span { key: "{tag}", class: "song-info__tag", "{tag}" }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(it) = itunes_info.as_ref() {
                        if it.genre.is_some() || it.release_date.is_some() || it.artwork_url.is_some() {
                            div { class: "song-info__row",
                                span { class: "song-info__label song-info__label--fixed", "iTunes" }
                                span { class: "song-info__value song-info__value--actions",
                                    "{[it.genre.clone(), it.release_date.clone()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(\" · \")}"
                                    if it.artwork_url.is_some() {
                                        button {
                                            onclick: move |_| show_artwork.set(true),
                                            class: "song-info__btn song-info__btn--tight",
                                            "Artwork"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if lyrics_pending {
                        p { class: "song-info__note", "Looking up lyrics…" }
                    }
                    if lyrics_failed {
                        div { class: "song-info__row song-info__row--center",
                            span { class: "song-info__label", "Lyrics" }
                            button {
                                onclick: move |_| lyrics_res.restart(),
                                class: "song-info__btn",
                                title: "The lyrics service didn't answer",
                                "Couldn't load - retry"
                            }
                        }
                    }
                    if matches!(lyrics_state, Some(Ok(None))) {
                        p { class: "song-info__note", "No lyrics on file - the music speaks for itself." }
                    }
                    if lyrics_info.as_ref().map(|l| l.instrumental).unwrap_or(false) {
                        p { class: "song-info__note", "Instrumental - no words here, just the music." }
                    }
                    if lyrics_info.as_ref().and_then(|l| l.lyrics.as_ref()).is_some_and(|l| !l.is_empty()) {
                        div { class: "song-info__row song-info__row--center",
                            span { class: "song-info__label", "Lyrics" }
                            button {
                                onclick: move |_| show_lyrics.set(true),
                                class: "song-info__btn",
                                if features.lyrics_ai { "View + [AI] deep dive" } else { "View lyrics" }
                            }
                        }
                    }
                }

                div { class: "song-info__footer",
                    button {
                        onclick: move |_| on_close.call(()),
                        class: "song-info__footer-btn btn--icon",
                        title: "Close",
                        "aria-label": "Close",
                        CloseIcon { size: 18 }
                    }
                }
            }

            if show_lyrics() {
                if let Some(text) = lyrics_info.as_ref().and_then(|l| l.lyrics.clone()).filter(|l| !l.is_empty()) {
                    LyricsModal { track: track.clone(), lyrics: text, on_close: move |_| show_lyrics.set(false) }
                }
            }

            if show_artwork() {
                if let Some(url) = itunes_info.as_ref().and_then(|i| i.artwork_url.clone()) {
                    ArtworkModal { image_url: url, title: track.title.clone(), on_close: move |_| show_artwork.set(false) }
                }
            }

            if show_audio_analysis() {
                AudioAnalysisModal {
                    track: t_audio.clone(),
                    initial_analysis: None,
                    local_file_path: None,
                    on_close: move |_| show_audio_analysis.set(false),
                }
            }
        }
    }
}
