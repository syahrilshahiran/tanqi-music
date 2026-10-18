//! Decoding an in-memory audio file to mono samples.

use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;


/// Receives every decoded packet as interleaved samples with the channel count and sample rate, so the
/// per-channel measurements (loudness, spectrum) can run while the file decodes without a second copy.
pub(super) type Sink<'a> = &'a mut dyn FnMut(&[f32], usize, u32);

/// What decoding leaves behind besides what the sink measured.
pub(super) struct Decoded {
    /// The mono mix of the part of the track tempo and key are read from (see `Excerpt`), not the whole track.
    pub excerpt: Vec<f32>,
    /// Frames decoded in total, so a too-short file can still be told apart.
    pub frames: usize,
    pub sample_rate: u32,
}

/// Keeps only the middle `max_seconds` of the mono mix when the file says how long it is: a half-hour file
/// then costs a minute and a half of samples instead of the whole thing. When the length is not known up
/// front everything is kept (tempo cuts its own excerpt later), as before.
struct Excerpt {
    window: Option<(usize, usize)>,
    pos: usize,
    out: Vec<f32>,
}

impl Excerpt {
    fn new(sample_rate: u32, total_frames: Option<u64>, max_seconds: f32) -> Self {
        let max = (sample_rate as f32 * max_seconds) as usize;
        let window = total_frames
            .map(|t| t as usize)
            .filter(|&t| t > max)
            .map(|t| {
                let start = (t - max) / 2;
                (start, start + max)
            });
        Excerpt { window, pos: 0, out: Vec::new() }
    }

    fn push(&mut self, mono: &[f32]) {
        match self.window {
            None => self.out.extend_from_slice(mono),
            Some((from, to)) => {
                let (lo, hi) = (self.pos, self.pos + mono.len());
                if hi > from && lo < to {
                    let a = from.saturating_sub(lo);
                    let b = (to - lo).min(mono.len());
                    self.out.extend_from_slice(&mono[a..b]);
                }
            }
        }
        self.pos += mono.len();
    }
}

/// Decodes any symphonia-supported container/codec from `source` (a file or an in-memory buffer), handing
/// each packet's interleaved samples to `sink` on the way, and keeps the mono mix of the middle
/// `excerpt_seconds` for tempo detection.
pub(super) fn decode_source(source: Box<dyn MediaSource>, excerpt_seconds: f32, sink: Sink) -> Result<Decoded, String> {
    let mss = MediaSourceStream::new(source, Default::default());

    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("unrecognized audio format: {e}"))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .cloned()
        .ok_or("no decodable audio track found")?;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("audio track has no sample rate")?;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("failed to create decoder: {e}"))?;

    let mut excerpt = Excerpt::new(sample_rate, track.codec_params.n_frames, excerpt_seconds);
    let mut frames = 0usize;
    let mut mono = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(format!("error reading audio packet: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                mono.clear();
                append_mono_samples(&decoded, &mut mono, sink, sample_rate);
                frames += mono.len();
                excerpt.push(&mono);
            }
            // A single malformed packet shouldn't sink the whole analysis.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("error decoding audio packet: {e}")),
        }
    }

    Ok(Decoded { excerpt: excerpt.out, frames, sample_rate })
}

/// Downmixes a decoded packet to mono by averaging channels - bass/treble
/// balance is what this feature cares about, not stereo image.
fn append_mono_samples(decoded: &AudioBufferRef, out: &mut Vec<f32>, sink: &mut dyn FnMut(&[f32], usize, u32), sample_rate: u32) {
    let spec = *decoded.spec();
    let channels = spec.channels.count().max(1);
    let mut sample_buf = SampleBuffer::<f32>::new(decoded.frames() as u64, spec);
    sample_buf.copy_interleaved_ref(decoded.clone());
    sink(sample_buf.samples(), channels, sample_rate);

    out.extend(
        sample_buf
            .samples()
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32),
    );
}
