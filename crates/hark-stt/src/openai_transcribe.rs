//! The gpt-transcribe adapter: multipart `POST {base_url}/audio/transcriptions`
//! with Bearer auth.
//!
//! Shares an endpoint with [`crate::openai_compatible`] but not a contract, so
//! it earns its own adapter for the same reason Deepgram did: the biasing is
//! different in kind. Whisper-family models take one free-text `prompt` capped
//! at 224 tokens, so bias terms have to be flattened into a comma-separated
//! glossary and truncated. gpt-transcribe takes repeated `keywords[]` fields —
//! discrete literal terms, no packing, no token budget — which is a direct map
//! for the spellbook, exactly like Deepgram's repeated `keyterm` params. It
//! also uses `languages[]` (plural), not Whisper's `language`.
//!
//! Multipart bodies are buffered by hand via
//! [`crate::openai_compatible::build_multipart_body`] rather than
//! `reqwest::blocking::multipart` — see that module's header for why streaming
//! multipart breaks the error taxonomy (LL-G Rust HIGH,
//! `reqwest-multipart-masks-connect-timeout-errors`).

use crate::error::{error_for_status, error_for_transport, SttError};
use crate::openai_compatible::{
    build_multipart_body, multipart_boundary, parse_response, retry_after_secs, transcriptions_url,
};
use crate::{ProviderConfig, SttProvider, Transcript, TOTAL_TIMEOUT_MS};
use reqwest::blocking::Client;
use std::time::Instant;

pub struct OpenAiTranscribe {
    client: Client,
    label: String,
    url: String,
    model: String,
    api_key: String,
    bias_terms: Vec<String>,
}

impl OpenAiTranscribe {
    pub fn new(config: &ProviderConfig, client: Client) -> Self {
        if !config.bias_terms.is_empty() {
            // Counts only: terms are user content and never appear in logs.
            log::info!("keyword bias: {} terms", config.bias_terms.len());
        }
        Self {
            client,
            label: config.label.clone(),
            url: transcriptions_url(&config.base_url),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            bias_terms: config.bias_terms.clone(),
        }
    }
}

/// The text fields of the multipart form, as (name, value) pairs in wire order.
///
/// Bias terms become one repeated `keywords[]` field each. They deliberately do
/// *not* go in `prompt`: `prompt` is free-form context describing the recording
/// ("a customer support call about account AC-42"), while `keywords` is the slot
/// for literal terms expected in the audio — which is what a spellbook entry is.
/// Nothing in Hark's settings describes the *setting* of a dictation, so
/// `prompt` is left off entirely rather than filled with a synthetic sentence.
///
/// No cap on term count, matching the Deepgram adapter's unbounded `keyterm`
/// list: the spellbook is the user's own vocabulary and silently dropping half
/// of it is worse than a slightly larger request.
pub fn form_text_fields(model: &str, bias_terms: &[String]) -> Vec<(&'static str, String)> {
    let mut fields = vec![
        ("model", model.to_string()),
        ("response_format", "json".to_string()),
        ("languages[]", "en".to_string()),
    ];
    for term in bias_terms {
        fields.push(("keywords[]", term.clone()));
    }
    fields
}

impl SttProvider for OpenAiTranscribe {
    fn transcribe(&self, wav_bytes: &[u8]) -> Result<Transcript, SttError> {
        let boundary = multipart_boundary(wav_bytes);
        let body = build_multipart_body(
            &boundary,
            &form_text_fields(&self.model, &self.bias_terms),
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
        // gpt-transcribe returns the same `{"text": ...}` envelope as the
        // Whisper-family endpoint, so the response parser is shared.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fields_carry_model_and_plural_languages() {
        let fields = form_text_fields("gpt-transcribe", &[]);
        assert!(fields.contains(&("model", "gpt-transcribe".to_string())));
        assert!(fields.contains(&("response_format", "json".to_string())));
        // Plural: the Whisper-family `language` field is a different contract.
        assert!(fields.contains(&("languages[]", "en".to_string())));
        assert!(!fields.iter().any(|(name, _)| *name == "language"));
    }

    #[test]
    fn each_bias_term_is_its_own_keywords_field() {
        let fields = form_text_fields("gpt-transcribe", &terms(&["Hark", "Levenshtein"]));
        let keywords: Vec<&String> = fields
            .iter()
            .filter(|(name, _)| *name == "keywords[]")
            .map(|(_, value)| value)
            .collect();
        assert_eq!(keywords, vec!["Hark", "Levenshtein"]);
    }

    #[test]
    fn multi_word_terms_stay_whole_and_are_never_packed_into_prompt() {
        let fields = form_text_fields("gpt-transcribe", &terms(&["premium plan", "AC-42"]));
        assert!(fields.contains(&("keywords[]", "premium plan".to_string())));
        // The Whisper adapter would comma-join these into one `prompt`; this
        // contract must not.
        assert!(!fields.iter().any(|(name, _)| *name == "prompt"));
    }

    #[test]
    fn no_terms_means_no_keywords_fields() {
        let fields = form_text_fields("gpt-transcribe", &[]);
        assert!(!fields.iter().any(|(name, _)| *name == "keywords[]"));
    }

    #[test]
    fn bias_terms_are_not_truncated_by_a_token_budget() {
        // 200 terms would blow the Whisper 224-token prompt budget; all must survive.
        let many: Vec<String> = (0..200).map(|i| format!("term{i}")).collect();
        let fields = form_text_fields("gpt-transcribe", &many);
        assert_eq!(
            fields.iter().filter(|(n, _)| *n == "keywords[]").count(),
            200
        );
    }

    #[test]
    fn body_encodes_repeated_keywords_as_separate_parts() {
        let wav = b"RIFFfake".to_vec();
        let boundary = multipart_boundary(&wav);
        let body = build_multipart_body(
            &boundary,
            &form_text_fields("gpt-transcribe", &terms(&["Hark", "Levenshtein"])),
            &wav,
        );
        let text = String::from_utf8_lossy(&body);
        assert_eq!(text.matches("name=\"keywords[]\"").count(), 2);
        assert!(text.contains("name=\"file\"; filename="));
    }
}
