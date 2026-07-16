//! Pure request/response layer for the OpenAI-compatible chat-completions
//! contract: `POST {base_url}/chat/completions`, Bearer auth, buffered JSON
//! body. One code path serves every provider that clones the OpenAI contract
//! (OpenAI itself, Groq, any compatible endpoint); Deepgram has no chat
//! product and is rejected at config validation.
//!
//! `OpenAiCompatibleChat` is a thin I/O shell over the pure functions here;
//! the cleanup spike drives the same functions against real endpoints.

use crate::error::{error_for_status, error_for_transport, truncate_snippet, CleanupError};
use crate::{present_terms, system_prompt, Cleaned, CleanupProvider, Voice, CLEANUP_TIMEOUT_MS};
use reqwest::blocking::Client;
use std::time::{Duration, Instant};

/// `{base_url}/chat/completions`, tolerant of a trailing slash on base_url.
pub fn chat_completions_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Output-token floor. Deliberately generous: reasoning models spend
/// reasoning tokens from `max_completion_tokens`, and a too-tight cap yields
/// empty `content` with `finish_reason: "length"` (headroom verified at CP0).
pub const MAX_COMPLETION_TOKENS_FLOOR: usize = 512;
/// Output-token ceiling: a cleanup rewrite never legitimately needs more.
pub const MAX_COMPLETION_TOKENS_CAP: usize = 4_096;

/// Derive the output-token cap from input length: estimate input tokens as
/// chars/4, allow twice that plus 256, clamped to [floor, cap].
pub fn max_completion_tokens(input: &str) -> u32 {
    let estimated_input_tokens = input.chars().count() / 4;
    (2 * estimated_input_tokens + 256).clamp(MAX_COMPLETION_TOKENS_FLOOR, MAX_COMPLETION_TOKENS_CAP)
        as u32
}

#[derive(serde::Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(serde::Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    // Both OpenAI and Groq have converged on this name (Groq deprecates
    // `max_tokens`); no per-provider branching.
    max_completion_tokens: u32,
    // Only serialized when configured: OpenAI's GPT-5 family rejects any
    // non-default temperature with a 400. Presets set it per provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    // OpenAI GPT-5 family only ("minimal" for short deterministic rewrites);
    // the Groq preset leaves it unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
}

/// Assemble the complete JSON request body: system prompt + the transcript as
/// the single user message. Buffered `Vec<u8>` (never streamed) so transport
/// errors stay classifiable and tests can assert on the exact fields.
pub fn build_request_body(
    model: &str,
    system_prompt: &str,
    user_text: &str,
    temperature: Option<f32>,
    reasoning_effort: Option<&str>,
) -> Vec<u8> {
    let request = ChatRequest {
        model,
        messages: [
            ChatMessage {
                role: "system",
                content: system_prompt,
            },
            ChatMessage {
                role: "user",
                content: user_text,
            },
        ],
        max_completion_tokens: max_completion_tokens(user_text),
        temperature,
        reasoning_effort,
    };
    // Plain structs of strings and numbers cannot fail JSON serialization.
    serde_json::to_vec(&request).expect("chat request serialization cannot fail")
}

/// Extract `choices[0].message.content` from the JSON response body, trimmed.
/// Pure for unit tests. Empty or missing content is a `Provider` error (the
/// pipeline treats every cleanup error as fail-open); `finish_reason` rides
/// the detail because `"length"` there means reasoning tokens ate the whole
/// `max_completion_tokens` budget, which otherwise looks like a provider bug.
pub fn parse_response(provider: &str, body: &str) -> Result<String, CleanupError> {
    #[derive(serde::Deserialize)]
    struct Response {
        choices: Vec<Choice>,
    }
    #[derive(serde::Deserialize)]
    struct Choice {
        message: Message,
        finish_reason: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Message {
        // Reasoning models return `content: null` when the budget runs out.
        content: Option<String>,
    }

    let response: Response = serde_json::from_str(body).map_err(|e| CleanupError::Provider {
        provider: provider.to_string(),
        detail: format!("unexpected response body ({e}): {}", truncate_snippet(body)),
    })?;
    let Some(choice) = response.choices.into_iter().next() else {
        return Err(CleanupError::Provider {
            provider: provider.to_string(),
            detail: "response contained no choices".to_string(),
        });
    };
    let content = choice.message.content.unwrap_or_default();
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(CleanupError::Provider {
            provider: provider.to_string(),
            detail: format!(
                "empty content (finish_reason: {})",
                choice.finish_reason.as_deref().unwrap_or("unknown")
            ),
        });
    }
    Ok(trimmed.to_string())
}

