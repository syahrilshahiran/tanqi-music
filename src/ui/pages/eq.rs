use crate::ui::{
    api,
    components::{
        analyzed_audio_card::AnalyzedAudioCard,
        dropdown::Dropdown,
        filter_pill::FilterPill,
        audio_analysis_modal::{track_from_audio_analysis, AudioAnalysisModal},
    },
    player::use_player,
    Route,
};
use crate::ui::components::icons::BackIcon;
use app_lib::audio::{tone, Tone};
use app_lib::models::{AudioAnalysis, TrackSummary};
use dioxus::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
enum SortBy {
    Recent,
    BpmAsc,
    BpmDesc,
}

/// The analyses as listed (filtered and sorted) and the Play queue they make; rebuilt only when what they
/// depend on changes. Compared by pointer, since a rebuild is exactly a change.
#[derive(Clone)]
struct EqView {
    visible: Arc<Vec<AudioAnalysis>>,
    queue: Arc<Vec<TrackSummary>>,
}

impl PartialEq for EqView {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.visible, &other.visible) && Arc::ptr_eq(&self.queue, &other.queue)
    }
}

/// Tracks per page.
const PAGE_SIZE: usize = 12;

/// The balance filters offered on the page, and what each is called.
/// `None` is a balanced track (one that leans no way).
const TONE_FILTERS: [(Option<Tone>, &str); 5] = [
    (None, "Balanced"),
    (Some(Tone::Bassy), "Bassy"),
    (Some(Tone::Bright), "Bright"),
    (Some(Tone::Dark), "Dark"),
    (Some(Tone::MidForward), "Mid"),
];

