//! Voices, per-request prompt assembly, and the word-count gate. All pure:
//! the adapter calls these per dictation because the protected-terms clause
//! depends on the outgoing text.

use std::fmt;
use std::str::FromStr;

/// The voice a transcript is rewritten in. `Verbatim` never makes a cleanup
/// call at all; the pipeline short-circuits before an adapter exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Verbatim,
    Clean,
    Professional,
    Casual,
    Custom,
}

impl Voice {
    /// Every valid name, in display order (the CLI's error message and the
    /// config docs both derive from this).
    pub const NAMES: [&'static str; 5] = ["verbatim", "clean", "professional", "casual", "custom"];

    pub fn name(self) -> &'static str {
        match self {
            Voice::Verbatim => "verbatim",
            Voice::Clean => "clean",
            Voice::Professional => "professional",
            Voice::Casual => "casual",
            Voice::Custom => "custom",
        }
    }
}

/// An unrecognized voice name. Display lists the valid names so callers
/// (the CLI) can print it as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownVoice(pub String);

impl fmt::Display for UnknownVoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown voice \"{}\"; valid voices: {}",
            self.0,
            Voice::NAMES.join(", ")
        )
    }
}

impl std::error::Error for UnknownVoice {}

impl FromStr for Voice {
    type Err = UnknownVoice;

    /// Case-insensitive, whitespace-tolerant. Config (`voice.default`) and
    /// the CLI (`--voice`) share this parse.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "verbatim" => Ok(Voice::Verbatim),
            "clean" => Ok(Voice::Clean),
            "professional" => Ok(Voice::Professional),
            "casual" => Ok(Voice::Casual),
            "custom" => Ok(Voice::Custom),
            _ => Err(UnknownVoice(s.trim().to_string())),
        }
    }
}

/// Word-count gate: true when `text` has fewer than `min_words` words
/// (`min_words == 0` disables the gate). Words are Unicode-whitespace-
/// separated tokens, so an exactly-at-threshold transcript is NOT skipped.
pub fn skips_cleanup(text: &str, min_words: u32) -> bool {
    if min_words == 0 {
        return false;
    }
    (text.split_whitespace().count() as u64) < u64::from(min_words)
}

/// Dictionary terms that actually appear in `text` (case-insensitive
/// containment), in dictionary order. Keeps the protected-terms clause tiny
/// for the common case: terms the user never spoke stay out of the prompt.
pub fn present_terms<'a>(text: &str, terms: &'a [String]) -> Vec<&'a str> {
    let haystack = text.to_lowercase();
    terms
        .iter()
        .filter(|t| !t.trim().is_empty() && haystack.contains(&t.to_lowercase()))
        .map(String::as_str)
        .collect()
}

/// Protected-terms token budget, chars/4 heuristic like hark-stt's
/// `prompt_from_bias_terms` (there 200 for Whisper's 224-token cap; here the
/// prompt has no model-side cap, so the budget just bounds cost).
const PROTECTED_TERMS_TOKEN_BUDGET: usize = 400;

/// Same drop rule as `prompt_from_bias_terms`: terms are included in order
/// until the budget is spent; the first term that would cross it is dropped
/// along with everything after it (order is the user's priority signal).
fn budgeted_terms<'a>(present: &[&'a str]) -> Vec<&'a str> {
    let budget_chars = PROTECTED_TERMS_TOKEN_BUDGET * 4;
    let mut kept = Vec::new();
    let mut chars = 0;
    for term in present {
        let added = term.chars().count() + if kept.is_empty() { 0 } else { 2 };
        if chars + added > budget_chars {
            break;
        }
        kept.push(*term);
        chars += added;
    }
    kept
}

/// The closing instruction every voice prompt ends with.
pub const RETURN_ONLY_CLAUSE: &str =
    "Return only the rewritten text, with no commentary and no surrounding quotes.";

const CLEAN_INSTRUCTION: &str = "Rewrite the transcript below. Fix punctuation, capitalization, \
     filler words (um, uh, you know), false starts, and repeated words. Preserve the original \
     wording, meaning, and tone. Never add or remove content.";

const PROFESSIONAL_INSTRUCTION: &str = "Rewrite the transcript below in a polished, professional \
     business register suitable for a written message to a colleague. Preserve the meaning; \
     never add or remove content.";

const CASUAL_INSTRUCTION: &str = "Rewrite the transcript below in a relaxed, casual \
     conversational register. Fix filler words and false starts but keep it informal. Preserve \
     the meaning; never add or remove content.";

/// Assemble the per-request system prompt (§2.2 shape: voice instruction,
/// protected-terms clause for terms present in the outgoing text, return-only
/// close). `None` for Verbatim, which never calls. `custom_prompt` is the
/// user's text, used verbatim, only for `Voice::Custom`. Prompts are user
/// content: they may ride the request body but must never be logged.
pub fn system_prompt(voice: Voice, custom_prompt: &str, present_terms: &[&str]) -> Option<String> {
    let instruction = match voice {
        Voice::Verbatim => return None,
        Voice::Clean => CLEAN_INSTRUCTION,
        Voice::Professional => PROFESSIONAL_INSTRUCTION,
        Voice::Casual => CASUAL_INSTRUCTION,
        Voice::Custom => custom_prompt,
    };
    let mut prompt = instruction.to_string();
    let kept = budgeted_terms(present_terms);
    if !kept.is_empty() {
        prompt.push_str(" Leave these terms exactly as written: ");
        prompt.push_str(&kept.join(", "));
        prompt.push('.');
    }
    prompt.push(' ');
    prompt.push_str(RETURN_ONLY_CLAUSE);
    Some(prompt)
}
