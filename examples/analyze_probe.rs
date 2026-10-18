//! `cargo run --release --example analyze_probe -- file.flac [bytes | <excerpt seconds>]`: runs the audio analysis on one file and
//! prints the result and the time. By default the file is read as it decodes (`analyze_file`); with `bytes` the
//! whole file is read into memory first (`analyze_band_levels`), for comparing the two.
fn main() {
    let path = std::env::args().nth(1).expect("usage: analyze_probe <audio file> [bytes]");
    let arg = std::env::args().nth(2);
    let whole = arg.as_deref() == Some("bytes");
    let seconds: Option<f32> = arg.as_deref().and_then(|a| a.parse().ok());
    let t = std::time::Instant::now();
    let levels = if whole {
        app_lib::audio::analyze_band_levels(std::fs::read(&path).expect("read file"))
    } else {
        match seconds {
            Some(s) => app_lib::audio::analyze_file_with_excerpt(&path, s),
            None => app_lib::audio::analyze_file(&path),
        }
    };
    let secs = t.elapsed().as_secs_f64();
    match levels {
        Ok(l) => println!(
            "{} bpm {:?} key {:?} loudness {:?} LUFS true peak {:?} dBTP preamp {:.1} dB, {} bands, in {secs:.2}s",
            if whole { "[bytes]" } else { "[file]" },
            l.bpm,
            l.key,
            l.loudness_lufs.map(|v| (v * 10.0).round() / 10.0),
            l.true_peak_db.map(|v| (v * 10.0).round() / 10.0),
            l.preamp_db,
            l.eq_bands.len()
        ),
        Err(e) => println!("error: {e} in {secs:.2}s"),
    }
}
