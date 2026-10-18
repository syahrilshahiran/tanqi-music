//! Small helpers shared by pages and components: time and duration
//! formatting, and URLs for files served through the `tq` protocol.


/// Base of the in-process protocol served by `main.rs` (`tq` scheme).
pub fn tq_base() -> &'static str {
    if cfg!(target_os = "android") {
        "https://tq.localhost"
    } else if cfg!(windows) {
        "http://tq.localhost"
    } else {
        "tq://localhost"
    }
}

/// URL that streams a file from disk through the `tq` protocol (audio for
/// the player, local cover art for `<img>`). Only audio/image files and
/// Tanqi's own temp audio are served.
pub fn file_url(path: &str) -> String {
    format!("{}/file?path={}", tq_base(), urlencoding::encode(path))
}

/// A thumbnail the webview can load directly: a real http(s) URL as-is, a
/// `local:` path served through `file_url`.
pub fn resolve_thumb(url: Option<&str>) -> Option<String> {
    let url = url?;
    match app_lib::local_ref::decode(url) {
        Some(path) => Some(file_url(path)),
        None => Some(url.to_string()),
    }
}

pub fn time_ago(iso: &str) -> String {
    let Ok(then) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return iso.to_string();
    };
    let diff_min = ((chrono::Utc::now() - then.with_timezone(&chrono::Utc)).num_milliseconds() as f64 / 60000.0).round() as i64;
    if diff_min < 1 {
        return "just now".into();
    }
    if diff_min < 60 {
        return format!("{diff_min}m ago");
    }
    let hr = (diff_min as f64 / 60.0).round() as i64;
    if hr < 24 {
        return format!("{hr}h ago");
    }
    let day = (hr as f64 / 24.0).round() as i64;
    if day < 7 {
        return format!("{day}d ago");
    }
    let week = (day as f64 / 7.0).round() as i64;
    if week < 5 {
        return format!("{week}w ago");
    }
    let month = (day as f64 / 30.0).round() as i64;
    if month < 12 {
        return format!("{month}mo ago");
    }
    format!("{}y ago", (day as f64 / 365.0).round() as i64)
}

/// "m:ss" (or "h:mm:ss" past an hour) from milliseconds.
pub fn format_duration(ms: i64) -> String {
    format_seconds(ms as f64 / 1000.0)
}

/// Same, from seconds (player position / duration).
pub fn format_seconds(secs: f64) -> String {
    let total = secs.max(0.0).round() as i64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "1.4 MB", "820 KB", "0 B": a file size for people.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{size:.1} {}", UNITS[unit]) }
}
