//! After the call: let the live transcriber finish, archive the recording,
//! then the Deepgram final pass (speaker labels) and the notes. One thread per
//! meeting, so a new meeting can record while an older one finishes.
//!
//! The archive comes first because the final pass uploads it: a 40-minute
//! call is 19 MB of MP3 against 155 MB of WAV, which matters on a cellular
//! uplink. With compression off, the pass uploads the WAVs as before.
//!
//! Every step degrades instead of failing the meeting (plan D2, §4.2): no
//! Deepgram key or a failed pass keeps the live Me/Them transcript; no text
//! provider or a failed summary keeps the transcript without notes; a failed
//! archive keeps the WAVs. Each of those says why in a notice, and the final
//! pass's outcome is also kept as the meeting's transcript status. A pass
//! that could not reach Deepgram is handed to the retry scheduler
//! (`pending.rs`) rather than given up.

use super::pending::{self, Pending};
use super::recorder::Recorded;
use super::status::{Failure, TranscriptStatus};
use super::{LiveSegment, MeetingEvent};
use hark_config::{CleanupKeySource, CleanupResolution, FinalPass, Settings, VoiceName};
use hark_meeting::{advance, export, Channel, Event, SessionState, SpeakerKey, Transcript};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Environment override for the meetings Deepgram key, mirroring the
/// dictation key's `HARK_STT_KEY`.
pub(super) const DEEPGRAM_ENV_OVERRIDE: &str = "HARK_DEEPGRAM_KEY";
/// The keychain account the Deepgram key lives under (plan D2), shared with a
/// Deepgram dictation setup on purpose: one key, one place.
pub const DEEPGRAM_ACCOUNT: &str = "deepgram";
/// Written into a meeting's folder when it starts, removed only once its
/// after-call work is done. A folder that still has it at startup belongs to
/// a meeting whose finishing was cut short (Hark quit or crashed mid-call or
/// mid-finish), and is finished again then: nothing about a meeting may
/// depend on the app staying open for the 15 minutes a final pass can take.
///
/// The finisher writes the recording's length into it (`audio_ms=<n>`):
/// once the archive has replaced the WAVs, the spools can no longer say how
/// long the meeting was.
pub(super) const FINISHING_MARKER: &str = ".finishing";

pub(super) struct FinishJob {
    pub recorded: Recorded,
    pub live: Option<JoinHandle<()>>,
    pub transcript: Arc<Mutex<Transcript>>,
    pub settings: Box<Settings>,
    pub events: Sender<MeetingEvent>,
    /// Ids the storage cap must not touch; this job removes its own when done.
    pub protected: Arc<Mutex<Vec<String>>>,
    /// Where the meeting is in its lifecycle (Finalizing on arrival).
    pub session: SessionState,
    /// Where a pass that could not reach Deepgram goes to be retried.
    pub retries: pending::Queue,
}

pub(super) fn spawn(job: FinishJob) -> Option<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("hark-meeting-finish".to_string())
        .spawn(move || finish(job))
        .map_err(|e| log::error!("cannot start the meeting finisher: {e}"))
        .ok()
}

/// What the final pass left the meeting with.
enum Pass {
    Refined(Vec<LiveSegment>),
    /// No refined transcript (none configured, or it failed for good): the
    /// live one stays.
    Kept,
    /// Deepgram could not be reached: the live transcript stays for now.
    Retry,
}

