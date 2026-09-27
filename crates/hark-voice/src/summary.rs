//! Meeting notes: one long-context BYOK chat-completions call turns a full
//! meeting transcript into structured notes (title, summary, key points,
//! decisions, action items). No map-reduce (plan D7): an hour of talk fits
//! comfortably in one request, so the caller sends the whole transcript and
//! this module asks for JSON back.
//!
//! Same discipline as `openai_compatible`: pure request-building and
//! response-parsing functions, with `summarize` as the thin I/O shell that
//! calls them. Reuses the chat-completions URL, response parsing, and
//! status/transport error mapping from `openai_compatible`/`error` rather
//! than duplicating them; only the notes-specific prompt, JSON schema, and
//! validation live here.

use crate::error::truncate_snippet;
use crate::openai_compatible::{
    chat_completions_url, max_completion_tokens, parse_response, retry_after_secs,
};
use crate::{error_for_status, error_for_transport, CleanupError};
use reqwest::blocking::Client;
use std::time::Duration;

/// Reused as the notes error taxonomy: a bad or empty summary response is a
/// `Provider` error exactly like an unparseable cleanup response, and
/// transport/status mapping is identical. Two near-identical enums would be
/// premature abstraction avoidance taken too far in the other direction.
pub type SummaryError = CleanupError;

/// Provider tag used for errors raised by pure parsing (`parse_notes`,
/// `MeetingNotes::from_json`), which have no live request to attribute to a
/// specific configured provider.
const PARSE_ERROR_TAG: &str = "summary";

/// Per-request total timeout for the summary call (`RequestBuilder::timeout`).
/// Far longer than `CLEANUP_TIMEOUT_MS`: this is one call after the meeting
/// ends, not on the release-to-inject hot path, and a long transcript can
/// take the provider tens of seconds to read and reason over.
pub const SUMMARY_TIMEOUT_MS: u64 = 120_000;

/// Built-in system-prompt instructions: what to extract, the expected tone,
/// the "never invent facts" guardrail, and the instruction to reuse speaker
/// labels exactly as they appear in the transcript. Callers may substitute
/// their own template (config `summary_template`, per §4.4/§4.6); the JSON
/// schema clause below is appended to either one, so parsing never depends
/// on the user's wording.
pub const DEFAULT_SUMMARY_TEMPLATE: &str = "You are turning a meeting transcript into notes for \
     the person who attended. Extract a short, descriptive title, a plain-language summary of \
     what was discussed, the key points, any decisions that were made, and any action items \
     (with an owner when one was stated). Use the speaker labels exactly as they appear in the \
     transcript, such as \"Me\", \"Speaker 2\", or a name the user has already assigned; never \
     invent, guess, or reassign a speaker's identity. Never invent facts, numbers, dates, or \
     commitments that are not in the transcript: if something was not discussed, leave the \
     corresponding field empty rather than filling it in. Keep the tone neutral and factual, \
     matching the register of the meeting rather than embellishing it. Respond in JSON.";

/// Appended to every system prompt, default or user-supplied, so the response
/// shape never depends on what the template said (or forgot to say) about
/// formatting. Deliberately restates the schema in prose: `response_format`
/// asks the provider to emit valid JSON but says nothing about which keys it
/// contains, and not every OpenAI-compatible endpoint honors that field.
const JSON_OUTPUT_CLAUSE: &str = "Respond with a single JSON object and nothing else: no \
     commentary, no markdown code fence, and no text before or after it. The object must have \
     exactly these keys: \"title\" (a short string), \"summary\" (a string), \"key_points\" (an \
     array of strings), \"decisions\" (an array of strings), and \"action_items\" (an array of \
     objects, each with \"text\" (a string), \"owner\" (a string, or null when no owner was \
     stated), and \"done\" (a boolean, false unless the transcript says the item is already \
     done)). Use an empty array for any list with nothing to report.";

/// Notes extracted from one meeting transcript. Stored as-is in
/// `meetings.notes_json` (§4.5) via [`MeetingNotes::to_json`]/`from_json`,
/// and rendered with action-item checkboxes (§4.7).
///
/// No derived `Debug`: transcript-derived content must never ride a
/// reflexive `{notes:?}` into a log line. See the hand-written impl below.
#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeetingNotes {
    pub title: String,
    pub summary: String,
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub action_items: Vec<ActionItem>,
}

impl std::fmt::Debug for MeetingNotes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeetingNotes")
            .field("title_chars", &self.title.chars().count())
            .field("summary_chars", &self.summary.chars().count())
            .field("key_points", &self.key_points.len())
            .field("decisions", &self.decisions.len())
            .field("action_items", &self.action_items.len())
            .finish()
    }
}

