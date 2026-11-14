use serde::{Deserialize, Serialize};

/// Which provider preset the user picked. `OpenaiCompatible` is the escape
/// hatch for any other endpoint speaking the multipart
/// `/audio/transcriptions` contract; it requires an explicit `base_url`.
///
/// Removing a variant needs a schema migration: there is no `#[serde(other)]`,
/// so a kind this build does not know fails the whole file (Groq, schema 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Deepgram,
    Openai,
    /// Gemini Live: a WebSocket session, not a REST POST. Selected here like
    /// any other provider; the transport difference is the adapter's problem.
    Gemini,
    OpenaiCompatible,
}

impl ProviderKind {
    /// Short label for reports, errors, and the keychain account name.
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Deepgram => "deepgram",
            ProviderKind::Openai => "openai",
            ProviderKind::Gemini => "gemini",
            ProviderKind::OpenaiCompatible => "openai-compatible",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    pub kind: ProviderKind,
    /// Defaults per kind; required (validated) for `openai-compatible`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Defaults per kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Gemini Live only: whether the provider hands back a literal transcript
    /// or one it has already cleaned up. Ignored by every other provider.
    #[serde(default)]
    pub live_mode: LiveMode,
    /// OpenAI only; ignored by every other provider. Streams the dictation to
    /// the realtime `gpt-live-transcribe` model while the key is held, instead
    /// of uploading the clip to the batch model after release. A flag rather
    /// than a model name, so `model` keeps the user's batch pin, which comes
    /// back when the toggle goes off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub live: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// OpenAI's realtime transcription model: what Live transcribe uses unless the
/// user pinned another `gpt-live-*` model.
pub const OPENAI_LIVE_MODEL: &str = "gpt-live-transcribe";

impl Default for Provider {
    fn default() -> Self {
        Provider {
            kind: ProviderKind::Deepgram,
            base_url: None,
            model: None,
            live_mode: LiveMode::default(),
            live: false,
        }
    }
}

/// How Gemini Live is asked to render the transcript.
///
/// Defaults to `Verbatim` rather than `Smart` on purpose. `Transcript::text` is
/// the ground truth the spellbook corrector, the invocation matcher and the
/// cleanup expansion guard all compare against; a provider that has already
/// rewritten the utterance leaves those checks with nothing honest to check.
/// `Smart` is a deliberate trade: one round trip instead of two, at the cost of
/// never seeing what was actually said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LiveMode {
    /// Literal transcript; Hark's own cleanup pass still runs.
    #[default]
    Verbatim,
    /// Provider removes disfluencies and formats in the same session, and the
    /// pipeline then skips its own cleanup call.
    Smart,
}

impl LiveMode {
    /// Short label for logs and the UI.
    pub fn label(self) -> &'static str {
        match self {
            LiveMode::Verbatim => "verbatim",
            LiveMode::Smart => "smart",
        }
    }
}

impl Provider {
    /// The effective base URL: explicit value, else the kind's default.
    pub fn resolved_base_url(&self) -> Option<String> {
        if let Some(url) = &self.base_url {
            return Some(url.clone());
        }
        match self.kind {
            ProviderKind::Deepgram => Some("https://api.deepgram.com".to_string()),
            ProviderKind::Openai => Some("https://api.openai.com/v1".to_string()),
            // The REST host, used by the batch/fused Gemini adapter. The Live
            // adapter talks to a fixed `wss://` host instead and ignores this;
            // the value is still resolved so every kind has a coherent config.
            ProviderKind::Gemini => {
                Some("https://generativelanguage.googleapis.com/v1beta".to_string())
            }
            ProviderKind::OpenaiCompatible => None,
        }
    }

    /// The effective model: explicit value, else the kind's default.
    /// The spike verdict (2026-07-16) made Deepgram nova-3 the app default.
    /// The OpenAI default moved to `gpt-transcribe` on 2026-09-17: OpenAI
    /// documents it as the recommended model for file transcription and no
    /// longer recommends `gpt-4o-transcribe`/`whisper-1` for new integrations.
    pub fn resolved_model(&self) -> String {
        if let Some(model) = &self.model {
            return model.clone();
        }
        match self.kind {
            ProviderKind::Deepgram => "nova-3",
            ProviderKind::Openai => "gpt-transcribe",
            ProviderKind::Gemini => "gemini-3.5-transcribe-live",
            // Validation guarantees openai-compatible configs are explicit;
            // an empty model is still a valid request for some servers.
            ProviderKind::OpenaiCompatible => "",
        }
        .to_string()
    }

