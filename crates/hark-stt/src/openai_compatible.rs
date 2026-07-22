//! The OpenAI-compatible adapter: multipart `POST {base_url}/audio/transcriptions`
//! with Bearer auth. One code path serves every provider that clones the OpenAI
//! contract (OpenAI itself, Groq).
//!
//! The multipart body is assembled by hand into a buffered `Vec<u8>` instead of
//! `reqwest::blocking::multipart`: reqwest streams multipart bodies through a
//! channel to its internal runtime, so a connect/timeout failure surfaces as an
//! opaque "send failed because receiver is gone" body error with
//! `is_connect()`/`is_timeout()` both false, breaking the error taxonomy. A
//! buffered body keeps transport errors classifiable (and the assembly testable).

use crate::error::{error_for_status, error_for_transport, truncate_snippet, SttError};
use crate::{ProviderConfig, SttProvider, Transcript, TOTAL_TIMEOUT_MS};
use reqwest::blocking::Client;
use std::time::Instant;

pub struct OpenAiCompatible {
    client: Client,
    label: String,
    url: String,
    model: String,
    api_key: String,
    prompt: Option<String>,
}

impl OpenAiCompatible {
    pub fn new(config: &ProviderConfig, client: Client) -> Self {
        let (prompt, included) = prompt_from_bias_terms(&config.bias_terms);
        if !config.bias_terms.is_empty() {
            // Counts only: terms are user content and never appear in logs.
            log::info!(
                "prompt bias: included {included} of {} terms",
                config.bias_terms.len()
            );
        }
        Self {
            client,
            label: config.label.clone(),
            url: transcriptions_url(&config.base_url),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            prompt,
        }
    }
}

/// `{base_url}/audio/transcriptions`, tolerant of a trailing slash on base_url.
pub fn transcriptions_url(base_url: &str) -> String {
    format!("{}/audio/transcriptions", base_url.trim_end_matches('/'))
}

/// Whisper-family models truncate prompts at 224 tokens; keep the bias
/// glossary safely under that with the ~4-chars-per-token heuristic.
const PROMPT_TOKEN_BUDGET: usize = 200;

/// Whisper-family biasing: bias terms ride the `prompt` field as a
/// comma-separated glossary, included in order until the approximate token
/// budget is spent; the rest are dropped (a term that would cross the
/// budget is dropped even if a later, shorter one might fit — order is the
/// user's priority signal). Returns the prompt (`None` when no term made
/// it, so the field is omitted entirely) and how many terms were included.
pub fn prompt_from_bias_terms(bias_terms: &[String]) -> (Option<String>, usize) {
    let budget_chars = PROMPT_TOKEN_BUDGET * 4;
    let mut prompt = String::new();
    let mut chars = 0;
    let mut included = 0;
    for term in bias_terms {
        let added = term.chars().count() + if included > 0 { 2 } else { 0 };
        if chars + added > budget_chars {
            break;
        }
        if included > 0 {
            prompt.push_str(", ");
        }
        prompt.push_str(term);
        chars += added;
        included += 1;
    }
    ((included > 0).then_some(prompt), included)
}

/// The text fields of the multipart form, as (name, value) pairs.
pub fn form_text_fields(model: &str, prompt: Option<&str>) -> Vec<(&'static str, String)> {
    let mut fields = vec![
        ("model", model.to_string()),
        ("response_format", "json".to_string()),
        ("language", "en".to_string()),
    ];
    if let Some(p) = prompt {
        fields.push(("prompt", p.to_string()));
    }
    fields
}

/// A boundary that provably does not occur in the payload (extends itself until
/// it doesn't; WAV bytes could theoretically contain any fixed string).
pub fn multipart_boundary(payload: &[u8]) -> String {
    let mut boundary = "hark-stt-boundary-7f3a".to_string();
    while payload
        .windows(boundary.len())
        .any(|w| w == boundary.as_bytes())
    {
        boundary.push('x');
    }
    boundary
}

/// Assemble the complete multipart/form-data body: every text field, then the
/// file part (`name="file"`, `filename`, `Content-Type: audio/wav`). Pure and
/// buffered, so tests can assert on the exact bytes.
pub fn build_multipart_body(
    boundary: &str,
    text_fields: &[(&str, String)],
    file_bytes: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(file_bytes.len() + 1024);
    for (name, value) in text_fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"spike_clip.wav\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// Extract `text` from the JSON response body. Pure for unit tests.
pub fn parse_response(provider: &str, body: &str) -> Result<String, SttError> {
    #[derive(serde::Deserialize)]
    struct Response {
        text: String,
    }
    serde_json::from_str::<Response>(body)
        .map(|r| r.text)
        .map_err(|e| SttError::Provider {
            provider: provider.to_string(),
            detail: format!("unexpected response body ({e}): {}", truncate_snippet(body)),
        })
}

impl SttProvider for OpenAiCompatible {
    fn transcribe(&self, wav_bytes: &[u8]) -> Result<Transcript, SttError> {
        let boundary = multipart_boundary(wav_bytes);
        let body = build_multipart_body(
            &boundary,
            &form_text_fields(&self.model, self.prompt.as_deref()),
            wav_bytes,
        );

        let started = Instant::now();
        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .map_err(|e| error_for_transport(&self.label, TOTAL_TIMEOUT_MS, &e))?;

        let status = response.status();
        let retry_after_s = retry_after_secs(response.headers());
        let text = response
            .text()
            .map_err(|e| error_for_transport(&self.label, TOTAL_TIMEOUT_MS, &e))?;
        let request_ms = started.elapsed().as_millis();

        if !status.is_success() {
            return Err(error_for_status(
                &self.label,
                status.as_u16(),
                retry_after_s,
                &text,
            ));
        }
        Ok(Transcript {
            text: parse_response(&self.label, &text)?,
            cleaned: None,
            request_ms,
        })
    }

    fn label(&self) -> &str {
        &self.label
    }
}

/// Parse a Retry-After header value in seconds form (HTTP-date form is rare on
/// these APIs and not worth the dependency).
pub(crate) fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}
