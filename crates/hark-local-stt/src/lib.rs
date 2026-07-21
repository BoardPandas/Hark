//! Hark opt-in on-device STT: model download management plus a sherpa-onnx
//! Parakeet recognizer.
//!
//! Two halves, deliberately separable:
//!
//! - **Model management** (`model`, `download`) is always compiled. It knows
//!   which files a model is made of, whether a usable copy is on disk, and
//!   how to fetch one with resume, progress, cancel, and a pinned-sha256
//!   integrity check.
//! - **The engine** (`engine`) is behind the `engine` Cargo feature, because
//!   statically linking sherpa-onnx + ONNX Runtime costs ~28 MB of binary
//!   (measured 2026-07-21) that users who never enable local STT would
//!   otherwise pay. Without the feature, `LocalEngine::load` returns
//!   [`LocalSttError::EngineUnavailable`] and everything else still builds
//!   and tests.
//!
//! Weights are never bundled with the app; they are downloaded on demand
//! into `<data_dir>/models/<model-id>/`.

mod download;
mod engine;
mod error;
mod model;

pub use download::{download, remove, Progress};
pub use engine::{LocalEngine, LocalTranscript};
pub use error::LocalSttError;
pub use model::{find, ModelFile, ModelSpec, ModelStatus, CATALOG, PARAKEET_V3_INT8};

/// Resolve the model named in `[local_stt] model` together with the directory
/// it belongs in. The common first step for every caller.
pub fn resolve(model_id: &str) -> Result<(&'static ModelSpec, std::path::PathBuf), LocalSttError> {
    let spec = find(model_id)?;
    let dir = spec.dir()?;
    Ok((spec, dir))
}

/// A human-readable size for the UI ("670 MB", "1.2 GB").
pub fn format_bytes(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    const GB: f64 = 1_000_000_000.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_render_for_the_settings_card() {
        assert_eq!(format_bytes(670_478_772), "670 MB");
        assert_eq!(format_bytes(1_200_000_000), "1.2 GB");
        assert_eq!(format_bytes(0), "0 MB");
    }

    #[test]
    fn resolving_an_unknown_model_fails_before_touching_the_filesystem() {
        assert!(matches!(
            resolve("nope"),
            Err(LocalSttError::UnknownModel(_))
        ));
    }
}