    /// Whether the resolved model speaks the gpt-transcribe biasing contract
    /// (repeated `keywords[]` + plural `languages[]`) rather than the
    /// Whisper-family one (a single 224-token `prompt`).
    ///
    /// Keyed on the model, not the provider kind, because the contract belongs
    /// to the model: an `openai-compatible` base_url pointed at a gateway that
    /// proxies gpt-transcribe needs the same treatment as OpenAI itself.
    ///
    /// `gpt-live-transcribe` deliberately does not match. It is realtime-only:
    /// it reaches the WebSocket adapter through
    /// [`Provider::uses_live_transcribe`], never the multipart endpoint this
    /// contract describes.
    pub fn uses_keywords_biasing(&self) -> bool {
        let model = self.resolved_model();
        model == "gpt-transcribe" || model.starts_with("gpt-transcribe-")
    }

    /// Whether OpenAI dictation streams to the realtime model: the toggle is
    /// on, or a hand-edited `model` is a `gpt-live-*` one (the contract
    /// belongs to the model, as with gpt-transcribe). False for every other
    /// kind, whatever `live` says.
    pub fn uses_live_transcribe(&self) -> bool {
        self.kind == ProviderKind::Openai
            && (self.live || self.resolved_model().starts_with("gpt-live-"))
    }

