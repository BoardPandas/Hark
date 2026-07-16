use crate::error::SttError;
use std::path::PathBuf;

/// Decoding strategy. `Greedy` is reliable and hotword-free; `ModifiedBeamSearch`
/// is the only mode that supports decode-time hotword biasing on Parakeet TDT but
/// carries sherpa-onnx issue #3267 (~20% empty/hallucinated output).
#[derive(Debug, Clone)]
pub enum DecodingMethod {
    Greedy,
    ModifiedBeamSearch { max_active_paths: i32 },
}

impl DecodingMethod {
    /// The string sherpa-onnx expects in `OfflineRecognizerConfig::decoding_method`.
    pub fn as_sherpa_str(&self) -> &'static str {
        match self {
            DecodingMethod::Greedy => "greedy_search",
            DecodingMethod::ModifiedBeamSearch { .. } => "modified_beam_search",
        }
    }
}

/// Decode-time hotword biasing config. `modeling_unit`/`bpe_vocab` are required by
/// sherpa-onnx when biasing a BPE-tokenized model; whether the Parakeet release
/// ships a usable `bpe.vocab` is the Checkpoint 3 gate.
#[derive(Debug, Clone)]
pub struct HotwordConfig {
    pub file: PathBuf,
    pub score: f32,
    pub modeling_unit: Option<String>,
    pub bpe_vocab: Option<PathBuf>,
}

/// Everything needed to build a warm recognizer. Kept free of runtime I/O so the
/// eventual pipeline can construct it once and reuse `SttEngine` across decodes.
#[derive(Debug, Clone)]
pub struct SttConfig {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
    /// "cpu" | "coreml" | "directml" | "cuda" — a runtime string, not a Cargo feature.
    pub provider: String,
    pub decoding: DecodingMethod,
    pub hotwords: Option<HotwordConfig>,
    pub num_threads: i32,
}

/// Reject audio the recognizer cannot consume before we ever touch the native lib.
/// Pure so it is unit-testable without a model or hardware.
pub fn check_audio_format(got_sr: u32, got_channels: u16) -> Result<(), SttError> {
    if got_sr != 16_000 || got_channels != 1 {
        return Err(SttError::BadAudioFormat {
            got_sr,
            got_channels,
        });
    }
    Ok(())
}

/// One parsed line of a hotwords file: a phrase with an optional trailing boost
/// score (`PHRASE :SCORE`). Score, when present, is the last whitespace-delimited
/// token and begins with ':'.
#[derive(Debug, Clone, PartialEq)]
pub struct HotwordEntry {
    pub phrase: String,
    pub score: Option<f32>,
}

/// Parse a single hotwords-file line. Blank/whitespace-only lines return `None`.
pub fn parse_hotword_line(line: &str) -> Option<HotwordEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let mut score = None;
    if let Some(last) = tokens.last() {
        if let Some(rest) = last.strip_prefix(':') {
            if let Ok(parsed) = rest.parse::<f32>() {
                score = Some(parsed);
                tokens.pop();
            }
        }
    }
    if tokens.is_empty() {
        return None;
    }
    Some(HotwordEntry {
        phrase: tokens.join(" "),
        score,
    })
}

/// Serialize an entry back to its canonical line form. `parse_hotword_line` of the
/// result reproduces the same structured entry (round-trip stable).
pub fn serialize_hotword_entry(entry: &HotwordEntry) -> String {
    match entry.score {
        Some(s) => format!("{} :{}", entry.phrase, s),
        None => entry.phrase.clone(),
    }
}
