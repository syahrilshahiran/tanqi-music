use crate::ui::{
    jobs::{use_jobs, Job, JobStatus},
    targets::use_open_target,
};
use dioxus::prelude::*;

fn status_label(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "Queued",
        JobStatus::Running => "Running",
        JobStatus::Done => "Done",
        JobStatus::Failed => "Failed",
        JobStatus::Cancelled => "Cancelled",
    }
}

fn status_color(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Running => "job-row__status--running",
        JobStatus::Failed => "job-row__status--failed",
        JobStatus::Done => "job-row__status--done",
        _ => "",
    }
}

#[component]
fn JobRow(job: Job, on_open: EventHandler<()>) -> Element {
    let jobs = use_jobs();
    let open_target = use_open_target();
    let target = job.target.clone();
    let finished = job.status.is_finished();
    let pct = job.progress.map(|p| (p * 100.0).round() as i64);
    let running = job.status == JobStatus::Running;
    let color = status_color(job.status);
    let id = job.id.clone();
    let cancel_title = if running {
        "Stop after the current step - work already in flight can't be aborted"
    } else {
        "Remove from the queue"
    };

    rsx! {
        div {
            class: "job-row job-row--link",
            title: "Open the result",
            onclick: move |_| {
                open_target.open(target.clone());
                on_open.call(());
            },
            div { class: "job-row__main",
                if running {
                    span { class: "job-row__dot" }
                }
                span { class: "job-row__label", "{job.label}" }
                span { class: "job-row__status {color}",
                    "{status_label(job.status)}"
                    if let (Some(p), true) = (pct, running) {
                        " {p}%"
                    }
                }
                button {
                    onclick: move |e| {
                        // Dismissing or cancelling is not opening.
                        e.stop_propagation();
                        if finished {
                            jobs.dismiss(&id);
                        } else {
                            jobs.cancel(&id);
                        }
                    },
                    title: if finished { "Dismiss" } else { cancel_title },
                    class: "job-row__cancel",
                    if finished { "×" } else { "Cancel" }
                }
            }

            if let Some(detail) = &job.detail {
                p { class: "job-row__detail", "{detail}" }
            }
            if let Some(err) = &job.error {
                p { class: "job-row__error line-clamp-2", "{err}" }
            }
            if let (true, Some(p)) = (running, pct) {
                div { class: "job-row__bar",
                    div { class: "job-row__bar-fill", style: "width: {p}%" }
                }
            }
        }
    }
}

/// The queue's one piece of chrome: a collapsed chip while things are
/// running, expandable into the list with per-job cancel. Renders nothing
/// when there's nothing to report, which is most of the time.
#[component]
pub fn JobQueueIndicator() -> Element {
    let jobs = use_jobs();
    let mut expanded = use_signal(|| false);

    let list = jobs.jobs.read().clone();
    if list.is_empty() {
        return rsx! {};
    }
    let active_count = list.iter().filter(|j| !j.status.is_finished()).count();
    let running = list.iter().find(|j| j.status == JobStatus::Running).cloned();
    let failed = list.iter().filter(|j| j.status == JobStatus::Failed).count();
    let expanded_now = expanded();
    let summary = if active_count > 0 {
        format!("{active_count} in queue")
    } else if failed > 0 {
        format!("{failed} failed")
    } else {
        "All done".to_string()
    };
    let total = list.len();

    rsx! {
        div { class: "jobq",
            button {
                onclick: move |_| expanded.set(!expanded()),
                class: "jobq__head",
                if active_count > 0 || failed > 0 {
                    span { class: "jobq__dot" }
                } else {
                    span { class: "jobq__dot jobq__dot--done" }
                }
                span { class: "jobq__summary", "{summary}" }
                if let (Some(r), false) = (&running, expanded_now) {
                    span { class: "jobq__running-label", "{r.label}" }
                }
                span { class: "jobq__chevron",
                    if expanded_now { "▾" } else { "▸" }
                }
            }

            if expanded_now {
                div { class: "jobq__list",
                    for job in list.iter() {
                        JobRow { key: "{job.id}", job: job.clone(), on_open: move |_| expanded.set(false) }
                    }
                }
                div { class: "jobq__foot",
                    if active_count > 0 {
                        button {
                            onclick: move |_| jobs.cancel_all(),
                            class: "jobq__foot-btn jobq__foot-btn--danger",
                            "Cancel all"
                        }
                    }
                    if total > active_count {
                        button {
                            onclick: move |_| jobs.dismiss_finished(),
                            class: "jobq__foot-btn jobq__foot-btn--neutral",
                            "Clear finished"
                        }
                    }
                }
            }
        }
    }
}
