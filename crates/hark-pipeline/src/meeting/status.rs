//! The final-pass transcript status a meeting keeps in the database
//! (`meetings.transcript_status`, migration 006), so a pass that could not
//! run is visible after a restart instead of living only in a notice.
//!
//! Stored text: `pending` or `pending:<unix ms of the next try>` while a
//! retry is scheduled, `failed:<slug>` once Hark has stopped trying, and NULL
//! when nothing is outstanding. Labels only, never transcript content.

use hark_stt::meeting::FailureKind;

/// Why a final pass produced no transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Both channels stayed under the silence floor: nothing was uploaded.
    Silent,
    /// No Deepgram key in the keychain (or its environment override).
    NoKey,
    /// The recording is missing, unsafe to upload, or unreadable.
    NoAudio,
    /// Deepgram answered, and heard no speech.
    Empty,
    /// The HTTP client could not be built.
    Setup,
    /// The request failed; the kind says whether retrying can help.
    Stt(FailureKind),
}

impl Failure {
    /// Only a network failure is worth sending again later. Never a 4xx.
    pub fn retryable(self) -> bool {
        matches!(self, Failure::Stt(kind) if kind.retryable())
    }

    pub fn slug(self) -> &'static str {
        match self {
            Failure::Silent => "silent",
            Failure::NoKey => "no-key",
            Failure::NoAudio => "no-audio",
            Failure::Empty => "empty",
            Failure::Setup => "setup",
            Failure::Stt(kind) => kind.slug(),
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        Some(match slug {
            "silent" => Failure::Silent,
            "no-key" => Failure::NoKey,
            "no-audio" => Failure::NoAudio,
            "empty" => Failure::Empty,
            "setup" => Failure::Setup,
            "network" => Failure::Stt(FailureKind::Network),
            "auth" => Failure::Stt(FailureKind::Auth),
            "rate-limited" => Failure::Stt(FailureKind::RateLimited),
            "provider" => Failure::Stt(FailureKind::Provider),
            "bad-audio" => Failure::Stt(FailureKind::BadAudio),
            _ => return None,
        })
    }

    /// One sentence for a notice, the detail page, or a re-run error.
    pub fn message(self) -> &'static str {
        match self {
            Failure::Silent => "This recording is silent; Hark captured no audio from either side.",
            Failure::NoKey => {
                "No Deepgram key. Add one in Settings > Meetings to transcribe this recording."
            }
            Failure::NoAudio => "The recording is missing or cannot be read.",
            Failure::Empty => "Deepgram heard no speech in this recording.",
            Failure::Setup => "Hark could not start the transcription client.",
            Failure::Stt(FailureKind::Network) => "Deepgram could not be reached.",
            Failure::Stt(FailureKind::Auth) => {
                "Deepgram refused the key. Check it in Settings > Meetings."
            }
            Failure::Stt(FailureKind::RateLimited) => {
                "Deepgram is rate limiting this key. Try again later."
            }
            Failure::Stt(FailureKind::Provider) => "Deepgram could not complete the final pass.",
            Failure::Stt(FailureKind::BadAudio) => "Deepgram could not read this recording.",
        }
    }
}

/// A meeting's persisted final-pass status. `None` (no row value) means
/// nothing is outstanding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptStatus {
    /// A retry is scheduled; `retry_at_ms` is unix ms when known.
    Pending {
        retry_at_ms: Option<i64>,
    },
    Failed(Failure),
}

impl TranscriptStatus {
    pub fn to_db(self) -> String {
        match self {
            TranscriptStatus::Pending { retry_at_ms: None } => "pending".to_string(),
            TranscriptStatus::Pending {
                retry_at_ms: Some(at),
            } => format!("pending:{at}"),
            TranscriptStatus::Failed(failure) => format!("failed:{}", failure.slug()),
        }
    }

    /// `None` for text this version does not know (a newer Hark wrote it):
    /// the page then shows no status rather than a wrong one.
    pub fn parse(text: &str) -> Option<Self> {
        if text == "pending" {
            return Some(TranscriptStatus::Pending { retry_at_ms: None });
        }
        if let Some(at) = text.strip_prefix("pending:") {
            return at.parse().ok().map(|at| TranscriptStatus::Pending {
                retry_at_ms: Some(at),
            });
        }
        text.strip_prefix("failed:")
            .and_then(Failure::from_slug)
            .map(TranscriptStatus::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Failure; 10] = [
        Failure::Silent,
        Failure::NoKey,
        Failure::NoAudio,
        Failure::Empty,
        Failure::Setup,
        Failure::Stt(FailureKind::Network),
        Failure::Stt(FailureKind::Auth),
        Failure::Stt(FailureKind::RateLimited),
        Failure::Stt(FailureKind::Provider),
        Failure::Stt(FailureKind::BadAudio),
    ];

    #[test]
    fn every_status_round_trips_through_its_stored_text() {
        let mut statuses = vec![
            TranscriptStatus::Pending { retry_at_ms: None },
            TranscriptStatus::Pending {
                retry_at_ms: Some(1_760_000_000_000),
            },
        ];
        statuses.extend(ALL.map(TranscriptStatus::Failed));
        for status in statuses {
            assert_eq!(TranscriptStatus::parse(&status.to_db()), Some(status));
        }
    }

    #[test]
    fn unknown_or_damaged_text_parses_to_nothing() {
        for text in [
            "",
            "pending:",
            "pending:soon",
            "failed:",
            "failed:gremlins",
            "done",
        ] {
            assert_eq!(TranscriptStatus::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn only_a_network_failure_is_retried() {
        let retryable: Vec<&str> = ALL
            .iter()
            .filter(|f| f.retryable())
            .map(|f| f.slug())
            .collect();
        assert_eq!(retryable, ["network"]);
    }

    #[test]
    fn the_silent_message_never_blames_the_provider() {
        assert_eq!(
            Failure::Silent.message(),
            "This recording is silent; Hark captured no audio from either side."
        );
    }
}
