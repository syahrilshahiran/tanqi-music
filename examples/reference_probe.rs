//! `cargo run --release --example reference_probe -- <music folder>`: measures the Audio analysis spectrum of every
//! track under a folder (read-only) and prints the built-in reference curve and its normal spread, as Rust arrays
//! for `src/audio/eq.rs`.
//!
//! Each track's levels are expressed relative to its own anchor (the mean of the 100 Hz - 5 kHz bands), so only
//! the SHAPE is compared, not loudness. Per album (a folder holding audio) the median over its tracks is taken,
//! so a long album doesn't outweigh a short one; albums are then grouped by the top-level folder they sit in
//! (an artist or a series: thirty Death Note releases are one group) and the group takes the median of its
//! albums. The reference is the median over groups for each band. The normal range (`SPREAD_DB`) is how far
//! individual tracks stray from it: 1.5 x the robust standard deviation (1.4826 x the median absolute
//! deviation) of every track's deviation, each track weighted by 1 / the size of its group so no artist
//! dominates. Raw per-track levels are written to the file named by the second argument; if that file already
//! exists it is read instead of measuring again, so the statistics can be re-tuned in seconds.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const ANCHOR: std::ops::RangeInclusive<usize> = 6..=23; // 100 Hz .. 5 kHz bands

fn audio_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            audio_files(&p, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if matches!(ext.to_ascii_lowercase().as_str(), "flac" | "mp3" | "m4a" | "wav" | "aac") {
                out.push(p);
            }
        }
    }
}

fn median(v: &mut Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 0 { (v[m - 1] + v[m]) / 2.0 } else { v[m] }
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("usage: reference_probe <music folder>"));
    let raw_path = std::env::args().nth(2);
    let cached = raw_path.as_ref().filter(|p| Path::new(p).exists());
    let mut files = Vec::new();
    if cached.is_none() {
        audio_files(&root, &mut files);
    }
    files.sort();
    eprintln!("{} files", files.len());
    let results: Mutex<Vec<(PathBuf, Vec<Option<f32>>)>> = Mutex::new(Vec::new());
    let next = Mutex::new(0usize);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = {
                    let mut n = next.lock().unwrap();
                    let i = *n;
                    *n += 1;
                    i
                };
                let Some(path) = files.get(i) else { break };
                let Ok(bytes) = std::fs::read(path) else { continue };
                if let Ok(l) = app_lib::audio::analyze_band_levels(bytes) {
                    let a = l.spectrum_db;
                    let anchor: Vec<f32> = ANCHOR.filter_map(|b| a.get(b).copied().flatten()).collect();
                    if anchor.len() < 10 {
                        continue;
                    }
                    let mean = anchor.iter().sum::<f32>() / anchor.len() as f32;
                    let rel = a.iter().map(|v| v.map(|x| x - mean)).collect();
                    results.lock().unwrap().push((path.parent().unwrap().to_path_buf(), rel));
                }
                if i % 50 == 0 {
                    eprintln!("  {i}/{}", files.len());
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    if let Some(path) = cached {
        let rows: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        results = rows
            .iter()
            .map(|r| {
                let rel: Vec<Option<f32>> = serde_json::from_value(r["rel"].clone()).unwrap();
                (PathBuf::from(r["dir"].as_str().unwrap()), rel)
            })
            .collect();
    } else if let Some(out) = raw_path {
        let rows: Vec<_> = results.iter().map(|(d, r)| serde_json::json!({"dir": d.to_string_lossy(), "rel": r})).collect();
        std::fs::write(out, serde_json::to_string(&rows).unwrap()).unwrap();
    }
    let bands = results.first().map(|r| r.1.len()).unwrap_or(0);

    let mut albums: std::collections::BTreeMap<PathBuf, Vec<&Vec<Option<f32>>>> = Default::default();
    for (dir, rel) in &results {
        albums.entry(dir.clone()).or_default().push(rel);
    }
    eprintln!("{} tracks measured in {} albums", results.len(), albums.len());
    let album_medians: Vec<Vec<Option<f32>>> = albums
        .values()
        .map(|tracks| {
            (0..bands)
                .map(|b| {
                    let mut v: Vec<f32> = tracks.iter().filter_map(|t| t[b]).collect();
                    // A band only counts for an album if most of its tracks have it.
                    if v.len() * 2 > tracks.len() { Some(median(&mut v)) } else { None }
                })
                .collect()
        })
        .collect();

    // Group by the top-level folder under the root.
    let mut groups: std::collections::BTreeMap<String, Vec<&Vec<Option<f32>>>> = Default::default();
    for ((dir, _), am) in albums.iter().zip(&album_medians) {
        let top = dir.strip_prefix(&root).ok().and_then(|r| r.components().next()).map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
        groups.entry(top).or_default().push(am);
    }
    let album_medians: Vec<Vec<Option<f32>>> = groups
        .values()
        .map(|albums| {
            (0..bands)
                .map(|b| {
                    let mut v: Vec<f32> = albums.iter().filter_map(|a| a[b]).collect();
                    if v.is_empty() { None } else { Some(median(&mut v)) }
                })
                .collect()
        })
        .collect();
    eprintln!("{} groups", groups.len());
    for (name, albums) in &groups {
        println!("  group {name}: {} albums", albums.len());
    }

    let mut reference = vec![0.0f32; bands];
    let mut spread = vec![0.0f32; bands];
    let mut counts = vec![0usize; bands];
    for b in 0..bands {
        let mut v: Vec<f32> = album_medians.iter().filter_map(|a| a[b]).collect();
        counts[b] = v.len();
        if v.is_empty() {
            continue;
        }
        reference[b] = median(&mut v);
    }
    // Spread: weighted median of |track - reference|, each track weighted 1 / its group's track count.
    let group_of = |dir: &Path| -> String {
        dir.strip_prefix(&root).ok().and_then(|r| r.components().next()).map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default()
    };
    let mut group_size: std::collections::HashMap<String, usize> = Default::default();
    for (dir, _) in &results {
        *group_size.entry(group_of(dir)).or_default() += 1;
    }
    for b in 0..bands {
        let mut pts: Vec<(f32, f32)> = results
            .iter()
            .filter_map(|(dir, rel)| rel[b].map(|v| ((v - reference[b]).abs(), 1.0 / group_size[&group_of(dir)] as f32)))
            .collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let total: f32 = pts.iter().map(|p| p.1).sum();
        let mut acc = 0.0;
        for (d, w) in &pts {
            acc += w;
            if acc >= total / 2.0 {
                spread[b] = 1.5 * 1.4826 * d;
                break;
            }
        }
    }
    println!("groups per band: {counts:?}");
    println!("pub(super) const REFERENCE_DB: [f32; {bands}] = {:?};", reference.iter().map(|x| (x * 10.0).round() / 10.0).collect::<Vec<_>>());
    println!("pub(super) const SPREAD_DB: [f32; {bands}] = {:?};", spread.iter().map(|x| (x * 10.0).round() / 10.0).collect::<Vec<_>>());
}
