//! Concatenates assets/css/*.css (sorted; 00-base.css first) into one
//! stylesheet in OUT_DIR, inlining the two fonts as data URIs so the page
//! needs no asset pipeline.
//!
//! It also derives the silver (light) skin from the dark device styles, so the
//! two can't drift apart: every rule that hardcodes a colour is copied under
//! `:root.silver` with its greys inverted onto a silver scale. Hand-tuning
//! lives in `assets/css/silver/*.css`, appended after the derived rules.
use base64::Engine;
use regex::{Captures, Regex};
use std::{fs, path::Path};

fn data_uri(path: &str) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    format!("data:font/woff2;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Selectors that must stay as they are in the silver skin: the black
/// OLED-style displays.
const KEEP_DARK: &[&str] = &[".dev-display", ".player-bar__meter", ".dev-chip"];

/// Top-level `selector { body }` blocks. At-rules (`@media`, `@keyframes`,
/// `@font-face`) are skipped whole, nested blocks included.
fn top_level_rules(css: &str) -> Vec<(String, String)> {
    let comment = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let css = comment.replace_all(css, "");
    let mut rules = Vec::new();
    let (mut depth, mut start, mut sel_start) = (0usize, 0usize, 0usize);
    let mut skip_until_close = false;
    for (i, ch) in css.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    let sel = css[sel_start..i].trim();
                    skip_until_close = sel.starts_with('@');
                    start = i + 1;
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if !skip_until_close {
                        rules.push((css[sel_start..start - 1].trim().to_string(), css[start..i].to_string()));
                    }
                    sel_start = i + 1;
                }
            }
            _ => {}
        }
    }
    rules
}

fn grey(v: f64) -> u8 {
    // Dark -> light, onto a silver range: black becomes near-white, white
    // becomes near-black ink (#1c).
    // Steep through the dark half so borders and surfaces stay distinct,
    // then gentler up to the ink.
    let out = if v < 96.0 { 247.0 - v } else { 151.0 - (v - 96.0) * 0.774 };
    out.round().clamp(0.0, 255.0) as u8
}

fn map_hex(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    let full: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_string() };
    let n = |i: usize| u8::from_str_radix(&full[i..i + 2], 16).unwrap() as f64;
    let (r, g, b) = (n(0), n(2), n(4));
    // Cool-tinted greys stay cool; colours (the reds) keep their hue and just
    // darken a little so they read on a light ground.
    if (r - g).abs() < 14.0 && (g - b).abs() < 14.0 {
        let lum = (r + g + b) / 3.0;
        let v = grey(lum);
        format!("#{:02x}{:02x}{:02x}", v, v, v.saturating_add(2))
    } else {
        format!("#{:02x}{:02x}{:02x}", (r * 0.85) as u8, (g * 0.85) as u8, (b * 0.85) as u8)
    }
}

fn lightness(hex: &str) -> u32 {
    let h = hex.trim_start_matches('#');
    u32::from_str_radix(&h[0..2], 16).unwrap_or(0)
}

fn silver_color_decls(body: &str) -> String {
    let hex = Regex::new(r"#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b").unwrap();
    let rgba = Regex::new(r"rgba\((\d+),\s*(\d+),\s*(\d+),\s*([\d.]+)\)").unwrap();
    let gradient = Regex::new(r"linear-gradient\(180deg,\s*(#[0-9a-f]{6}),\s*(#[0-9a-f]{6})\)").unwrap();
    let mut out = String::new();
    for decl in body.split(';') {
        if !(hex.is_match(decl) || rgba.is_match(decl)) {
            continue;
        }
        let d = rgba.replace_all(decl, |c: &Captures| {
            let a: f64 = c[4].parse().unwrap_or(0.5);
            if &c[1] == "255" {
                // A light highlight on dark becomes a bright sheen on metal.
                format!("rgba(255,255,255,{:.2})", (a * 8.0).min(0.9))
            } else {
                // A heavy dark shadow becomes a soft one.
                format!("rgba(0,0,0,{:.2})", a * 0.3)
            }
        });
        let d = hex.replace_all(&d, |c: &Captures| map_hex(&c[0].to_lowercase()));
        // Light edge on top, so a swapped (inverted) gradient keeps its sheen.
        let d = gradient.replace_all(&d, |c: &Captures| {
            let (a, b) = (c[1].to_string(), c[2].to_string());
            if lightness(&a) < lightness(&b) {
                format!("linear-gradient(180deg, {b}, {a})")
            } else {
                c[0].to_string()
            }
        });
        out.push_str(d.trim());
        out.push(';');
    }
    out
}

fn derive_silver(css: &str) -> String {
    let mut out = String::from("/* ---- silver skin (derived from the dark device styles by build.rs) ---- */\n");
    for (selector, body) in top_level_rules(css) {
        let parts: Vec<String> = selector
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !KEEP_DARK.iter().any(|k| s.contains(k)))
            .map(|s| match s.strip_prefix(":root.dark") {
                Some(rest) => format!(":root.silver{rest}"),
                None => format!(":root.silver {s}"),
            })
            .collect();
        // Only rules that are dark-specific or hardcode their own dark colours.
        let decls = silver_color_decls(&body);
        if parts.is_empty() || decls.is_empty() || selector.starts_with(':') && !selector.starts_with(":root.dark") {
            continue;
        }
        out.push_str(&format!("{} {{ {} }}\n", parts.join(", "), decls));
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=assets/css");
    println!("cargo:rerun-if-changed=assets/fonts");
    let mut files: Vec<_> = fs::read_dir("assets/css")
        .expect("assets/css")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "css"))
        .collect();
    files.sort();
    let mut css = String::new();
    for f in &files {
        css.push_str(&format!("/* {} */\n", f.display()));
        css.push_str(&fs::read_to_string(f).unwrap());
        css.push('\n');
    }
    // The derived skin comes after every dark rule, the hand-tuned silver
    // files after that.
    let mut full = css.clone();
    full.push_str(&derive_silver(&css));
    let mut tweaks: Vec<_> = fs::read_dir("assets/css/silver")
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "css")).collect())
        .unwrap_or_default();
    tweaks.sort();
    for f in tweaks {
        full.push_str(&format!("/* {} */\n", f.display()));
        full.push_str(&fs::read_to_string(&f).unwrap());
        full.push('\n');
    }
    let full = full.replace("__JB__", &data_uri("assets/fonts/jetbrains-mono-latin.woff2"));
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("main.css");
    fs::write(out, full).unwrap();
}
