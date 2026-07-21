//! The on-device recognizer: sherpa-onnx offline transducer over Parakeet.
//!
//! Only compiled with the `engine` feature. Without it, [`LocalEngine::load`]
//! is still callable and returns [`LocalSttError::EngineUnavailable`], so
//! every caller can be written once against the same signature.
//!
//! **Decoding is always `greedy_search`.** `modified_beam_search` with
//! hotwords is the open sherpa-onnx #3267 hallucination bug; we never need it
//! because `hark-dictionary`'s phonetic pass corrects local transcripts
//! exactly as it corrects cloud ones.

use crate::error::LocalSttError;
use crate::model::ModelSpec;
use std::path::Path;
use std::time::Instant;

/// One on-device transcription. Mirrors `hark_stt::Transcript` so the worker
/// can treat cloud and local results identically; `request_ms` here is
/// decode wall time rather than an HTTP round trip.
pub struct LocalTranscript {
    pub text: String,
    pub request_ms: u128,
}

/// A loaded recognizer. Construction is expensive (hundreds of MB read into
/// RAM); the pipeline builds one lazily on first use and keeps it resident.
///
/// Not `Send`: it is created on, and only used from, the pipeline worker
/// thread. That is also why the pipeline loads it lazily there rather than
/// building it during `run()` on the caller's thread.
pub struct LocalEngine {
    #[cfg(feature = "engine")]
    recognizer: sherpa_onnx::OfflineRecognizer,
    model_id: &'static str,
    load_ms: u128,
}

impl LocalEngine {
    /// Load `spec`'s weights from `dir`. Verifies presence first so a missing
    /// or half-downloaded model gives a clear error instead of a native crash.
    pub fn load(
        spec: &'static ModelSpec,
        dir: &Path,
        threads: u32,
    ) -> Result<LocalEngine, LocalSttError> {
        if !spec.status_in(dir).is_ready() {
            return Err(LocalSttError::ModelMissing {
                dir: dir.display().to_string(),
            });
        }
        let started = Instant::now();
        let engine = Self::build(spec, dir, threads, started)?;
        log::info!(
            "local engine ready: model={} threads={threads} loaded in {} ms",
            spec.id,
            engine.load_ms
        );
        Ok(engine)
    }

    #[cfg(feature = "engine")]
    fn build(
        spec: &'static ModelSpec,
        dir: &Path,
        threads: u32,
        started: Instant,
    ) -> Result<LocalEngine, LocalSttError> {
        use sherpa_onnx::{
            OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
        };

        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(path("encoder.int8.onnx")),
            decoder: Some(path("decoder.int8.onnx")),
            joiner: Some(path("joiner.int8.onnx")),
        };
        config.model_config.tokens = Some(path("tokens.txt"));
        config.model_config.provider = Some("cpu".to_string());
        config.model_config.model_type = Some("nemo_transducer".to_string());
        config.model_config.num_threads = threads as i32;
        config.decoding_method = Some("greedy_search".to_string());

        // `create` returns Option, not Result: no detail is available beyond
        // "it failed", so say what we can and point at the likely cause.
        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            LocalSttError::EngineInit(format!(
                "sherpa-onnx rejected the model in {} (corrupt or incomplete weights?)",
                dir.display()
            ))
        })?;
        Ok(LocalEngine {
            recognizer,
            model_id: spec.id,
            load_ms: started.elapsed().as_millis(),
        })
    }

    #[cfg(not(feature = "engine"))]
    fn build(
        _spec: &'static ModelSpec,
        _dir: &Path,
        _threads: u32,
        _started: Instant,
    ) -> Result<LocalEngine, LocalSttError> {
        Err(LocalSttError::EngineUnavailable)
    }

    /// Transcribe 16 kHz mono samples.
    ///
    /// Takes raw `f32` rather than encoded WAV on purpose: the pipeline
    /// already holds the samples before it encodes a WAV for the cloud
    /// adapters, so the local path skips both an encode and a decode.
    #[cfg(feature = "engine")]
    pub fn transcribe(&self, samples_16k: &[f32]) -> Result<LocalTranscript, LocalSttError> {
        let started = Instant::now();
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(16_000, samples_16k);
        self.recognizer.decode(&stream);
        let text = stream.get_result().map(|r| r.text).unwrap_or_default();
        Ok(LocalTranscript {
            text,
            request_ms: started.elapsed().as_millis(),
        })
    }

    #[cfg(not(feature = "engine"))]
    pub fn transcribe(&self, _samples_16k: &[f32]) -> Result<LocalTranscript, LocalSttError> {
        Err(LocalSttError::EngineUnavailable)
    }

    pub fn model_id(&self) -> &'static str {
        self.model_id
    }

    /// How long the weights took to load, for the startup log line.
    pub fn load_ms(&self) -> u128 {
        self.load_ms
    }

    /// Whether this build can run on-device transcription at all.
    pub fn is_available() -> bool {
        cfg!(feature = "engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PARAKEET_V3_INT8;

    /// `LocalEngine` has no `Debug` impl (the native recognizer has none), so
    /// `expect_err` is unavailable; unwrap the error by hand. Same workaround
    /// `hark-stt` uses for `Transcript`.
    fn expect_err(result: Result<LocalEngine, LocalSttError>) -> LocalSttError {
        match result {
            Err(e) => e,
            Ok(engine) => panic!("expected an error, got an engine for {}", engine.model_id()),
        }
    }

    #[test]
    fn loading_a_missing_model_reports_the_directory_not_a_native_error() {
        // The presence check must run before sherpa-onnx sees the paths, so
        // the user gets "not downloaded" rather than an opaque init failure.
        let d = tempfile::tempdir().unwrap();
        let err = expect_err(LocalEngine::load(&PARAKEET_V3_INT8, d.path(), 2));
        assert!(matches!(err, LocalSttError::ModelMissing { .. }));
    }

    #[test]
    fn availability_tracks_the_engine_feature() {
        assert_eq!(LocalEngine::is_available(), cfg!(feature = "engine"));
    }
}
