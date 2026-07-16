use std::path::PathBuf;
use thiserror::Error;

/// Errors surfaced by the STT engine. Boundaries (model files, audio format,
/// native recognizer construction, decode result) are validated explicitly and
/// never swallowed.
#[derive(Debug, Error)]
pub enum SttError {
    #[error("model file not found: {0} (run scripts/fetch-model.sh)")]
    ModelNotFound(PathBuf),

    #[error("failed to read wav: {0}")]
    WavRead(String),

    #[error("unsupported audio format: got {got_sr} Hz, {got_channels} channel(s); expected 16000 Hz mono")]
    BadAudioFormat { got_sr: u32, got_channels: u16 },

    #[error("failed to create recognizer (provider={provider}): {detail}")]
    RecognizerCreate { provider: String, detail: String },

    #[error("decode produced no result")]
    NoResult,
}