fn finish(job: FinishJob) {
    let FinishJob {
        recorded,
        live,
        transcript,
        settings,
        events,
        protected,
        mut session,
        retries,
    } = job;
    let id = recorded.id.clone();
    let mut step = |event: Event| {
        let (next, _) = advance(session, event);
        log::info!("meeting {id}: {session:?} -> {next:?}");
        session = next;
    };
    let notice = |text: String| {
        let _ = events.send(MeetingEvent::Notice {
            id: Some(id.clone()),
            text,
        });
    };
    let status = |status: Option<TranscriptStatus>| {
        let _ = events.send(MeetingEvent::TranscriptStatus {
            id: id.clone(),
            status,
        });
    };

    // The last chunks were queued at stop; their lines belong in the notes.
    if let Some(live) = live {
        let _ = live.join();
    }
    let audio_ms =
        recorded.me_samples.max(recorded.them_samples) * 1000 / hark_meeting::SAMPLE_RATE as u64;
    mark_audio_ms(&recorded.dir, audio_ms);

    if settings.meeting.compress_audio && settings.meeting.audio_cap_mb > 0 {
        match hark_audio::compress_meeting_dir(&recorded.dir) {
            Ok(outcome) => log::info!("meeting {id}: archive {}", compress_label(&outcome)),
            // The WAVs are untouched on failure; they are still the audio,
            // and the final pass uploads them instead.
            Err(e) => log::warn!("meeting {id}: compression failed ({e}); keeping the WAV files"),
        }
    }

    let mut lines = live_lines(&transcript);
    let mut retry = false;
    if audio_ms > 0 {
        match final_pass(&recorded.dir, &id, audio_ms, &settings, &notice, &status) {
            Pass::Refined(refined) => {
                lines = NotesInput::of(&refined);
                let _ = events.send(MeetingEvent::Refined {
                    id: id.clone(),
                    segments: refined,
                });
            }
            Pass::Kept => {}
            Pass::Retry => retry = true,
        }
    }
    // Refined or not, the transcript is now settled (plan D2).
    step(Event::Finalized);

    let notes = settings.meeting.summary
        && !lines.lines.is_empty()
        && send_notes(&settings, &id, &lines, &events);
    // Notes saved, or none to make: either way the meeting is complete.
    step(Event::Summarized);

    if retry {
        let schedule = Pending::first(
            pending::unix_now_ms(),
            audio_ms,
            settings.meeting.summary && !notes,
        );
        match pending::write(&recorded.dir, schedule) {
            Ok(()) => {
                status(Some(schedule.status()));
                retries.schedule(&id);
                notice(
                    "Deepgram could not be reached. Hark kept the recording and will try \
                     again in a minute; until then this meeting keeps its live transcript."
                        .to_string(),
                );
            }
            Err(e) => {
                log::warn!("meeting {id}: retry not scheduled ({e})");
                let failure = Failure::Stt(hark_stt::meeting::FailureKind::Network);
                status(Some(TranscriptStatus::Failed(failure)));
                notice(format!(
                    "{} Open the meeting and choose Transcribe recording to try again.",
                    failure.message()
                ));
            }
        }
    }

    let audio_bytes = dir_bytes(&recorded.dir);
    let _ = events.send(MeetingEvent::Finished {
        id: id.clone(),
        audio_bytes,
    });
    // Only after the results are on their way: a quit before this point
    // leaves the marker, and the next start finishes the meeting again.
    match std::fs::remove_file(recorded.dir.join(FINISHING_MARKER)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("meeting {id}: finishing marker not removed ({e})"),
    }
    let remaining = match protected.lock() {
        Ok(mut p) => {
            p.retain(|x| x != &id);
            p.clone()
        }
        Err(_) => Vec::new(),
    };
    let _ = events.send(MeetingEvent::EnforceAudioCap {
        protected: remaining,
    });
}

/// Record the length in the finishing marker, only while there is one.
fn mark_audio_ms(dir: &Path, audio_ms: u64) {
    let marker = dir.join(FINISHING_MARKER);
    if marker.exists() {
        if let Err(e) = std::fs::write(&marker, format!("audio_ms={audio_ms}\n")) {
            log::warn!("meeting length not recorded in {} ({e})", marker.display());
        }
    }
}

/// The length a finisher recorded in the finishing marker, as samples (0
/// when it recorded none).
pub(super) fn marked_samples(dir: &Path) -> u64 {
    std::fs::read_to_string(dir.join(FINISHING_MARKER))
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|l| l.strip_prefix("audio_ms="))
                .and_then(|ms| ms.trim().parse::<u64>().ok())
        })
        .map_or(0, |ms| ms * hark_meeting::SAMPLE_RATE as u64 / 1000)
}

fn compress_label(outcome: &hark_audio::CompressOutcome) -> String {
    match outcome {
        hark_audio::CompressOutcome::Compressed {
            archive_bytes,
            wav_bytes_freed,
        } => format!("{archive_bytes} bytes, {wav_bytes_freed} WAV bytes freed"),
        hark_audio::CompressOutcome::NothingToDo => "not needed".to_string(),
    }
}

