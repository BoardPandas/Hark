//! The `[meeting]` section: opt-in meeting transcription (Hark Meetings).
//!
//! Off by default in spirit (no meeting starts without a manual action or an
//! auto-detect hit) but additive, so every config file written before this
//! section existed keeps loading unchanged. `enabled` is the master switch;
//! everything else is meaningless while it is `false`.

use serde::{Deserialize, Serialize};

/// Which processes the loopback capture records once a meeting is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SystemSource {
    /// Everything except Hark itself. Always used for a manually started
    /// meeting, regardless of this setting.
    All,
    /// A detected meeting captures only the app's process tree (its root PID
    /// and children); everything else on the machine stays out of the mix.
    #[default]
    App,
}

/// Whether a Deepgram speaker-diarization pass runs after the meeting ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FinalPass {
    /// Re-transcribe with diarization for Speaker 1/2/3 labels ($0.52/hour,
    /// see the Speaker labels settings page). Requires its own Deepgram key.
    #[default]
    Deepgram,
    /// Keep the live Me/Them transcript as final; no second pass, no cost.
    None,
}

/// How eagerly Hark offers to record a detected meeting (§4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AutoDetect {
    /// Never prompt or auto-start; a meeting only begins manually.
    Off,
    /// Non-modal prompt when a meeting app takes the mic. The default: it
    /// offers the feature without ever recording without being asked.
    #[default]
    Ask,
    /// Start recording silently on detection. The tray and live pane
    /// indicator are always visible regardless -- "auto" is never invisible.
    Auto,
}

/// Upper bound on `audio_cap_mb`: 1 TB, expressed in MB. An absurd value here
/// is a typo, not a reason to refuse to start, so [`clamp`] moves it in place
/// instead of [`crate::Settings::validate`] rejecting the whole file.
const MAX_AUDIO_CAP_MB: u32 = 1_048_576;

/// The `[meeting]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Meeting {
    /// Master switch. Everything below is inert while this is `false`.
    pub enabled: bool,
    /// Which microphone to record "Me" from, by cpal device name. `None`
    /// falls back to the Windows communications-default microphone, which is
    /// the device Windows already treats as the one calls/meetings use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mic_device: Option<String>,
    pub system_source: SystemSource,
    /// Show a rolling transcript in the live pane while recording.
    pub live_transcript: bool,
    pub final_pass: FinalPass,
    /// Generate a summary (decisions, action items) after the final pass.
    pub summary: bool,
    /// Custom summary prompt; `None` uses the built-in template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_template: Option<String>,
    /// Show the first-run recording-consent notice before the first meeting.
    pub consent_reminder: bool,
    /// Whether that first-run notice has been seen. Distinct from
    /// `consent_reminder`, which is the ongoing "show it again" toggle: this
    /// flag is what actually gates showing it once.
    pub consent_acknowledged: bool,
    /// Circular storage cap for recording audio, in MB. `0` means don't keep
    /// audio at all -- it is deleted as soon as the final pass and summary
    /// are saved (§4.9 rule 6). Clamped to `0..=MAX_AUDIO_CAP_MB` at load.
    pub audio_cap_mb: u32,
    /// Compress kept recordings to stereo MP3 after processing.
    pub compress_audio: bool,
    pub auto_detect: AutoDetect,
    /// Auto-stop once the detected app has released the mic for this long.
    /// `0` disables auto-stop; a manually started meeting is never
    /// auto-stopped regardless of this value.
    pub auto_stop_after_s: u32,
    /// User-editable match list for auto-detect. `None` uses the built-in
    /// list `hark-meeting` owns, so that list can grow without a config
    /// migration here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detect_apps: Option<Vec<String>>,
}

impl Default for Meeting {
    fn default() -> Self {
        Meeting {
            enabled: true,
            mic_device: None,
            system_source: SystemSource::App,
            live_transcript: true,
            final_pass: FinalPass::Deepgram,
            summary: true,
            summary_template: None,
            consent_reminder: true,
            consent_acknowledged: false,
            audio_cap_mb: 5_120,
            compress_audio: true,
            auto_detect: AutoDetect::Ask,
            auto_stop_after_s: 60,
            detect_apps: None,
        }
    }
}

impl Meeting {
    /// The storage cap in bytes, for comparison against `metadata().len()`
    /// sums (§4.9 rule 1 makes the filesystem the source of truth for size).
    pub fn audio_cap_bytes(&self) -> u64 {
        u64::from(self.audio_cap_mb) * 1024 * 1024
    }

    /// The auto-stop delay in milliseconds, for comparison against a
    /// mic-release timestamp.
    pub fn auto_stop_after_ms(&self) -> u64 {
        u64::from(self.auto_stop_after_s) * 1_000
    }
}

