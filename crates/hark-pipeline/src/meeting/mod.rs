//! Meeting mode, worker side (plan §4): record a call's two channels, show a
//! live Me/Them transcript, then refine, summarize and archive it once the call
//! ends. Everything here runs on worker threads; the app sees only
//! [`MeetingEvent`]s and sends [`MeetingHandle`] commands.
//!
//! Threads, and why each exists:
//! - the **coordinator** (`coordinator.rs`) owns the detector and the active
//!   recording, and drains capture every 100 ms. Nothing on it blocks on the
//!   network.
//! - one **live transcriber** per meeting (`live.rs`) runs the chunks through
//!   the dictation provider, FIFO across both channels (so each channel stays
//!   sequential), and never blocks capture.
//! - one **finisher** per meeting (`finish.rs`) does the slow after-call work:
//!   the Deepgram final pass, the summary, the MP3 archive. A new meeting can
//!   start while an older one is still finishing.
//!
//! Meeting mode never touches the push-to-talk pipeline: its own capture
//! streams, its own state machine, its own threads. Dictation keeps working
//! during a meeting and never counts as one.
//!
//! Content hygiene as everywhere else: segments, notes and titles travel in
//! events to the UI and the database only. None of these types derive
//! `Debug`, and no log line carries text or audio.

mod coordinator;
mod finish;
mod live;
mod recorder;
mod rerun;

pub use coordinator::{run, MeetingHandle};
pub use finish::DEEPGRAM_ACCOUNT;
pub use hark_meeting::Answer;

/// How a meeting started. Stored with the meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Manual,
    /// Detected, and the user accepted the prompt.
    Ask,
    /// Detected and started without asking (auto mode).
    Auto,
}

impl Trigger {
    pub fn label(self) -> &'static str {
        match self {
            Trigger::Manual => "manual",
            Trigger::Ask => "ask",
            Trigger::Auto => "auto",
        }
    }
}

/// One transcript line. `channel` 0 = Me, 1 = Them; `speaker` is the
/// diarized speaker within Them after the final pass (0-based).
pub struct LiveSegment {
    pub channel: u8,
    pub speaker: Option<u32>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Everything the app needs to create the meeting's row and show it live.
pub struct StartedMeeting {
    pub id: String,
    /// Unix ms.
    pub started_ms: i64,
    pub trigger: Trigger,
    /// The detected app's id, when detection started it.
    pub app: Option<String>,
    /// The live provider's label, stored with the meeting.
    pub stt_provider: String,
    /// `false` when only the microphone could be captured.
    pub system_audio: bool,
}

/// Worker -> UI. Also the app's cue for database writes (it owns the store).
pub enum MeetingEvent {
    /// Ask mode: "<name> is using your mic. Take meeting notes?"
    Prompt {
        app: String,
        name: String,
    },
    /// The prompted app let go of the mic, or a manual start made it moot.
    PromptRetracted,
    Started(StartedMeeting),
    Segment {
        id: String,
        segment: LiveSegment,
    },
    /// The detected meeting's app released the mic: notes stop at `at_ms`
    /// (unix ms) unless it takes the mic back. `app` is the display name.
    AutoStopPending {
        id: String,
        at_ms: i64,
        app: String,
    },
    /// The app took the mic back before the stop: recording goes on.
    AutoStopCancelled {
        id: String,
    },
    /// Capture stopped; the meeting is finishing (final pass, notes, archive).
    Stopped {
        id: String,
        ended_ms: i64,
    },
    /// The final pass replaced the live transcript.
    Refined {
        id: String,
        segments: Vec<LiveSegment>,
    },
    /// Explicit reprocessing succeeded; replace segments and reset speaker renames.
    Reprocessed {
        id: String,
        segments: Vec<LiveSegment>,
    },
    ReprocessFinished {
        id: String,
        error: Option<String>,
    },
    /// Notes JSON (`hark_voice::MeetingNotes`) and the suggested title.
    Notes {
        id: String,
        notes_json: String,
        title: String,
    },
    /// Finishing is complete; `audio_bytes` is what the meeting's audio takes on disk.
    Finished {
        id: String,
        audio_bytes: u64,
    },
    /// Something the user should know that did not stop the meeting (no
    /// system audio, final pass failed, no summary). Labels only, no content.
    Notice {
        id: Option<String>,
        text: String,
    },
    /// The meeting could not be kept (nothing was recorded).
    Failed {
        id: Option<String>,
        detail: String,
    },
    /// Re-check the storage cap now; never evict these ids.
    EnforceAudioCap {
        protected: Vec<String>,
    },
}

/// A friendly name for a detected app id, for the prompt.
pub fn app_display_name(app: &str) -> String {
    let name = match app {
        "msteams_8wekyb3d8bbwe" | "teams.exe" | "ms-teams.exe" => "Teams",
        "zoom.exe" => "Zoom",
        "webex.exe" | "ciscocollabhost.exe" => "Webex",
        "slack.exe" => "Slack",
        "discord.exe" => "Discord",
        "goto.exe" => "GoTo",
        "ringcentral.exe" => "RingCentral",
        "chrome.exe" => "Chrome",
        "msedge.exe" => "Edge",
        "firefox.exe" => "Firefox",
        "brave.exe" => "Brave",
        other => return other.trim_end_matches(".exe").to_string(),
    };
    name.to_string()
}

/// Whether meeting capture exists on this platform (D4: Windows first; the
/// macOS process tap comes later; Linux is deferred). The UI hides Meetings
/// where this is false.
pub fn meetings_supported() -> bool {
    cfg!(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_apps_get_friendly_names_and_unknown_ones_lose_exe() {
        assert_eq!(app_display_name("msteams_8wekyb3d8bbwe"), "Teams");
        assert_eq!(app_display_name("zoom.exe"), "Zoom");
        assert_eq!(app_display_name("whereby.exe"), "whereby");
    }

    #[test]
    fn trigger_labels_match_the_store_contract() {
        assert_eq!(
            [Trigger::Manual, Trigger::Ask, Trigger::Auto].map(Trigger::label),
            ["manual", "ask", "auto"]
        );
    }
}
