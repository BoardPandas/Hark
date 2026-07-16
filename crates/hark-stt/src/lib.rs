//! Hark STT engine seed: load Parakeet TDT via the official `sherpa-onnx` crate,
//! keep it warm, and batch-decode 16 kHz mono f32 audio to text.
//!
//! `SttEngine` is deliberately I/O-free beyond the model itself — wav loading,
//! timing, and reporting live in the harness (`examples/decode_spike.rs`) so the
//! engine stays reusable by the eventual pipeline, which will call
//! `decode(&[f32], u32)` from a worker thread.

mod config;
mod error;
pub mod metrics;

pub use config::{
    check_audio_format, parse_hotword_line, serialize_hotword_entry, DecodingMethod, HotwordConfig,
    HotwordEntry, SttConfig,
};
pub use error::SttError;

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use std::path::Path;
use std::time::Instant;

/// A decode result plus how long the decode call took (release-to-text on the hot
/// path; excludes wav loading, which the caller owns).
pub struct Transcript {
    pub text: String,
    pub decode_ms: u128,
}

/// A loaded, ready-to-decode recognizer.
pub struct SttEngine {
    recognizer: OfflineRecognizer,
    provider: String,
}

impl SttEngine {
    /// Build the recognizer and run one warmup decode so the first *real* decode is
    /// not cold. This is the path the pipeline should use.
    pub fn load(cfg: SttConfig) -> Result<Self, SttError> {
        let engine = Self::build(cfg)?;
        engine.warmup();
        Ok(engine)
    }

    /// Build the recognizer without warming it. The harness uses this to measure the
    /// cold-vs-warm delta; production code should prefer [`SttEngine::load`].
    pub fn load_without_warmup(cfg: SttConfig) -> Result<Self, SttError> {
        Self::build(cfg)
    }

    /// One throwaway decode on 1 s of silence to trigger lazy graph init/allocation.
    pub fn warmup(&self) {
        let silence = vec![0.0f32; 16_000];
        let _ = self.decode(&silence, 16_000);
    }

    fn build(cfg: SttConfig) -> Result<Self, SttError> {
        for path in [&cfg.encoder, &cfg.decoder, &cfg.joiner, &cfg.tokens] {
            if !path.exists() {
                return Err(SttError::ModelNotFound(path.clone()));
            }
        }

        let mut rc = OfflineRecognizerConfig::default();
        rc.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(path_str(&cfg.encoder)),
            decoder: Some(path_str(&cfg.decoder)),
            joiner: Some(path_str(&cfg.joiner)),
        };
        rc.model_config.tokens = Some(path_str(&cfg.tokens));
        rc.model_config.provider = Some(cfg.provider.clone());
        rc.model_config.num_threads = cfg.num_threads;

        rc.decoding_method = Some(cfg.decoding.as_sherpa_str().to_string());
        if let DecodingMethod::ModifiedBeamSearch { max_active_paths } = cfg.decoding {
            rc.max_active_paths = max_active_paths;
        }

        if let Some(hw) = &cfg.hotwords {
            rc.hotwords_file = Some(path_str(&hw.file));
            rc.hotwords_score = hw.score;
            if let Some(unit) = &hw.modeling_unit {
                rc.model_config.modeling_unit = Some(unit.clone());
            }
            if let Some(vocab) = &hw.bpe_vocab {
                rc.model_config.bpe_vocab = Some(path_str(vocab));
            }
        }

        let recognizer = OfflineRecognizer::create(&rc).map_err(|e| SttError::RecognizerCreate {
            provider: cfg.provider.clone(),
            detail: format!("{e:?}"),
        })?;

        Ok(Self {
            recognizer,
            provider: cfg.provider,
        })
    }

    /// Decode 16 kHz mono f32 samples to text. Times only the decode call.
    pub fn decode(&self, samples: &[f32], sample_rate: u32) -> Result<Transcript, SttError> {
        let started = Instant::now();
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate, samples);
        self.recognizer.decode(&stream);
        let text = stream
            .get_result()
            .map(|r| r.text)
            .ok_or(SttError::NoResult)?;
        Ok(Transcript {
            text,
            decode_ms: started.elapsed().as_millis(),
        })
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