/// Clamp `audio_cap_mb` to a sane range, warning once if it had to move.
/// Called from [`crate::Settings::from_toml`] before validation: an absurd
/// value is a typo, not a reason to refuse to start, so it is a mutation
/// here rather than a [`crate::ConfigError::Invalid`].
pub(crate) fn clamp(meeting: &mut Meeting) {
    if meeting.audio_cap_mb > MAX_AUDIO_CAP_MB {
        log::warn!(
            "meeting.audio_cap_mb {} exceeds the 1 TB ceiling; clamping to {MAX_AUDIO_CAP_MB}",
            meeting.audio_cap_mb
        );
        meeting.audio_cap_mb = MAX_AUDIO_CAP_MB;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConfigError, Settings};

    #[test]
    fn defaults_match_the_documented_shape() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.meeting.enabled);
        assert_eq!(s.meeting.mic_device, None);
        assert_eq!(s.meeting.system_source, SystemSource::App);
        assert!(s.meeting.live_transcript);
        assert_eq!(s.meeting.final_pass, FinalPass::Deepgram);
        assert!(s.meeting.summary);
        assert_eq!(s.meeting.summary_template, None);
        assert!(s.meeting.consent_reminder);
        assert!(!s.meeting.consent_acknowledged);
        assert_eq!(s.meeting.audio_cap_mb, 5_120);
        assert!(s.meeting.compress_audio);
        assert_eq!(s.meeting.auto_detect, AutoDetect::Ask);
        assert_eq!(s.meeting.auto_stop_after_s, 60);
        assert_eq!(s.meeting.detect_apps, None);
    }

    #[test]
    fn a_config_without_the_meeting_section_still_loads() {
        // The whole point of staying additive: a file written by a build
        // that predates this section must load with the documented defaults.
        let s = Settings::from_toml(
            "version = 1\n[provider]\nkind = \"groq\"\n[hotkey]\nptt_key = \"LCtrl+LWin\"",
        )
        .expect("a config predating [meeting] must load");
        assert!(s.meeting.enabled);
        assert_eq!(s.meeting.audio_cap_mb, 5_120);
    }

    #[test]
    fn helper_methods_convert_units() {
        let mut m = Meeting::default();
        assert_eq!(m.audio_cap_bytes(), 5_120 * 1024 * 1024);
        assert_eq!(m.auto_stop_after_ms(), 60_000);

        m.audio_cap_mb = 0;
        assert_eq!(m.audio_cap_bytes(), 0);
        m.auto_stop_after_s = 0;
        assert_eq!(m.auto_stop_after_ms(), 0);
    }

    #[test]
    fn full_round_trip_through_toml() {
        let meeting = Meeting {
            enabled: false,
            mic_device: Some("Yeti Stereo Microphone".to_string()),
            system_source: SystemSource::All,
            live_transcript: false,
            final_pass: FinalPass::None,
            summary: false,
            summary_template: Some("Summarize as bullet points.".to_string()),
            consent_reminder: false,
            consent_acknowledged: true,
            audio_cap_mb: 2_048,
            compress_audio: false,
            auto_detect: AutoDetect::Auto,
            auto_stop_after_s: 30,
            detect_apps: Some(vec!["ms-teams.exe".to_string(), "zoom.exe".to_string()]),
        };
        let s = Settings {
            meeting,
            ..Settings::default()
        };

        let text = s.to_toml().expect("serializes");
        let loaded = Settings::from_toml(&text).expect("re-parses");
        assert_eq!(loaded.meeting, s.meeting);
    }

    #[test]
    fn audio_cap_mb_above_one_terabyte_is_clamped_and_logged() {
        let s = Settings::from_toml("[meeting]\naudio_cap_mb = 2000000")
            .expect("an oversized cap is clamped, not rejected");
        assert_eq!(s.meeting.audio_cap_mb, MAX_AUDIO_CAP_MB);
    }

    #[test]
    fn a_zero_audio_cap_is_left_alone() {
        // 0 is the documented "don't keep audio" value, not an out-of-range
        // one; it must survive the clamp untouched.
        let s = Settings::from_toml("[meeting]\naudio_cap_mb = 0").expect("zero parses");
        assert_eq!(s.meeting.audio_cap_mb, 0);
    }

    #[test]
    fn an_in_range_audio_cap_round_trips_unchanged() {
        let s = Settings::from_toml("[meeting]\naudio_cap_mb = 10240").expect("parses");
        assert_eq!(s.meeting.audio_cap_mb, 10_240);
    }

    #[test]
    fn unknown_enum_values_are_load_errors_naming_the_key() {
        for (bad, key) in [
            ("[meeting]\nsystem_source = \"everything\"", "system_source"),
            ("[meeting]\nfinal_pass = \"whisper\"", "final_pass"),
            ("[meeting]\nauto_detect = \"always\"", "auto_detect"),
        ] {
            let err = Settings::from_toml(bad).expect_err("an unknown variant must be rejected");
            assert!(matches!(err, ConfigError::Parse(_)), "{bad}");
            assert!(err.to_string().contains(key), "{bad}: {err}");
        }
    }

    #[test]
    fn serialized_order_puts_meeting_before_invocations() {
        let text = Settings::default().to_toml().expect("serializes");
        // Defaults omit [[invocations.entries]] entirely (no entries), so
        // pin the ordering against a config that actually has one, which is
        // the shape that would fail to parse if [meeting] ever moved after
        // it in a future edit.
        let mut s = Settings::default();
        s.invocations.entries.push(crate::Invocation {
            phrase: "access granted".to_string(),
            aliases: Vec::new(),
            expansion: "x".to_string(),
            scope: crate::Scope::Utterance,
        });
        let text_with_entries = s.to_toml().expect("serializes");
        let meeting_pos = text_with_entries
            .find("[meeting]")
            .expect("meeting section present");
        let invocations_pos = text_with_entries
            .find("[[invocations.entries]]")
            .expect("invocations array present");
        assert!(
            meeting_pos < invocations_pos,
            "[meeting] must serialize before [[invocations.entries]]: {text_with_entries}"
        );
        // The plain (no-invocations) default output must still parse.
        Settings::from_toml(&text).expect("serialized defaults re-parse");
    }
}
