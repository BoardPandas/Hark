/// Which HTTP contract an adapter speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Multipart `POST {base_url}/audio/transcriptions`, Bearer auth.
    /// One code path, multiple providers (OpenAI, Groq).
    OpenAiCompatible,
    /// `POST {base_url}/v1/listen`, `Token` auth, raw `audio/wav` body.
    Deepgram,
    /// `POST {base_url}/interactions`, `x-goog-api-key` auth, inline base64
    /// audio. The only fused adapter: returns transcript *and* cleanup from
    /// one round trip (see `gemini::FusedText`).
    Gemini,
}

/// Everything needed to build one provider adapter. The spike fills this from
/// env vars; the app will fill it from settings + the OS keychain.
#[derive(Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    /// A short human label for reports and errors ("groq", "openai", "deepgram").
    /// Error messages carry this, never the key.
    pub label: String,
    /// e.g. "https://api.groq.com/openai/v1" or "https://api.deepgram.com".
    pub base_url: String,
    /// e.g. "whisper-large-v3-turbo", "gpt-4o-mini-transcribe", "nova-3".
    pub model: String,
    /// Spike: from env. App: from keyring. Never logged.
    pub api_key: String,
    /// Dictionary-ish bias terms, mapped per adapter: `prompt` (openai-compatible),
    /// repeated `keyterm` query params (deepgram), or a spelling glossary in the
    /// system instruction (gemini).
    pub bias_terms: Vec<String>,
    /// Fused adapters only (`ProviderKind::Gemini`): the cleanup rules to apply
    /// in the same call, supplied by the caller as hark-voice's assembled voice
    /// prompt so the tuned wording lives in one place. `None` means transcribe
    /// only; every non-fused adapter ignores this field.
    pub cleanup_instruction: Option<String>,
}

// Deliberately no Debug derive: a reflexive `{config:?}` in some future log line
// must not be able to leak `api_key`.
impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("kind", &self.kind)
            .field("label", &self.label)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .field("bias_terms", &self.bias_terms)
            // User content (a Custom voice prompt can reach here): count only.
            .field("cleanup_instruction", &self.cleanup_instruction.is_some())
            .finish()
    }
}
