use thiserror::Error;

/// Errors from the on-device engine and its model downloader. Every variant
/// is safe to log: none carries audio, transcript text, or key material.
#[derive(Debug, Error)]
pub enum LocalSttError {
    #[error("unknown local model {0:?}")]
    UnknownModel(String),

    /// No OS data directory, so there is nowhere to put the weights.
    #[error("no OS data directory found; cannot store local models")]
    NoDataDir,

    #[error("download failed ({file}): {detail}")]
    Http { file: String, detail: String },

    #[error("cannot write {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// The bytes on disk are not the bytes we pinned. Always deletes the
    /// offending file so a retry starts clean rather than resuming garbage.
    #[error("{file} failed its integrity check (expected {expected}, got {actual})")]
    Checksum {
        file: String,
        expected: String,
        actual: String,
    },

    #[error("the local model is not downloaded (looked in {dir})")]
    ModelMissing { dir: String },

    /// The user pressed Cancel. Partial `.part` files are deliberately kept
    /// so a later Download resumes instead of restarting.
    #[error("download cancelled")]
    Cancelled,

    #[error("could not initialize the local engine: {0}")]
    EngineInit(String),

    /// The binary was built without the `engine` feature.
    #[error("this build of Hark does not include the on-device engine")]
    EngineUnavailable,
}