#[component]
pub fn Eq() -> Element {
    let mut analyzed = use_signal(Vec::<AudioAnalysis>::new);
    let mut search = use_signal(String::new);
    let mut sort_by = use_signal(|| SortBy::Recent);
    // Tones picked as filters; a track matches if it has any of them.
    let mut tones = use_signal(HashSet::<Option<Tone>>::new);
    // The page of the list being shown (0-based); back to the first whenever the list changes.
    let mut page = use_signal(|| 0usize);
    let mut selected = use_signal(|| None::<AudioAnalysis>);
    let player = use_player();
    // A clicked notification for one song's analysis: open it.
    let target = crate::ui::targets::use_open_target();
    use_effect(move || {
        if let Some(id) = target.track_for("eq") {
            spawn(async move {
                if let Ok(Some(a)) = api::get_audio_analysis(id).await {
                    selected.set(Some(a));
                }
                target.clear();
            });
        }
    });
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);

    use_future(move || async move {
        error.set(None);
        match api::list_analyzed_audio().await {
            Ok(list) => analyzed.set(list),
            Err(e) => error.set(Some(e)),
        }
        loading.set(false);
    });

    // Tone counts depend only on the analyses; the filtered, sorted list and its Play queue only on the
    // analyses, the search, the tone filters and the sort - not on paging or the open modal.
    let tone_counts = use_memo(move || {
        let all = analyzed.read();
        TONE_FILTERS
            .iter()
            .map(|&(t, label)| (t, label, all.iter().filter(|a| tone(a) == t).count()))
            .collect::<Vec<(Option<Tone>, &'static str, usize)>>()
    });
    let view = use_memo(move || {
        let needle = search().trim().to_lowercase();
        let picked = tones();
        let sort = sort_by();
        let mut visible: Vec<AudioAnalysis> = analyzed
            .read()
            .iter()
            .filter(|a| {
                needle.is_empty() || a.title.to_lowercase().contains(&needle) || a.artist.to_lowercase().contains(&needle)
            })
            .filter(|a| picked.is_empty() || picked.contains(&tone(a)))
            .cloned()
            .collect();
        // Tracks with no detected tempo sort to the end regardless of direction -
        // "unknown" isn't meaningfully slower or faster than a real BPM value.
        if sort != SortBy::Recent {
            visible.sort_by(|a, b| match (a.bpm, b.bpm) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(x), Some(y)) => {
                    let ord = x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
                    if sort == SortBy::BpmAsc { ord } else { ord.reverse() }
                }
            });
        }
        // What "play" queues: the songs as currently listed, so next/previous follow the list.
        let queue = Arc::new(visible.iter().map(track_from_audio_analysis).collect());
        EqView { visible: Arc::new(visible), queue }
    });

    let picked = tones();
    let sort = sort_by();
    let total = analyzed.read().len();
    let view = view.read();
    let visible = view.visible.clone();
    let queue = view.queue.clone();
    drop(view);
    let tone_counts = tone_counts.read().clone();

    let pages = visible.len().div_ceil(PAGE_SIZE).max(1);
    let current = page().min(pages - 1);
    let first = current * PAGE_SIZE;
    let shown_total = visible.len();

    rsx! {
        div {
            div { class: "eq-page__head",
                h1 { class: "eq-page__title", "EQ" }
            }
            p { class: "text-muted-sm eq-page__intro",
                "Every track you've run an audio EQ analysis on, in one place."
            }

            if let Some(e) = error() {
                div { class: "alert-error eq-page__error", "{e}" }
            }

            if loading() {
                p { class: "text-muted-sm", "Loading…" }
            } else if total == 0 {
                div { class: "eq-page__none",
                    h2 { class: "eq-page__none-title", "Nothing analyzed yet" }
                    p { class: "eq-page__none-text",
                        "Open a track's info panel, open Audio analysis and run it to get a suggested EQ - it'll show up here afterward."
                    }
                    Link {
                        to: Route::Library {},
                        class: "btn-solid",
                        "Go to Library"
                    }
                }
            } else {
                div { class: "eq-page__filters",
                    input {
                        value: "{search}",
                        oninput: move |e| {
                            search.set(e.value());
                            page.set(0);
                        },
                        placeholder: "Search analyzed tracks…",
                        class: "eq-page__field eq-page__search",
                    }
                    Dropdown {
                        class: "eq-page__sort",
                        value: match sort {
                            SortBy::Recent => "recent",
                            SortBy::BpmDesc => "bpmDesc",
                            SortBy::BpmAsc => "bpmAsc",
                        }
                        .to_string(),
                        options: vec![
                            ("recent".to_string(), "Most recent".to_string()),
                            ("bpmDesc".to_string(), "BPM: fast → slow".to_string()),
                            ("bpmAsc".to_string(), "BPM: slow → fast".to_string()),
                        ],
                        on_change: move |v: String| {
                            page.set(0);
                            sort_by.set(match v.as_str() {
                                "bpmAsc" => SortBy::BpmAsc,
                                "bpmDesc" => SortBy::BpmDesc,
                                _ => SortBy::Recent,
                            });
                        },
                    }
                }

                div { class: "eq-page__tones",
                    for (t, label, count) in tone_counts.iter().copied() {
                        FilterPill {
                            key: "{label}",
                            label: format!("{label} ({count})"),
                            active: picked.contains(&t),
                            on_click: move |_| {
                                let mut next = tones.peek().clone();
                                if !next.remove(&t) {
                                    next.insert(t);
                                }
                                tones.set(next);
                                page.set(0);
                            },
                        }
                    }
                }

                if visible.is_empty() {
                    p { class: "empty-state",
                        "No analyzed tracks match \"{search}\"."
                    }
                } else {
                    div { class: "eq-page__grid",
                        for a in visible.iter().skip(first).take(PAGE_SIZE) {
                            AnalyzedAudioCard {
                                key: "{a.track_id}",
                                analysis: a.clone(),
                                on_play: {
                                    let queue = queue.clone();
                                    move |a: AudioAnalysis| player.play_track(track_from_audio_analysis(&a), Some((*queue).clone()))
                                },
                                on_details: move |a| selected.set(Some(a)),
                            }
                        }
                    }
                    if pages > 1 {
                        div { class: "eq-page__pager",
                            button {
                                class: "dev-tag eq-page__pager-btn",
                                disabled: current == 0,
                                title: "Previous page",
                                "aria-label": "Previous page",
                                onclick: move |_| {
                                    page.set(current.saturating_sub(1));
                                    document::eval("document.querySelector('.eq-page__head')?.scrollIntoView()");
                                },
                                BackIcon { size: 14 }
                            }
                            span { class: "eq-page__pager-info",
                                "{first + 1}-{(first + PAGE_SIZE).min(shown_total)} of {shown_total}"
                            }
                            button {
                                class: "dev-tag eq-page__pager-btn eq-page__pager-btn--next",
                                disabled: current + 1 >= pages,
                                title: "Next page",
                                "aria-label": "Next page",
                                onclick: move |_| {
                                    page.set((current + 1).min(pages - 1));
                                    document::eval("document.querySelector('.eq-page__head')?.scrollIntoView()");
                                },
                                BackIcon { size: 14 }
                            }
                        }
                    }
                }
            }

            if let Some(sel) = selected() {
                AudioAnalysisModal {
                    track: track_from_audio_analysis(&sel),
                    initial_analysis: Some(sel.clone()),
                    local_file_path: None,
                    on_close: move |_| selected.set(None),
                }
            }
        }
    }
}