/// Parse a Retry-After header value in seconds form (HTTP-date form is rare
/// on these APIs and not worth the dependency). Rate-limit headers are
/// identical across OpenAI and Groq, so the STT pattern carries over.
pub fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Everything needed to build one cleanup adapter. The pipeline fills this
/// from the resolved provider (hark-config) plus the key from hark-keychain
/// or STT-key reuse. Dictionary terms are user content: they may enter the
/// request body but never logs.
#[derive(Clone)]
pub struct CleanupConfig {
    /// Short human label for logs and errors ("openai", "groq"). Error
    /// messages carry this, never the key.
    pub label: String,
    /// e.g. "https://api.openai.com/v1"; chat and STT share base URLs.
    pub base_url: String,
    /// e.g. "gpt-5-nano", "llama-3.1-8b-instant".
    pub model: String,
    /// From the keychain or STT-key reuse. Never logged.
    pub api_key: String,
    /// Serialized into the request only when present (GPT-5 family rejects
    /// any non-default temperature).
    pub temperature: Option<f32>,
    /// Serialized only when present (OpenAI GPT-5 family only).
    pub reasoning_effort: Option<String>,
    /// The effective voice. Verbatim is rejected at construction: the
    /// pipeline short-circuits it long before an adapter exists.
    pub voice: Voice,
    /// The user's prompt for `Voice::Custom`; ignored otherwise.
    pub custom_prompt: String,
    /// Dictionary terms; the per-request protected-terms clause subsets
    /// these to the ones present in the outgoing text.
    pub dictionary_terms: Vec<String>,
}

// Deliberately no Debug derive: a reflexive `{config:?}` in some future log
// line must not be able to leak `api_key` (or prompt/term user content).
impl std::fmt::Debug for CleanupConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CleanupConfig")
            .field("label", &self.label)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .field("voice", &self.voice)
            .field("custom_prompt", &"<user content>")
            .field("dictionary_terms", &self.dictionary_terms.len())
            .finish()
    }
}

/// The one live adapter: `POST {base_url}/chat/completions` with Bearer auth
/// and a per-request timeout tighter than STT's. **No retry**: cleanup
/// failure has a graceful fallback (the pipeline injects the uncleaned
/// text), so a retry would only double worst-case hot-path latency.
pub struct OpenAiCompatibleChat {
    client: Client,
    label: String,
    url: String,
    model: String,
    api_key: String,
    temperature: Option<f32>,
    reasoning_effort: Option<String>,
    voice: Voice,
    custom_prompt: String,
    dictionary_terms: Vec<String>,
}

impl OpenAiCompatibleChat {
    /// Build the adapter, sharing the process-wide HTTP client (`Client` is
    /// an `Arc` internally). Rejects `Voice::Verbatim`: constructing a
    /// cleaner for a voice that never calls is a caller bug, surfaced as an
    /// error the fail-open pipeline logs rather than a panic.
    pub fn new(config: &CleanupConfig, client: Client) -> Result<Self, CleanupError> {
        if config.voice == Voice::Verbatim {
            return Err(CleanupError::Provider {
                provider: config.label.clone(),
                detail: "verbatim voice never constructs a cleanup adapter".to_string(),
            });
        }
        Ok(Self {
            client,
            label: config.label.clone(),
            url: chat_completions_url(&config.base_url),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            temperature: config.temperature,
            reasoning_effort: config.reasoning_effort.clone(),
            voice: config.voice,
            custom_prompt: config.custom_prompt.clone(),
            dictionary_terms: config.dictionary_terms.clone(),
        })
    }
}

impl CleanupProvider for OpenAiCompatibleChat {
    fn clean(&self, text: &str) -> Result<Cleaned, CleanupError> {
        // Prompt assembly is per-request: the protected-terms clause depends
        // on which dictionary terms the outgoing text actually contains.
        let present = present_terms(text, &self.dictionary_terms);
        let prompt = system_prompt(self.voice, &self.custom_prompt, &present)
            .expect("non-verbatim voice enforced at construction");
        let body = build_request_body(
            &self.model,
            &prompt,
            text,
            self.temperature,
            self.reasoning_effort.as_deref(),
        );

        let started = Instant::now();
        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(Duration::from_millis(CLEANUP_TIMEOUT_MS))
            .body(body)
            .send()
            .map_err(|e| error_for_transport(&self.label, CLEANUP_TIMEOUT_MS, &e))?;

        let status = response.status();
        let retry_after_s = retry_after_secs(response.headers());
        let body_text = response
            .text()
            .map_err(|e| error_for_transport(&self.label, CLEANUP_TIMEOUT_MS, &e))?;
        let request_ms = started.elapsed().as_millis();

        if !status.is_success() {
            return Err(error_for_status(
                &self.label,
                status.as_u16(),
                retry_after_s,
                &body_text,
            ));
        }
        Ok(Cleaned {
            text: parse_response(&self.label, &body_text)?,
            request_ms,
        })
    }

    fn label(&self) -> &str {
        &self.label
    }
}
