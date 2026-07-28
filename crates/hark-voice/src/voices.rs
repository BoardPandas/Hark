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
    Grammar,
    Clean,
    Professional,
    Casual,
    Notes,
    Concise,
    Direct,
    Plain,
    Pirate,
    Custom,
}

impl Voice {
    /// Every valid name, in display order (the CLI's error message and the
    /// config docs both derive from this).
    pub const NAMES: [&'static str; 11] = [
        "verbatim",
        "grammar",
        "clean",
        "professional",
        "casual",
        "notes",
        "concise",
        "direct",
        "plain",
        "pirate",
        "custom",
    ];

    pub fn name(self) -> &'static str {
        match self {
            Voice::Verbatim => "verbatim",
            Voice::Grammar => "grammar",
            Voice::Clean => "clean",
            Voice::Professional => "professional",
            Voice::Casual => "casual",
            Voice::Notes => "notes",
            Voice::Concise => "concise",
            Voice::Direct => "direct",
            Voice::Plain => "plain",
            Voice::Pirate => "pirate",
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
            "grammar" => Ok(Voice::Grammar),
            "clean" => Ok(Voice::Clean),
            "professional" => Ok(Voice::Professional),
            "casual" => Ok(Voice::Casual),
            "notes" => Ok(Voice::Notes),
            "concise" => Ok(Voice::Concise),
            "direct" => Ok(Voice::Direct),
            "plain" => Ok(Voice::Plain),
            "pirate" => Ok(Voice::Pirate),
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

/// Spellbook terms that actually appear in `text` (case-insensitive
/// containment), in spellbook order. Keeps the protected-terms clause tiny
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

/// Appended to every built-in voice prompt (never to Custom). Keeps the cleanup
/// model from emitting em/en dashes, which read as machine-generated and are a
/// nuisance to retype; pure substitution, so it costs nothing against the
/// over-expansion guard.
pub const PUNCTUATION_CLAUSE: &str = "Do not use em dashes or en dashes. Use a comma, a period, \
     or a plain hyphen instead.";

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

// The lightest built-in: a proofreader, not an editor. Every other voice is
// allowed to change wording; this one is not, so the instruction spends its
// words on what must NOT change rather than on what to fix.
const GRAMMAR_INSTRUCTION: &str = "Correct the grammar of the transcript below and change nothing \
     else. Fix grammatical errors (verb tense, subject-verb agreement, plurals, articles, \
     pronoun case), misspellings, punctuation, and capitalization. Do not rewrite, rephrase, or \
     restructure anything: keep the speaker's exact words and word order, keep every sentence as \
     its own sentence without merging or splitting, and never swap a word for a synonym or a more \
     formal equivalent. Leave informal phrasing, filler words, and repetition exactly as spoken. \
     If a sentence is already grammatical, return it untouched.";

const CLEAN_INSTRUCTION: &str = "Rewrite the transcript below. Fix punctuation, capitalization, \
     filler words (um, uh, you know), false starts, and repeated words. Keep the speaker's own \
     wording, meaning, and tone.";

const PROFESSIONAL_INSTRUCTION: &str = "Rewrite the transcript below in a polished, professional \
     business register suitable for a written message to a colleague. Adjust word choice and \
     formality only, and fix filler words and false starts. Keep the meaning.";

const CASUAL_INSTRUCTION: &str = "Rewrite the transcript below in a relaxed, casual \
     conversational register. Adjust word choice only, and fix filler words and false starts \
     while keeping it informal. Keep the meaning.";

const NOTES_INSTRUCTION: &str = "Rewrite the transcript below as concise first-person work \
     notes, the way a technician logs a ticket. Use short declarative sentences or fragments \
     and past tense, and drop conversational filler and pleasantries. Fix filler words and \
     false starts. Keep every name, number, and technical detail exactly, and keep the meaning.";

const CONCISE_INSTRUCTION: &str = "Rewrite the transcript below to be tighter and less \
     repetitive. Remove redundancy, restatements, and filler while keeping the speaker's own \
     wording and every fact. Do not add anything or change the meaning.";

const DIRECT_INSTRUCTION: &str = "Rewrite the transcript below to be direct and to the point. \
     Lead with the main point, cut hedging and warm-up phrases, and fix filler words and false \
     starts. Keep the facts and the meaning.";

const PLAIN_INSTRUCTION: &str = "Rewrite the transcript below in plain, courteous language for \
     a non-technical reader. Keep it clear and neutral and fix filler words and false starts, \
     but do not add explanations, definitions, or context the speaker did not give. Keep the \
     meaning and all specifics.";

// Novelty voice, revealed only through the Settings unlock. Kept length-matched
// so the shared over-expansion guard does not discard its rewrites.
const PIRATE_INSTRUCTION: &str = "Rewrite the transcript below in the voice of a jolly pirate. \
     Swap in pirate diction (aye, arr, matey, ye, be) and a little nautical color, but keep it \
     readable and keep every name, number, and fact intact. Match the original length and do \
     not add new sentences or ideas.";

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
        Voice::Grammar => GRAMMAR_INSTRUCTION,
        Voice::Clean => CLEAN_INSTRUCTION,
        Voice::Professional => PROFESSIONAL_INSTRUCTION,
        Voice::Casual => CASUAL_INSTRUCTION,
        Voice::Notes => NOTES_INSTRUCTION,
        Voice::Concise => CONCISE_INSTRUCTION,
        Voice::Direct => DIRECT_INSTRUCTION,
        Voice::Plain => PLAIN_INSTRUCTION,
        Voice::Pirate => PIRATE_INSTRUCTION,
        Voice::Custom => custom_prompt,
    };
    let mut prompt = instruction.to_string();
    // Custom is the escape hatch: a user who writes "turn this into an email"
    // means it, so neither the length/punctuation clauses nor `over_expanded`
    // apply there.
    if voice != Voice::Custom {
        prompt.push(' ');
        prompt.push_str(LENGTH_DISCIPLINE_CLAUSE);
        prompt.push(' ');
        prompt.push_str(PUNCTUATION_CLAUSE);
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
