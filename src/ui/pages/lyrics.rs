//! Every song that has a Lyrics analysis, filterable by the topics and
//! entities the local AI model pulled out of the lyrics.

use crate::ui::{
    api,
    components::{analyzed_track_card::AnalyzedTrackCard, filter_pill::FilterPill, lyrics_modal::LyricsModal},
    Route,
};
use app_lib::models::{LyricsAnalysis, TrackSummary};
use dioxus::prelude::*;
use std::collections::HashSet;

/// `LyricsAnalysis` snapshots only a few track fields; `LyricsModal` wants a
/// `TrackSummary`, and with an analysis already in hand it needs no more.
fn track_from_analysis(a: &LyricsAnalysis) -> TrackSummary {
    TrackSummary {
        id: a.track_id.clone(),
        title: a.title.clone(),
        artist: a.artist.clone(),
        album: None,
        duration_ms: None,
        thumbnail_url: a.thumbnail_url.clone(),
    }
}

#[derive(Clone, PartialEq)]
struct TagCount {
    key: String,
    label: String,
    count: usize,
}

/// Counts each distinct tag (case-insensitively), most common first.
fn aggregate_tags(items: &[LyricsAnalysis], field: impl Fn(&LyricsAnalysis) -> &Vec<String>) -> Vec<TagCount> {
    let mut out: Vec<TagCount> = Vec::new();
    for item in items {
        for raw in field(item) {
            let label = raw.trim();
            if label.is_empty() {
                continue;
            }
            let key = label.to_lowercase();
            match out.iter_mut().find(|t| t.key == key) {
                Some(t) => t.count += 1,
                None => out.push(TagCount { key, label: label.to_string(), count: 1 }),
            }
        }
    }
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));
    out
}

/// A song matches when it carries ANY selected tag.
fn matches_tags(a: &LyricsAnalysis, selected: &HashSet<String>) -> bool {
    a.topics.iter().chain(a.entities.iter()).any(|t| selected.contains(&t.trim().to_lowercase()))
}

#[component]
fn TagGroup(heading: &'static str, tags: Vec<TagCount>, selected: Signal<HashSet<String>>) -> Element {
    let mut selected = selected;
    if tags.is_empty() {
        return rsx! {};
    }
    let sel = selected();
    rsx! {
        div {
            h2 { class: "section-label tag-filters__heading", "{heading}" }
            div { class: "tag-filters__pills",
                for t in tags.iter() {
                    FilterPill {
                        key: "{t.key}",
                        label: format!("{} ({})", t.label, t.count),
                        active: sel.contains(&t.key),
                        on_click: {
                            let k = t.key.clone();
                            move |_| {
                                let mut next = selected.peek().clone();
                                if !next.remove(&k) {
                                    next.insert(k.clone());
                                }
                                selected.set(next);
                            }
                        },
                    }
                }
            }
        }
    }
}

#[component]
pub fn Lyrics() -> Element {
    let mut analyzed = use_signal(Vec::<LyricsAnalysis>::new);
    let mut search = use_signal(String::new);
    let mut selected_tags = use_signal(HashSet::<String>::new);
    let mut open = use_signal(|| None::<LyricsAnalysis>);
    // A clicked notification for one song's lyrics: open its analysis, if it has one.
    let target = crate::ui::targets::use_open_target();
    use_effect(move || {
        if let Some(id) = target.track_for("lyrics") {
            spawn(async move {
                if let Ok(Some(a)) = api::get_lyrics_analysis(id).await {
                    open.set(Some(a));
                }
                target.clear();
            });
        }
    });
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);

    use_future(move || async move {
        match api::list_analyzed_lyrics().await {
            Ok(list) => analyzed.set(list),
            Err(e) => error.set(Some(e)),
        }
        loading.set(false);
    });

    let topics = use_memo(move || aggregate_tags(&analyzed.read(), |a| &a.topics));
    let entities = use_memo(move || aggregate_tags(&analyzed.read(), |a| &a.entities));

    let search_val = search();
    let needle = search_val.trim().to_lowercase();
    let sel = selected_tags();
    let total = analyzed.read().len();
    let visible: Vec<LyricsAnalysis> = analyzed
        .read()
        .iter()
        .filter(|a| sel.is_empty() || matches_tags(a, &sel))
        .filter(|a| needle.is_empty() || a.title.to_lowercase().contains(&needle) || a.artist.to_lowercase().contains(&needle))
        .cloned()
        .collect();

    rsx! {
        div {
            div { class: "topics__head",
                h1 { class: "topics__title", "Lyrics" }
            }
            p { class: "text-muted-sm topics__intro",
                "Every track you've run an [AI] lyrics analysis on. Pick topics or entities to find tracks that mention any of them."
            }

            if let Some(e) = error() {
                div { class: "alert-error topics__alert", "{e}" }
            }

            if loading() {
                p { class: "text-muted-sm", "Loading…" }
            } else if total == 0 {
                div { class: "topics-empty",
                    h2 { class: "topics-empty__title", "Nothing analyzed yet" }
                    p { class: "topics-empty__text",
                        "Open a track's info panel and run the lyrics analysis. It will show up here, indexed by topic and entity."
                    }
                    Link { to: Route::Library {}, class: "btn-solid", "Go to Library" }
                }
            } else {
                input {
                    value: "{search_val}",
                    oninput: move |e| search.set(e.value()),
                    placeholder: "Search by title or artist…",
                    class: "topics__search",
                }
                div { class: "tag-filters",
                    TagGroup { heading: "Topics", tags: topics.read().clone(), selected: selected_tags }
                    TagGroup { heading: "Entities", tags: entities.read().clone(), selected: selected_tags }
                    if !sel.is_empty() {
                        button {
                            class: "tag-filters__clear",
                            onclick: move |_| selected_tags.set(HashSet::new()),
                            "Clear {sel.len()} selected"
                        }
                    }
                }
                if visible.is_empty() {
                    p { class: "empty-state", "No analyzed tracks match." }
                } else {
                    div { class: "topics__grid topics__grid--tight",
                        for a in visible.iter() {
                            AnalyzedTrackCard { key: "{a.track_id}", analysis: a.clone(), on_open: move |a| open.set(Some(a)) }
                        }
                    }
                }
            }

            if let Some(a) = open() {
                LyricsModal {
                    track: track_from_analysis(&a),
                    lyrics: a.lines.join("\n"),
                    initial_analysis: Some(a.clone()),
                    on_close: move |_| open.set(None),
                }
            }
        }
    }
}
