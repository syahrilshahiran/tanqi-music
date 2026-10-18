//! Headphone corrections: the parametric-EQ export format they are saved in, and what it parses to.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CorrectionFilterType {
    Peaking,
    LowShelf,
    HighShelf,
}

impl CorrectionFilterType {
    pub(super) fn js(self) -> &'static str {
        match self {
            Self::Peaking => "peaking",
            Self::LowShelf => "lowshelf",
            Self::HighShelf => "highshelf",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct CorrectionBand {
    pub kind: CorrectionFilterType,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CorrectionProfile {
    pub preamp_db: f32,
    pub bands: Vec<CorrectionBand>,
}

/// Parses an Equalizer APO / PEACE-style parametric EQ export (what AutoEQ
/// generates), e.g.
///   Preamp: -7.2 dB
///   Filter 1: ON PK Fc 200 Hz Gain -3.0 dB Q 2.000
/// OFF filters and unsupported types are skipped; nothing matching returns
/// an empty `bands` (callers treat that as "nothing to apply").
pub fn parse_parametric_eq(text: &str) -> CorrectionProfile {
    use regex::Regex;
    use std::sync::OnceLock;
    static PREAMP: OnceLock<Regex> = OnceLock::new();
    static FILTER: OnceLock<Regex> = OnceLock::new();
    let preamp_re = PREAMP.get_or_init(|| Regex::new(r"(?i)^\s*Preamp:\s*(-?[\d.]+)\s*dB").unwrap());
    let filter_re = FILTER.get_or_init(|| {
        Regex::new(r"(?i)^\s*Filter\s+\d+:\s*ON\s+(PK|LSC|HSC)\s+Fc\s+([\d.]+)\s*Hz\s+Gain\s+(-?[\d.]+)\s*dB\s+Q\s+([\d.]+)").unwrap()
    });

    let mut preamp_db = 0.0;
    let mut bands = Vec::new();
    for line in text.lines() {
        if let Some(c) = preamp_re.captures(line) {
            preamp_db = c[1].parse().unwrap_or(0.0);
            continue;
        }
        let Some(c) = filter_re.captures(line) else { continue };
        let kind = match c[1].to_uppercase().as_str() {
            "PK" => CorrectionFilterType::Peaking,
            "LSC" => CorrectionFilterType::LowShelf,
            "HSC" => CorrectionFilterType::HighShelf,
            _ => continue,
        };
        let (Ok(freq), Ok(gain), Ok(q)) = (c[2].parse(), c[3].parse(), c[4].parse()) else { continue };
        bands.push(CorrectionBand { kind, freq, gain, q });
    }
    CorrectionProfile { preamp_db, bands }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_preamp_and_filters_and_skips_off_ones() {
        let p = parse_parametric_eq(
            "Preamp: -7.2 dB\nFilter 1: ON PK Fc 200 Hz Gain -3.0 dB Q 2.000\nFilter 2: OFF PK Fc 1000 Hz Gain 2.0 dB Q 1.0\nFilter 3: ON LSC Fc 105 Hz Gain 4.5 dB Q 0.70\n",
        );
        assert_eq!(p.preamp_db, -7.2);
        assert_eq!(p.bands.len(), 2);
        assert_eq!(p.bands[0], CorrectionBand { kind: CorrectionFilterType::Peaking, freq: 200.0, gain: -3.0, q: 2.0 });
        assert_eq!(p.bands[1].kind, CorrectionFilterType::LowShelf);
    }

    #[test]
    fn nothing_matching_gives_no_bands() {
        assert!(parse_parametric_eq("hello").bands.is_empty());
    }
}