impl MeetingNotes {
    /// Serialize for storage (`meetings.notes_json`). Plain strings, bools,
    /// and vecs of strings cannot fail JSON serialization.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("MeetingNotes serialization cannot fail")
    }

    /// Deserialize a previously stored record. Distinct from [`parse_notes`]:
    /// this is our own prior output (already validated once), not a raw
    /// model response, so it does not re-run truncation or the empty-summary
    /// rejection -- only shape validation.
    pub fn from_json(s: &str) -> Result<MeetingNotes, SummaryError> {
        serde_json::from_str(s).map_err(|e| SummaryError::Provider {
            provider: PARSE_ERROR_TAG.to_string(),
            detail: format!("stored notes JSON is invalid ({e})"),
        })
    }
}

/// One action item. `owner` is `None` when the transcript named no one;
/// `done` is always `false` coming out of a summary call (nothing marks an
/// item done before it exists) but round-trips through storage once the UI
/// checkbox (§4.7) sets it.
#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActionItem {
    pub text: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub done: bool,
}

impl std::fmt::Debug for ActionItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionItem")
            .field("text_chars", &self.text.chars().count())
            .field("has_owner", &self.owner.is_some())
            .field("done", &self.done)
            .finish()
    }
}

/// Everything needed to build one summary call. Mirrors `CleanupConfig`'s
/// model-quirk fields (same types, same "explicit wins, else the provider
/// preset" resolution upstream in hark-config) but carries no voice/spellbook
/// fields: notes have no protected-terms clause and no over-expansion guard.
#[derive(Clone)]
pub struct SummaryConfig {
    /// Short human label for logs and errors ("openai", "groq").
    pub label: String,
    /// e.g. "https://api.openai.com/v1"; shares the chat-completions contract
    /// with cleanup and STT.
    pub base_url: String,
    /// e.g. "gpt-5-nano".
    pub model: String,
    /// From the keychain. Never logged.
    pub api_key: String,
    /// Serialized into the request only when present (GPT-5 family rejects
    /// any non-default temperature).
    pub temperature: Option<f32>,
    /// Serialized only when present (OpenAI GPT-5 family only).
    pub reasoning_effort: Option<String>,
}

// Deliberately no derived Debug: same reasoning as `CleanupConfig` -- a
// reflexive `{config:?}` must not be able to leak `api_key`.
impl std::fmt::Debug for SummaryConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummaryConfig")
            .field("label", &self.label)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[derive(serde::Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(serde::Serialize)]
struct SummaryChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(serde::Serialize)]
struct SummaryChatRequest<'a> {
    model: &'a str,
    messages: [SummaryChatMessage<'a>; 2],
    max_completion_tokens: u32,
    // Both OpenAI and Groq's chat-completions contract accept this field;
    // unlike temperature/reasoning_effort it has no known model quirk, so
    // (like `max_completion_tokens`) it is sent unconditionally rather than
    // per-preset. `JSON_OUTPUT_CLAUSE` makes parsing work even on an
    // OpenAI-compatible endpoint that silently ignores it.
    response_format: ResponseFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
}

/// Assemble the complete JSON request body: the built-in or user template
/// plus the fixed output-schema clause as the system message, the full
/// transcript as the single user message. Pure so tests can assert on the
/// exact fields without a network.
pub fn build_summary_request(
    cfg: &SummaryConfig,
    transcript: &str,
    template: Option<&str>,
) -> serde_json::Value {
    let system = format!(
        "{}\n\n{}",
        template.unwrap_or(DEFAULT_SUMMARY_TEMPLATE),
        JSON_OUTPUT_CLAUSE
    );
    let request = SummaryChatRequest {
        model: &cfg.model,
        messages: [
            SummaryChatMessage {
                role: "system",
                content: &system,
            },
            SummaryChatMessage {
                role: "user",
                content: transcript,
            },
        ],
        max_completion_tokens: max_completion_tokens(transcript),
        response_format: ResponseFormat {
            kind: "json_object",
        },
        temperature: cfg.temperature,
        reasoning_effort: cfg.reasoning_effort.as_deref(),
    };
    serde_json::to_value(&request).expect("summary request serialization cannot fail")
}

/// Title cap in characters (`MeetingNotes::title`).
const TITLE_MAX_CHARS: usize = 120;
/// Item cap for `key_points`, `decisions`, and `action_items`.
const LIST_MAX_ITEMS: usize = 50;
/// Per-item cap in characters (`key_points`/`decisions` entries, and
/// `ActionItem::text`/`owner`).
const ITEM_MAX_CHARS: usize = 2000;
/// Cap in characters for `MeetingNotes::summary`.
const SUMMARY_MAX_CHARS: usize = 8000;

/// Truncate to at most `max_chars` Unicode scalar values. Never panics on
/// multibyte text: `char_indices`/`chars` walk whole characters, so the cut
/// point is always a valid byte boundary.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

