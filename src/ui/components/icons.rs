//! Small stroke icons on a 24px grid, one weight and round caps throughout,
//! used instead of text glyphs (which render at whatever weight each
//! platform's symbol font picks).

use dioxus::prelude::*;

#[component]
fn Svg(d: &'static str, size: Option<u32>, filled: Option<bool>) -> Element {
    let size = size.unwrap_or(14);
    let filled = filled.unwrap_or(false);
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: if filled { "currentColor" } else { "none" },
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            path { d: "{d}" }
        }
    }
}

#[component]
pub fn PlayIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M7 4.5v15a.5.5 0 00.77.42l12-7.5a.5.5 0 000-.84l-12-7.5A.5.5 0 007 4.5z", size, filled: true } }
}

#[component]
pub fn PauseIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M7.5 5h2v14h-2zM14.5 5h2v14h-2z", size, filled: true } }
}

#[component]
pub fn SkipBackIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M18.2 5.1L9 12l9.2 6.9a.5.5 0 00.8-.4v-13a.5.5 0 00-.8-.4zM6 5v14", size, filled: true } }
}

#[component]
pub fn SkipNextIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M5.8 5.1L15 12l-9.2 6.9a.5.5 0 01-.8-.4v-13a.5.5 0 01.8-.4zM18 5v14", size, filled: true } }
}

#[component]
pub fn StopIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M7 7h10v10H7z", size, filled: true } }
}

/// Three sliders: the EQ.
#[component]
pub fn SlidersIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6", size } }
}

#[component]
pub fn GearIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M15 12a3 3 0 11-6 0 3 3 0 016 0zM19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z", size } }
}

#[component]
pub fn BackIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M19 12H5M11 6l-6 6 6 6", size } }
}

#[component]
pub fn InfoIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M12 11v5M12 8h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z", size } }
}

#[component]
pub fn CloseIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M6 6l12 12M18 6L6 18", size } }
}

#[component]
pub fn MinimizeIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M5 18h14", size } }
}

#[component]
pub fn ExpandIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7", size } }
}

/// Heart; `filled` for a favorite.
#[component]
pub fn HeartIcon(size: Option<u32>, filled: Option<bool>) -> Element {
    let d = "M20.8 4.6a5.5 5.5 0 00-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 00-7.8 7.8l1 1.1L12 21.2l7.8-7.7 1-1.1a5.5 5.5 0 000-7.8z";
    rsx! { Svg { d, size, filled: filled.unwrap_or(false) } }
}

/// Three lines with a play mark: the queue.
#[component]
pub fn QueueIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M4 6h12M4 12h12M4 18h7M16 15v6l5-3z", size } }
}

#[component]
pub fn ChevronUpIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M6 15l6-6 6 6", size } }
}

#[component]
pub fn ChevronDownIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M6 9l6 6 6-6", size } }
}

#[component]
pub fn PlusIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M12 5v14M5 12h14", size } }
}

/// Two crossing arrows: shuffle.
#[component]
pub fn ShuffleIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M16 3h5v5M4 20L21 3M21 16v5h-5M15 15l6 6M4 4l5 5", size } }
}

/// Two arrows chasing each other: repeat.
#[component]
pub fn RepeatIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M17 1l4 4-4 4M3 11V9a4 4 0 014-4h14M7 23l-4-4 4-4M21 13v2a4 4 0 01-4 4H3", size } }
}

/// A speaker with sound waves: sound is on.
#[component]
pub fn VolumeIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M11 5L6 9H2v6h4l5 4V5zM19.07 4.93a10 10 0 010 14.14M15.54 8.46a5 5 0 010 7.07", size } }
}

/// A speaker crossed out: muted.
#[component]
pub fn MuteIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M11 5L6 9H2v6h4l5 4V5zM23 9l-6 6M17 9l6 6", size } }
}

/// A tray with a down arrow: download.
#[component]
pub fn DownloadIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4M7 10l5 5 5-5M12 15V3", size } }
}

/// A bin: delete / clear.
#[component]
pub fn TrashIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M3 6h18M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6M10 11v6M14 11v6", size } }
}

/// A folder: the Library folders.
#[component]
pub fn FolderIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z", size } }
}

/// Two circling arrows: rescan.
#[component]
pub fn RefreshIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M23 4v6h-6M1 20v-6h6M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15", size } }
}

/// A picture: cover art.
#[component]
pub fn ImageIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM8.5 10a1.5 1.5 0 100-3 1.5 1.5 0 000 3zM21 15l-5-5L5 21", size } }
}

/// A person: artist pictures.
#[component]
pub fn UserIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2M12 11a4 4 0 100-8 4 4 0 000 8z", size } }
}

/// A pulse line: audio analysis.
#[component]
pub fn PulseIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M22 12h-4l-3 9L9 3l-3 9H2", size } }
}

/// Headphones: the headphone correction.
#[component]
pub fn HeadphonesIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M3 18v-6a9 9 0 0118 0v6M21 19a2 2 0 01-2 2h-1a2 2 0 01-2-2v-3a2 2 0 012-2h3zM3 19a2 2 0 002 2h1a2 2 0 002-2v-3a2 2 0 00-2-2H3z", size } }
}

/// A pencil: edit.
#[component]
pub fn EditIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M12 20h9M16.5 3.5a2.1 2.1 0 013 3L7 19l-4 1 1-4z", size } }
}

/// A tick: save, confirm.
#[component]
pub fn CheckIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M20 6L9 17l-5-5", size } }
}

/// A music note: the Library.
#[component]
pub fn LibraryIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M9 18V5l12-2v13M9 18a3 3 0 11-6 0 3 3 0 016 0zM21 16a3 3 0 11-6 0 3 3 0 016 0z", size } }
}

/// A speech bubble: the lyrics.
#[component]
pub fn LyricsIcon(size: Option<u32>) -> Element {
    rsx! { Svg { d: "M21 15a2 2 0 01-2 2H8l-5 4V5a2 2 0 012-2h14a2 2 0 012 2zM8 9h8M8 13h5", size } }
}

/// A circle with its right half filled: contrast, for the appearance switch.
#[component]
pub fn ContrastIcon(size: Option<u32>) -> Element {
    let size = size.unwrap_or(14);
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            circle { cx: "12", cy: "12", r: "9" }
            path { d: "M12 3a9 9 0 010 18z", fill: "currentColor" }
        }
    }
}
