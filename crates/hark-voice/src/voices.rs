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

/// Appended to every built-in voice prompt (never to Custom, which is the
/// user's own text). Split out from the per-voice instructions because the
/// length rule is identical for all of them and is the one clause that most
/// needs to stay verbatim: "preserve the meaning" reads as permission to
/// elaborate, so the budget has to be stated as a quantity. No ratio number
/// appears here on purpose, so this text cannot drift out of sync with
/// `voice.max_expansion_ratio`; the exact bound is enforced by
/// `over_expanded` after the response arrives.
pub const LENGTH_DISCIPLINE_CLAUSE: &str = "You are editing a spoken transcript, not writing \
     prose. Return about the same number of words as the input: a five-word transcript comes \
     back about five words. Never add sentences, ideas, greetings, sign-offs, or context the \
     speaker did not say, and never expand a short remark into a paragraph or a list. If the \
     transcript is already clean, return it unchanged.";

const CLEAN_INSTRUCTION: &str = "Rewrite the transcript below. Fix punctuation, capitalization, \
     filler words (um, uh, you know), false starts, and repeated words. Keep the speaker's own \
     wording, meaning, and tone.";

const PROFESSIONAL_INSTRUCTION: &str = "Rewrite the transcript below in a polished, professional \
     business register suitable for a written message to a colleague. Adjust word choice and \
     formality only, and fix filler words and false starts. Keep the meaning.";

const CASUAL_INSTRUCTION: &str = "Rewrite the transcript below in a relaxed, casual \
     conversational register. Adjust word choice only, and fix filler words and false starts \
     while keeping it informal. Keep the meaning.";

/// Absolute slack allowed on top of `max_ratio`, in words. Without it the
/// ratio alone is unusably tight on short utterances, where a legitimate tidy
/// genuinely does add words ("yeah sounds good" -> "Yes, that sounds good.");
/// with it, a five-word transcript may come back as eight but not as a
/// paragraph. This is what keeps the guard live at the lengths the ratio
/// cannot police, so short dictations are covered rather than exempt.
pub const EXPANSION_GRACE_WORDS: f32 = 3.0;

/// True when `output` is too long to be an edit of `input` and should be
/// discarded in favor of the uncleaned transcript. The allowance is the
/// larger of `max_ratio` x input words and input words + [`EXPANSION_GRACE_WORDS`].
///
/// `max_ratio == 0.0` disables the check (same convention as
/// `skip_below_words == 0`), as does any non-finite ratio, which can reach
/// here from a hand-edited TOML `nan`.
pub fn over_expanded(input: &str, output: &str, max_ratio: f32) -> bool {
    if !max_ratio.is_finite() || max_ratio <= 0.0 {
        return false;
    }
    let input_words = input.split_whitespace().count() as f32;
    let output_words = output.split_whitespace().count() as f32;
    let allowed = (input_words * max_ratio).max(input_words + EXPANSION_GRACE_WORDS);
    output_words > allowed
}

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
    // Custom is the escape hatch: a user who writes "turn this into an email"
    // means it, so neither the clause nor `over_expanded` applies there.
    if voice != Voice::Custom {
        prompt.push(' ');
        prompt.push_str(LENGTH_DISCIPLINE_CLAUSE);
    }
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
