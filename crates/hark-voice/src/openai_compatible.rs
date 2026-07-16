//! Pure request/response layer for the OpenAI-compatible chat-completions
//! contract: `POST {base_url}/chat/completions`, Bearer auth, buffered JSON
//! body. One code path serves every provider that clones the OpenAI contract
//! (OpenAI itself, Groq, any compatible endpoint); Deepgram has no chat
//! product and is rejected at config validation.
//!
//! The live adapter (CP3) is a thin I/O shell over these functions; the CP0
//! spike drives them against real endpoints.

use crate::error::{truncate_snippet, CleanupError};

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