/// The live transcript as prompt lines.
fn live_lines(transcript: &Mutex<Transcript>) -> NotesInput {
    let mut input = NotesInput::default();
    let Ok(t) = transcript.lock() else {
        return input;
    };
    for s in t.segments() {
        let to_ms = |x: u64| x * 1000 / hark_meeting::SAMPLE_RATE as u64;
        let channel = u8::from(s.channel == Channel::Them);
        input.push(
            to_ms(s.start),
            SpeakerKey::transcribed(channel, None),
            &s.text,
        );
    }
    input
}

/// The transcript as the summary prompt sees it: `[mm:ss] {Label}: text`,
/// each speaker by their default label in braces, never a name the user
/// set; plus the `(label, key)` table the notes' braced references parse
/// back through, so the stored notes name keys and follow later renames.
#[derive(Default)]
pub(super) struct NotesInput {
    pub lines: Vec<String>,
    pub labels: Vec<(String, String)>,
}

impl NotesInput {
    /// Transcribed speakers, for a final pass's fresh segments.
    pub fn of(segments: &[LiveSegment]) -> Self {
        let mut input = NotesInput::default();
        for s in segments {
            let key = SpeakerKey::transcribed(s.channel, s.speaker);
            input.push(s.start_ms, key, &s.text);
        }
        input
    }

    pub fn push(&mut self, start_ms: u64, key: SpeakerKey, text: &str) {
        let label = hark_meeting::default_label(key);
        self.lines.push(format!(
            "[{}] {{{label}}}: {}",
            export::format_timestamp(start_ms),
            text.trim()
        ));
        if !self.labels.iter().any(|(l, _)| *l == label) {
            self.labels.push((label, key.to_string()));
        }
    }
}

/// The final pass, with its outcome as a notice and a transcript status.
fn final_pass(
    dir: &Path,
    id: &str,
    audio_ms: u64,
    settings: &Settings,
    notice: &dyn Fn(String),
    status: &dyn Fn(Option<TranscriptStatus>),
) -> Pass {
    let silent = || {
        notice(Failure::Silent.message().to_string());
        status(Some(TranscriptStatus::Failed(Failure::Silent)));
        Pass::Kept
    };
    match settings.meeting.final_pass {
        FinalPass::None => return Pass::Kept,
        FinalPass::Gemini => {
            if super::silence::recording_is_silent(dir) {
                return silent();
            }
            return match super::gemini_final::run(dir, settings) {
                Ok(segments) => Pass::Refined(segments),
                Err(error) => {
                    notice(format!("Gemini final pass unavailable: {error}"));
                    Pass::Kept
                }
            };
        }
        FinalPass::Deepgram => {}
    }
    let Some(root) = dir.parent() else {
        return Pass::Kept;
    };
    match super::rerun::deepgram_pass(root, id, audio_ms, settings) {
        Ok(segments) => {
            // A refinish can succeed where an earlier pass did not.
            pending::remove(dir);
            status(None);
            Pass::Refined(segments)
        }
        Err(failure) if failure.retryable() => Pass::Retry,
        Err(Failure::Silent) => silent(),
        Err(Failure::NoKey) => {
            notice(
                "Add a Deepgram key in Settings > Meetings to label each speaker. \
                 This meeting keeps its Me/Them transcript."
                    .to_string(),
            );
            status(Some(TranscriptStatus::Failed(Failure::NoKey)));
            Pass::Kept
        }
        // Nothing heard: keep whatever the live pass found.
        Err(Failure::Empty) => {
            status(Some(TranscriptStatus::Failed(Failure::Empty)));
            Pass::Kept
        }
        Err(failure) => {
            notice(format!(
                "Speaker labels are unavailable for this meeting: {} \
                 It keeps its Me/Them transcript.",
                failure.message()
            ));
            status(Some(TranscriptStatus::Failed(failure)));
            Pass::Kept
        }
    }
}

