//! How the model is asked to render the transcript, and what each mode's
//! answer means: the SMART refusal check and the [`Transcript`] a finished
//! session reports.

use super::fail;
use crate::error::SttError;
use crate::Transcript;

/// How the model is asked to render the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscribeMode {
    /// Literal transcript, fillers and self-corrections intact. Hark's default:
    /// it keeps `Transcript::text` a true verbatim record, which the spellbook
    /// corrector, the invocation matcher and the cleanup expansion guard all
    /// compare against.
    Verbatim,
    /// The provider removes disfluencies and formats the text in the same
    /// round trip. Fills `Transcript::cleaned`, and the pipeline then skips its
    /// own cleanup call.
    ///
    /// **It obeys spoken edit commands, and can strand a turn doing it.**
    /// "Scratch that" (and, in one user's voice, "remove it") is applied as an
    /// edit rather than transcribed. When the edit leaves nothing, the server
    /// sends `ACTIVITY_END` and then goes silent: no final transcript, no
    /// `generationComplete`, ever. Verified against the real API on
    /// 2026-09-24; the same clip in VERBATIM transcribes in under a second.
    /// Hence the pipeline never replays a failed stream in this mode.
    ///
    /// **It also answers dictations instead of transcribing them.** A
    /// dictation that sounds like a request ("Add the following MCP server")
    /// came back as Gemini's canned "I'm just a language model and can't help
    /// with that." — reported 2026-10-15, alongside a dropped opener ("So
    /// basically,") and an obeyed instruction ("Drop all the other bullet
    /// points."). The last two are SMART doing its job and cannot be told
    /// apart from good cleanup; the refusal can, see [`checked_text`].
    Smart,
}

impl TranscribeMode {
    /// The wire value for `inputAudioTranscription.mode`.
    pub fn wire(self) -> &'static str {
        match self {
            TranscribeMode::Verbatim => "VERBATIM",
            TranscribeMode::Smart => "SMART",
        }
    }
}

/// Whether `text` is Gemini declining a request rather than anyone's words.
///
/// Needs both halves — the model describing itself and the model declining —
/// and only a short utterance: a long dictation that mentions language models
/// is somebody talking about them, not the canned reply.
pub fn is_canned_refusal(text: &str) -> bool {
    /// The longest canned refusal seen is 24 words.
    const MAX_WORDS: usize = 40;
    const SELF: &[&str] = &[
        "language model",
        "text-based ai",
        "as an ai",
        "i'm an ai",
        "i am an ai",
        "generate text",
    ];
    const DECLINE: &[&str] = &[
        "can't help",
        "cannot help",
        "can't assist",
        "cannot assist",
        "unable to help",
        "unable to assist",
        "not able to help",
        "not able to assist",
        "don't have the capacity",
        "don't have the ability",
    ];
    if text.split_whitespace().count() > MAX_WORDS {
        return false;
    }
    let lower = text.to_lowercase().replace('\u{2019}', "'");
    SELF.iter().any(|p| lower.contains(p)) && DECLINE.iter().any(|p| lower.contains(p))
}

/// The session's text, or an error when SMART answered instead of transcribing.
///
/// An error rather than text because the pipeline answers a failed stream by
/// replaying the clip in VERBATIM, which returns what was actually said. That
/// makes a false match cheap — one replay, and the same words come back —
/// while a missed one injects a sentence the user never spoke. VERBATIM text is
/// never second-guessed: there it is someone dictating that sentence.
pub fn checked_text(mode: TranscribeMode, text: String) -> Result<String, SttError> {
    if mode == TranscribeMode::Smart && is_canned_refusal(&text) {
        log::warn!("gemini live: SMART answered the dictation instead of transcribing it");
        return Err(fail(
            "SMART mode answered the dictation instead of transcribing it".to_string(),
        ));
    }
    Ok(text)
}

/// Build the [`Transcript`] for a finished session.
///
/// In SMART mode the provider already removed disfluencies and formatted the
/// text, so the one string it returned is the cleaned one. It is reported in
/// **both** fields deliberately: `text` because the pipeline injects and
/// records something either way, and `cleaned` because that is the flag the
/// worker reads to skip its own cleanup call. Callers that need a guaranteed
/// verbatim record must use [`TranscribeMode::Verbatim`], which is the default.
pub fn transcript_for(mode: TranscribeMode, text: String, request_ms: u128) -> Transcript {
    match mode {
        TranscribeMode::Verbatim => Transcript {
            text,
            cleaned: None,
            request_ms,
        },
        TranscribeMode::Smart => Transcript {
            cleaned: Some(text.clone()),
            text,
            request_ms,
        },
    }
}
