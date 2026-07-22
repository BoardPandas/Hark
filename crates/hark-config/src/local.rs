//! The `[local_stt]` section: opt-in on-device transcription.
//!
//! Off by default and additive, so every config file written before this
//! section existed keeps loading unchanged. The model weights are *not*
//! shipped with the app; they are downloaded on demand from the Settings
//! page into `<data_dir>/models/<model>/` (see `hark-local-stt`).

use crate::ConfigError;
use serde::{Deserialize, Serialize};

/// How the on-device model participates in dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LocalMode {
    /// Never used. Nothing downloaded, no RAM, no behavior change.
    #[default]
    Off,
    /// Cloud first; the local model rescues the dictation when cloud fails.
    Fallback,
    /// Local only. The cloud provider is never called and no API key is
    /// required for the pipeline to start.
    Primary,
}

impl LocalMode {
    /// Short label for logs, dictation records, and the UI.
    pub fn label(self) -> &'static str {
        match self {
            LocalMode::Off => "off",
            LocalMode::Fallback => "fallback",
            LocalMode::Primary => "primary",
        }
    }

    /// Whether the on-device engine is loaded at all under this mode.
    pub fn uses_local(self) -> bool {
        !matches!(self, LocalMode::Off)
    }

    /// Whether the cloud provider is contacted under this mode. `false` for
    /// `Primary` is what lets the pipeline start with no API key.
    pub fn uses_cloud(self) -> bool {
        !matches!(self, LocalMode::Primary)
    }
}

/// The default model id. Parakeet TDT 0.6B v3, int8 ONNX — the newest
/// revision (v2 is the English-only predecessor, ~0.27 pp better on English
/// and 24 languages worse). Changing this string to
/// `parakeet-tdt-0.6b-v2-int8` is the supported way to go back.
pub const DEFAULT_MODEL: &str = "parakeet-tdt-0.6b-v3-int8";

/// Cloud request timeout used *instead of* `hark_stt::TOTAL_TIMEOUT_MS` when
/// a local fallback is armed and ready. The full 15 s timeout plus ~2 s of
/// local decoding would make a fallback dictation take 17 s, which is worse
/// than no fallback at all; failing over fast is the whole point.
pub const DEFAULT_FALLBACK_AFTER_MS: u64 = 6_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalStt {
    pub mode: LocalMode,
    /// Model id; also the directory name under `<data_dir>/models/`.
    pub model: String,
    /// ONNX intra-op threads. Kept low so decoding does not starve the UI
    /// thread on a 4-core laptop.
    pub threads: u32,
    /// See [`DEFAULT_FALLBACK_AFTER_MS`]. Only consulted in `Fallback` mode.
    pub fallback_after_ms: u64,
}

impl Default for LocalStt {
    fn default() -> Self {
        LocalStt {
            mode: LocalMode::Off,
            model: DEFAULT_MODEL.to_string(),
            threads: 2,
            fallback_after_ms: DEFAULT_FALLBACK_AFTER_MS,
        }
    }
}

/// Reject values that would produce a broken engine or a useless fallback.
/// A `Primary` mode with no model on disk is deliberately *not* an error
/// here: the user may flip the toggle before pressing Download, and the UI
/// surfaces that state far better than a failed config load would.
pub(crate) fn validate(local: &LocalStt) -> Result<(), ConfigError> {
    if local.model.trim().is_empty() {
        return Err(ConfigError::Invalid(
            "local_stt.model must not be empty".to_string(),
        ));
    }
    // A model id becomes a directory name; keep it a plain segment so it can
    // never escape the models root.
    if local.model.contains(['/', '\\']) || local.model.contains("..") {
        return Err(ConfigError::Invalid(format!(
            "local_stt.model must be a plain name, got {:?}",
            local.model
        )));
    }
    if local.threads == 0 {
        return Err(ConfigError::Invalid(
            "local_stt.threads must be at least 1".to_string(),
        ));
    }
    if local.mode == LocalMode::Fallback && local.fallback_after_ms < 500 {
        return Err(ConfigError::Invalid(
            "local_stt.fallback_after_ms must be at least 500".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    #[test]
    fn defaults_are_off_with_the_v3_model() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert_eq!(s.local_stt.mode, LocalMode::Off);
        assert_eq!(s.local_stt.model, "parakeet-tdt-0.6b-v3-int8");
        assert_eq!(s.local_stt.threads, 2);
        assert_eq!(s.local_stt.fallback_after_ms, 6_000);
    }

    #[test]
    fn modes_parse_from_kebab_case_and_round_trip() {
        for (text, mode) in [
            ("off", LocalMode::Off),
            ("fallback", LocalMode::Fallback),
            ("primary", LocalMode::Primary),
        ] {
            let s = Settings::from_toml(&format!("[local_stt]\nmode = \"{text}\""))
                .expect("mode parses");
            assert_eq!(s.local_stt.mode, mode);
            let round = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
            assert_eq!(round.local_stt.mode, mode);
        }
    }

    #[test]
    fn mode_predicates_drive_the_pipeline_decisions() {
        // Primary is the only mode that may start without a cloud key.
        assert!(!LocalMode::Primary.uses_cloud());
        assert!(LocalMode::Fallback.uses_cloud());
        assert!(LocalMode::Off.uses_cloud());
        // Off is the only mode that never loads the engine.
        assert!(!LocalMode::Off.uses_local());
        assert!(LocalMode::Fallback.uses_local());
        assert!(LocalMode::Primary.uses_local());
    }

    #[test]
    fn a_model_id_may_not_escape_the_models_root() {
        for bad in ["../../etc", "a/b", "a\\b", ".."] {
            let err = Settings::from_toml(&format!("[local_stt]\nmodel = {bad:?}"))
                .expect_err("path-like model ids must be rejected");
            assert!(matches!(err, ConfigError::Invalid(_)), "{bad}");
        }
    }

    #[test]
    fn zero_threads_and_an_instant_fallback_are_rejected() {
        let err = Settings::from_toml("[local_stt]\nthreads = 0")
            .expect_err("zero threads must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));

        let err = Settings::from_toml("[local_stt]\nmode = \"fallback\"\nfallback_after_ms = 10")
            .expect_err("a sub-500ms cloud budget must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));

        // The same tiny value is harmless when no fallback is armed.
        Settings::from_toml("[local_stt]\nfallback_after_ms = 10")
            .expect("unused fallback budget is not validated");
    }

    #[test]
    fn a_pre_local_stt_config_file_still_loads() {
        // The exact shape a 0.16.0 user has on disk today.
        let s = Settings::from_toml(
            "version = 1\n[provider]\nkind = \"deepgram\"\n[hotkey]\nptt_key = \"LCtrl+LWin\"",
        )
        .expect("a config file predating [local_stt] must load");
        assert_eq!(s.local_stt.mode, LocalMode::Off);
    }
}
