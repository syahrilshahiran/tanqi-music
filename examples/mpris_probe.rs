//! Exercises the real MPRIS service (`mpris::start_with_sink`) against a
//! live session bus, so it can be verified with `playerctl` without
//! launching the whole app. Prints each control request it receives
//! as `CMD <json>`, so a test script can assert on them.
//!
//! Run: `cargo run --example mpris_probe`

use app_lib::mpris::{self, NowPlaying};

fn main() {
    let handle = mpris::start_with_sink(Box::new(|cmd| {
        println!("CMD {}", serde_json::to_string(&cmd).unwrap_or_default());
    }));

    handle.update(NowPlaying {
        has_track: true,
        title: "Sun Kiss".to_string(),
        artist: "TUIDE".to_string(),
        album: Some("TUNE & PLAY".to_string()),
        art_url: Some("https://example.com/cover.jpg".to_string()),
        duration_ms: Some(181_000.0),
        position_ms: 42_000.0,
        playing: true,
        can_go_next: true,
        // Off by default so the capability gating itself is observable:
        // `playerctl -p tanqi previous` should be *refused* with
        // "No player could handle this command", and only go through with
        // PREV_OK=1 set. That mirrors how the real app advertises
        // CanGoPrevious from the player's actual queue position.
        can_go_previous: std::env::var("PREV_OK").is_ok(),
        shuffle: true,
        loop_status: "Playlist".to_string(),
        volume: 0.5,
    });

    println!("READY");
    // Stay alive so the bus name persists while the probe is queried.
    std::thread::sleep(std::time::Duration::from_secs(120));
}
