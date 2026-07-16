//! Minimal WAV plumbing for the 16 kHz mono PCM16 format Hark standardizes on.
//! The pipeline will encode straight from the ring buffer's f32 samples, so the
//! encoder takes `&[f32]`; the parser exists to validate fixtures and decode
//! them back to samples for encode-time measurement.

use crate::error::SttError;

pub const SAMPLE_RATE: u32 = 16_000;
pub const CHANNELS: u16 = 1;
pub const BITS_PER_SAMPLE: u16 = 16;

/// Encode f32 samples in [-1.0, 1.0] as a complete 16 kHz mono PCM16 WAV file.
pub fn encode_wav_16k_mono(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = SAMPLE_RATE * u32::from(CHANNELS) * u32::from(BITS_PER_SAMPLE) / 8;
    let block_align = CHANNELS * BITS_PER_SAMPLE / 8;

    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        out.extend_from_slice(&((clamped * 32767.0) as i16).to_le_bytes());
    }
    out
}

/// Parsed header facts + decoded samples for a PCM16 WAV.
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples: Vec<f32>,
}

impl WavInfo {
    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / (f64::from(self.sample_rate) * f64::from(self.channels))
    }
}

/// Parse a PCM16 WAV, walking chunks (TTS and DAW output often insert LIST or
/// fact chunks between `fmt ` and `data`). Rejects anything that is not the
/// 16 kHz mono PCM16 contract so provider results stay comparable.
pub fn parse_wav_16k_mono(bytes: &[u8]) -> Result<WavInfo, SttError> {
    let bad = |msg: &str| SttError::BadAudio(msg.to_string());
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("not a RIFF/WAVE file"));
    }

    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
    let mut data: Option<&[u8]> = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body_end = (pos + 8).saturating_add(size).min(bytes.len());
        let body = &bytes[pos + 8..body_end];
        match id {
            b"fmt " => {
                if body.len() < 16 {
                    return Err(bad("fmt chunk too short"));
                }
                fmt = Some((
                    u16::from_le_bytes(body[0..2].try_into().unwrap()),
                    u16::from_le_bytes(body[2..4].try_into().unwrap()),
                    u32::from_le_bytes(body[4..8].try_into().unwrap()),
                    u16::from_le_bytes(body[14..16].try_into().unwrap()),
                ));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        // Chunks are word-aligned: odd sizes are padded with one byte.
        pos += 8 + size + (size % 2);
    }

    let (format, channels, sample_rate, bits) = fmt.ok_or_else(|| bad("missing fmt chunk"))?;
    let data = data.ok_or_else(|| bad("missing data chunk"))?;
    if format != 1 || bits != BITS_PER_SAMPLE {
        return Err(bad(&format!(
            "expected PCM16, got format={format} bits={bits}"
        )));
    }
    if sample_rate != SAMPLE_RATE || channels != CHANNELS {
        return Err(bad(&format!(
            "expected {SAMPLE_RATE} Hz mono, got {sample_rate} Hz {channels} channel(s)"
        )));
    }

    let samples = data
        .chunks_exact(2)
        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
        .collect();
    Ok(WavInfo {
        sample_rate,
        channels,
        bits_per_sample: bits,
        samples,
    })
}
