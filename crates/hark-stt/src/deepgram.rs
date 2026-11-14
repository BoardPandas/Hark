//! The Deepgram adapter: `POST {base_url}/v1/listen` with `Token` auth and a
//! raw `audio/wav` body. Earns its own adapter because `keyterm` biasing
//! (nova-3+) maps directly onto Hark's spellbook feature.

use crate::error::{error_for_status, error_for_transport, json_error_detail, SttError};
use crate::openai_compatible::retry_after_secs;
use crate::{request_total_ms, ProviderConfig, SttProvider, Transcript};
use reqwest::blocking::Client;
use std::time::{Duration, Instant};

pub struct Deepgram {
    client: Client,
    label: String,
    url: String,
    api_key: String,
    /// `None`: scale each request to its clip (see [`crate::dictation_timeout_ms`]).
    fixed_total_ms: Option<u64>,
}

impl Deepgram {
    /// Hold every request to exactly `total_ms` instead of scaling to the
    /// clip; `None` keeps the dictation default.
    pub fn with_total_timeout(mut self, total_ms: Option<u64>) -> Self {
        self.fixed_total_ms = total_ms;
        self
    }

    pub fn new(config: &ProviderConfig, client: Client) -> Result<Self, SttError> {
        Ok(Self {
            client,
            label: config.label.clone(),
            url: listen_url(&config.base_url, &config.model, &config.bias_terms)?,
            api_key: config.api_key.clone(),
            fixed_total_ms: None,
        })
    }
}

/// Build the `/v1/listen` URL: model, smart_format, and one repeated `keyterm`
/// param per bias term (URL-encoded; multi-word terms are allowed). Pure for
/// unit tests.
pub fn listen_url(base_url: &str, model: &str, bias_terms: &[String]) -> Result<String, SttError> {
    let base = format!("{}/v1/listen", base_url.trim_end_matches('/'));
    let mut url = reqwest::Url::parse(&base).map_err(|e| SttError::Provider {
        provider: "deepgram".to_string(),
        detail: format!("invalid base_url: {e}"),
    })?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("model", model);
        q.append_pair("smart_format", "true");
        for term in bias_terms {
            q.append_pair("keyterm", term);
        }
    }
    Ok(url.into())
}

/// Extract `results.channels[0].alternatives[0].transcript`. Pure for unit tests.
pub fn parse_response(provider: &str, body: &str) -> Result<String, SttError> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| SttError::Provider {
        provider: provider.to_string(),
        detail: json_error_detail("unexpected response body", &e),
    })?;
    let parse = || -> Option<String> {
        Some(
            v.get("results")?
                .get("channels")?
                .get(0)?
                .get("alternatives")?
                .get(0)?
                .get("transcript")?
                .as_str()?
                .to_string(),
        )
    };
    parse().ok_or_else(|| SttError::Provider {
        provider: provider.to_string(),
        detail: "unexpected response shape: missing transcript string".to_string(),
    })
}

impl SttProvider for Deepgram {
    fn transcribe(&self, wav_bytes: &[u8]) -> Result<Transcript, SttError> {
        let total_ms = request_total_ms(self.fixed_total_ms, wav_bytes);
        let started = Instant::now();
        let response = self
            .client
            .post(&self.url)
            .timeout(Duration::from_millis(total_ms))
            .header("Authorization", format!("Token {}", self.api_key))
            .header("Content-Type", "audio/wav")
            .body(wav_bytes.to_vec())
            .send()
            .map_err(|e| error_for_transport(&self.label, total_ms, &e))?;

        let status = response.status();
        let retry_after_s = retry_after_secs(response.headers());
        let body = response
            .text()
            .map_err(|e| error_for_transport(&self.label, total_ms, &e))?;
        let request_ms = started.elapsed().as_millis();

        if !status.is_success() {
            return Err(error_for_status(
                &self.label,
                status.as_u16(),
                retry_after_s,
                &body,
            ));
        }
        Ok(Transcript {
            text: parse_response(&self.label, &body)?,
            cleaned: None,
            request_ms,
        })
    }

    fn label(&self) -> &str {
        &self.label
    }
}