/// Make notes from `lines` and send them; a failure becomes a notice. True
/// when notes were sent.
pub(super) fn send_notes(
    settings: &Settings,
    id: &str,
    input: &NotesInput,
    events: &Sender<MeetingEvent>,
) -> bool {
    match summarize(settings, input) {
        Ok(notes) => {
            let _ = events.send(MeetingEvent::Notes {
                id: id.to_string(),
                title: notes.title.clone(),
                notes_json: notes.to_json(),
            });
            true
        }
        Err(detail) => {
            let _ = events.send(MeetingEvent::Notice {
                id: Some(id.to_string()),
                text: format!("No notes for this meeting: {detail}"),
            });
            false
        }
    }
}

/// Notes from the user's text provider: the same one voice cleanup resolves
/// to, asked for as if a non-Verbatim voice were selected (a Verbatim
/// dictation setup still has a provider that can write notes).
fn summarize(settings: &Settings, input: &NotesInput) -> Result<hark_voice::MeetingNotes, String> {
    let resolved = match hark_config::resolve_cleanup_provider(
        &settings.provider,
        &settings.voice,
        VoiceName::Notes,
    ) {
        CleanupResolution::Resolved(r) => r,
        CleanupResolution::VerbatimWithWarning { reason } => return Err(reason),
        CleanupResolution::Verbatim => return Err("no text provider is configured".to_string()),
    };
    let api_key = match &resolved.key_source {
        CleanupKeySource::ReuseSttKey => hark_keychain::resolve_key(settings.provider.kind.label()),
        CleanupKeySource::Account(account) => {
            hark_keychain::resolve_key_for(hark_keychain::CLEANUP_ENV_OVERRIDE, account)
        }
    }
    .map_err(|e| e.to_string())?;
    let config = hark_voice::SummaryConfig {
        label: resolved.kind.label().to_string(),
        base_url: resolved.base_url.clone(),
        model: resolved.model.clone(),
        api_key,
        temperature: resolved.temperature,
        reasoning_effort: resolved.reasoning_effort.clone(),
    };
    let client = hark_stt::shared_client().map_err(|e| e.to_string())?;
    hark_voice::summarize(
        &client,
        &config,
        &input.lines.join("\n"),
        settings.meeting.summary_template.as_deref(),
        &input.labels,
    )
    .map_err(|e| e.to_string())
}

/// Bytes of the files in a meeting's directory (the audio it keeps on disk).
fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(channel: u8, speaker: Option<u32>, start_ms: u64, text: &str) -> LiveSegment {
        LiveSegment {
            channel,
            speaker,
            start_ms,
            end_ms: start_ms + 500,
            text: text.to_string(),
        }
    }

    #[test]
    fn prompt_lines_brace_default_labels_and_keep_the_time_shape() {
        let window = 0x8000_0000 | (1 << 10) | 2;
        let input = NotesInput::of(&[
            seg(0, Some(4), 0, " hi "),
            seg(1, Some(1), 65_000, "hello"),
            seg(1, Some(window), 3_725_000, "later"),
            seg(1, Some(1), 3_726_000, "again"),
        ]);
        assert_eq!(
            input.lines,
            [
                "[00:00] {Me}: hi",
                "[01:05] {Speaker 2}: hello",
                "[1:02:05] {Window 2 · Speaker 3}: later",
                "[1:02:06] {Speaker 2}: again",
            ]
        );
        let labels: Vec<(&str, &str)> = input
            .labels
            .iter()
            .map(|(l, k)| (l.as_str(), k.as_str()))
            .collect();
        assert_eq!(
            labels,
            [
                ("Me", "me"),
                ("Speaker 2", "them:1"),
                ("Window 2 · Speaker 3", "them:2147484674"),
            ]
        );
    }

    #[test]
    fn the_finishing_marker_remembers_the_length_once_the_wavs_are_gone() {
        let dir = tempfile::tempdir().unwrap();
        // No marker: nothing recorded, and none is created.
        mark_audio_ms(dir.path(), 5_000);
        assert!(!dir.path().join(FINISHING_MARKER).exists());
        assert_eq!(marked_samples(dir.path()), 0);
        // The coordinator's empty marker reads as nothing recorded too.
        std::fs::write(dir.path().join(FINISHING_MARKER), b"").unwrap();
        assert_eq!(marked_samples(dir.path()), 0);
        mark_audio_ms(dir.path(), 2_434_000);
        assert_eq!(marked_samples(dir.path()), 2_434_000 * 16);
    }
}
