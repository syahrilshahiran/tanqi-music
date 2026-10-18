//! `cargo run --release --example notify_probe -- <variant>`: sends one desktop notification in a given way, to see which ones the desktop pops up.
use notify_rust::{Hint, Notification, Urgency};
fn main() {
    let v = std::env::args().nth(1).unwrap_or_default();
    let mut n = Notification::new();
    n.summary("Tanqi").body(&format!("variant {v}"));
    match v.as_str() {
        "appname" => { n.appname("tanqi-music"); }
        "icon" => { n.appname("tanqi-music").icon("audio-x-generic"); }
        "normal" => { n.appname("tanqi-music").icon("audio-x-generic").hint(Hint::Urgency(Urgency::Normal)).timeout(6000); }
        _ => {}
    }
    println!("{v}: {:?}", n.show().map(|_| "sent"));
}