/// Trim, truncate, and drop-if-empty a list of free-text items, then cap the
/// list length. Shared by `key_points` and `decisions`.
fn clean_list(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .filter_map(|s| {
            let trimmed = truncate_chars(s.trim(), ITEM_MAX_CHARS);
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .take(LIST_MAX_ITEMS)
        .collect()
}

#[derive(serde::Deserialize, Default)]
struct RawNotes {
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    key_points: Vec<String>,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    action_items: Vec<RawActionItem>,
}

#[derive(serde::Deserialize, Default)]
struct RawActionItem {
    #[serde(default)]
    text: String,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    done: bool,
}

/// Find the outermost `{...}` in `content` and return it as a `&str` slice,
/// tolerating a raw JSON object, a ```json fenced block, or prose around one
/// JSON object. Brace-balances while respecting quoted strings (so a `}`
/// inside a string value cannot close the object early) rather than assuming
/// the first `{`/last `}` in the whole text form a matching pair.
fn extract_json_object(content: &str) -> Result<&str, SummaryError> {
    let start = content.find('{').ok_or_else(|| SummaryError::Provider {
        provider: PARSE_ERROR_TAG.to_string(),
        detail: format!(
            "no JSON object found in response: {}",
            truncate_snippet(content)
        ),
    })?;

    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in content[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + i + ch.len_utf8();
                    return Ok(&content[start..end]);
                }
            }
            _ => {}
        }
    }
    Err(SummaryError::Provider {
        provider: PARSE_ERROR_TAG.to_string(),
        detail: format!(
            "unterminated JSON object in response: {}",
            truncate_snippet(content)
        ),
    })
}

/// Parse and validate one notes response: extract the JSON object (raw,
/// fenced, or embedded in prose), decode it tolerating missing arrays
/// (default to empty), then trim, cap, and drop empty items. Rejects a
/// response whose summary is empty after trimming -- every other field is
/// allowed to come back empty (a short meeting may have no decisions), but
/// an empty summary means the call produced nothing usable and the pipeline
/// should treat it as a failure rather than store blank notes.
pub fn parse_notes(content: &str) -> Result<MeetingNotes, SummaryError> {
    let json_str = extract_json_object(content)?;
    let raw: RawNotes = serde_json::from_str(json_str).map_err(|e| SummaryError::Provider {
        provider: PARSE_ERROR_TAG.to_string(),
        detail: format!(
            "unexpected notes body ({e}): {}",
            truncate_snippet(json_str)
        ),
    })?;

    let summary = truncate_chars(raw.summary.trim(), SUMMARY_MAX_CHARS);
    if summary.is_empty() {
        return Err(SummaryError::Provider {
            provider: PARSE_ERROR_TAG.to_string(),
            detail: "notes response had an empty summary".to_string(),
        });
    }

    let action_items = raw
        .action_items
        .into_iter()
        .filter_map(|item| {
            let text = truncate_chars(item.text.trim(), ITEM_MAX_CHARS);
            if text.is_empty() {
                return None;
            }
            let owner = item
                .owner
                .map(|o| truncate_chars(o.trim(), ITEM_MAX_CHARS))
                .filter(|o| !o.is_empty());
            Some(ActionItem {
                text,
                owner,
                done: item.done,
            })
        })
        .take(LIST_MAX_ITEMS)
        .collect();

    Ok(MeetingNotes {
        title: truncate_chars(raw.title.trim(), TITLE_MAX_CHARS),
        summary,
        key_points: clean_list(raw.key_points),
        decisions: clean_list(raw.decisions),
        action_items,
    })
}

/// Call the provider and return validated notes. **No retry**: unlike
/// cleanup, there is no graceful fallback for a failed summary (there is no
/// "uncleaned notes" to show instead), but this runs once after the meeting
/// ends, off any latency-sensitive path, so a doubled worst case here costs
/// nothing the way it would on the dictation hot path.
pub fn summarize(
    client: &Client,
    cfg: &SummaryConfig,
    transcript: &str,
    template: Option<&str>,
) -> Result<MeetingNotes, SummaryError> {
    let url = chat_completions_url(&cfg.base_url);
    let body = build_summary_request(cfg, transcript, template);
    let body = serde_json::to_vec(&body).expect("summary request re-serialization cannot fail");

    let response = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .timeout(Duration::from_millis(SUMMARY_TIMEOUT_MS))
        .body(body)
        .send()
        .map_err(|e| error_for_transport(&cfg.label, SUMMARY_TIMEOUT_MS, &e))?;

    let status = response.status();
    let retry_after_s = retry_after_secs(response.headers());
    let body_text = response
        .text()
        .map_err(|e| error_for_transport(&cfg.label, SUMMARY_TIMEOUT_MS, &e))?;

    if !status.is_success() {
        return Err(error_for_status(
            &cfg.label,
            status.as_u16(),
            retry_after_s,
            &body_text,
        ));
    }

    let content = parse_response(&cfg.label, &body_text)?;
    parse_notes(&content)
}