    /// The realtime model when [`Provider::uses_live_transcribe`] holds: a
    /// pinned `gpt-live-*` model wins, else [`OPENAI_LIVE_MODEL`]. A pinned
    /// batch model is never sent to the realtime API; it stays in `model` for
    /// when the toggle goes off.
    pub fn live_transcribe_model(&self) -> Option<String> {
        if !self.uses_live_transcribe() {
            return None;
        }
        let model = self.resolved_model();
        Some(if model.starts_with("gpt-live-") {
            model
        } else {
            OPENAI_LIVE_MODEL.to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConfigError, Settings};

    #[test]
    fn the_keywords_contract_is_keyed_on_the_model_not_the_kind() {
        let openai_default = Settings::from_toml("[provider]\nkind = \"openai\"").unwrap();
        assert!(openai_default.provider.uses_keywords_biasing());

        let pinned_whisper =
            Settings::from_toml("[provider]\nkind = \"openai\"\nmodel = \"whisper-1\"").unwrap();
        assert!(!pinned_whisper.provider.uses_keywords_biasing());

        let whisper_server = Settings::from_toml(
            "[provider]\nkind = \"openai-compatible\"\nbase_url = \"http://localhost:8000/v1\"\n\
             model = \"whisper-large-v3-turbo\"",
        )
        .unwrap();
        assert!(!whisper_server.provider.uses_keywords_biasing());

        let deepgram = Settings::default();
        assert!(!deepgram.provider.uses_keywords_biasing());
    }

    #[test]
    fn a_dated_gpt_transcribe_snapshot_keeps_the_keywords_contract() {
        let dated = Settings::from_toml(
            "[provider]\nkind = \"openai\"\nmodel = \"gpt-transcribe-2026-09-01\"",
        )
        .unwrap();
        assert!(dated.provider.uses_keywords_biasing());
    }

    #[test]
    fn gpt_live_transcribe_is_not_the_batch_keywords_contract() {
        // Realtime-only model; it must not be mistaken for gpt-transcribe by a
        // loose substring match.
        let live =
            Settings::from_toml("[provider]\nkind = \"openai\"\nmodel = \"gpt-live-transcribe\"")
                .unwrap();
        assert!(!live.provider.uses_keywords_biasing());
    }

    // --- OpenAI Live transcribe toggle ---

    #[test]
    fn live_defaults_off_and_is_not_written_when_off() {
        let s = Settings::from_toml("[provider]\nkind = \"openai\"").unwrap();
        assert!(!s.provider.live);
        assert!(!s.provider.uses_live_transcribe());
        assert_eq!(s.provider.live_transcribe_model(), None);
        let out = s.to_toml().unwrap();
        assert!(!out.contains("live ="), "{out}");
    }

    #[test]
    fn live_round_trips_when_on() {
        let s = Settings::from_toml("[provider]\nkind = \"openai\"\nlive = true").unwrap();
        assert!(s.provider.uses_live_transcribe());
        assert_eq!(
            s.provider.live_transcribe_model().as_deref(),
            Some(OPENAI_LIVE_MODEL)
        );
        let out = s.to_toml().unwrap();
        assert!(out.contains("live = true"), "{out}");
        assert!(Settings::from_toml(&out).unwrap().provider.live);
    }

    #[test]
    fn toggling_live_keeps_a_pinned_batch_model() {
        let mut s =
            Settings::from_toml("[provider]\nkind = \"openai\"\nmodel = \"whisper-1\"").unwrap();
        s.provider.live = true;
        assert_eq!(
            s.provider.live_transcribe_model().as_deref(),
            Some("gpt-live-transcribe"),
            "a batch model is never sent to the realtime API"
        );
        s.provider.live = false;
        assert_eq!(s.provider.resolved_model(), "whisper-1");
        assert!(!s.provider.uses_live_transcribe());
    }

    #[test]
    fn a_hand_edited_live_model_turns_live_on_and_is_kept() {
        let s =
            Settings::from_toml("[provider]\nkind = \"openai\"\nmodel = \"gpt-live-transcribe\"")
                .unwrap();
        assert!(!s.provider.live);
        assert!(s.provider.uses_live_transcribe());
        let dated = Settings::from_toml(
            "[provider]\nkind = \"openai\"\nmodel = \"gpt-live-transcribe-2026-10-01\"",
        )
        .unwrap();
        assert_eq!(
            dated.provider.live_transcribe_model().as_deref(),
            Some("gpt-live-transcribe-2026-10-01")
        );
    }

    #[test]
    fn live_is_ignored_by_every_other_provider() {
        for kind in ["deepgram", "gemini"] {
            let s = Settings::from_toml(&format!("[provider]\nkind = \"{kind}\"\nlive = true"))
                .unwrap();
            assert!(!s.provider.uses_live_transcribe(), "{kind}");
            assert_eq!(s.provider.live_transcribe_model(), None, "{kind}");
        }
        let custom = Settings::from_toml(
            "[provider]\nkind = \"openai-compatible\"\nbase_url = \"https://gw.example/v1\"\n\
             model = \"gpt-live-transcribe\"\nlive = true",
        )
        .unwrap();
        assert!(
            !custom.provider.uses_live_transcribe(),
            "the realtime adapter is OpenAI's own; a gateway gets no WebSocket"
        );
    }

    #[test]
    fn gemini_defaults_to_the_live_model_and_a_verbatim_transcript() {
        let s = Settings::from_toml("[provider]\nkind = \"gemini\"").unwrap();
        assert_eq!(s.provider.kind, ProviderKind::Gemini);
        assert_eq!(s.provider.resolved_model(), "gemini-3.5-transcribe-live");
        assert_eq!(s.provider.kind.label(), "gemini");
        // Verbatim by default: `text` has to stay the ground truth the
        // spellbook, invocations and the expansion guard compare against.
        assert_eq!(s.provider.live_mode, LiveMode::Verbatim);
    }

    #[test]
    fn smart_mode_round_trips_through_toml() {
        let s =
            Settings::from_toml("[provider]\nkind = \"gemini\"\nlive_mode = \"smart\"").unwrap();
        assert_eq!(s.provider.live_mode, LiveMode::Smart);
        let out = toml::to_string_pretty(&s).expect("serializes");
        let back = Settings::from_toml(&out).expect("re-parses");
        assert_eq!(back.provider.live_mode, LiveMode::Smart);
    }

    #[test]
    fn gemini_is_not_mistaken_for_the_gpt_transcribe_contract() {
        let s = Settings::from_toml("[provider]\nkind = \"gemini\"").unwrap();
        assert!(!s.provider.uses_keywords_biasing());
    }

    #[test]
    fn openai_compatible_requires_base_url() {
        let err = Settings::from_toml("[provider]\nkind = \"openai-compatible\"")
            .expect_err("missing base_url must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));

        let s = Settings::from_toml(
            "[provider]\nkind = \"openai-compatible\"\nbase_url = \"http://localhost:8000/v1\"",
        )
        .expect("explicit base_url is accepted");
        assert_eq!(
            s.provider.resolved_base_url().as_deref(),
            Some("http://localhost:8000/v1")
        );
    }

    #[test]
    fn provider_labels_are_stable() {
        // The keychain account name derives from these; changing one silently
        // orphans stored keys.
        assert_eq!(ProviderKind::Deepgram.label(), "deepgram");
        assert_eq!(ProviderKind::Openai.label(), "openai");
        assert_eq!(ProviderKind::Gemini.label(), "gemini");
        assert_eq!(ProviderKind::OpenaiCompatible.label(), "openai-compatible");
    }
}
