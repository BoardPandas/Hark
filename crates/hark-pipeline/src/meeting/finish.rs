//! After the call: let the live transcriber finish, then the Deepgram final
//! pass (speaker labels), the notes, and the MP3 archive. One thread per
//! meeting, so a new meeting can record while an older one finishes.
//!
//! Every step degrades instead of failing the meeting (plan D2, §4.2): no
//! Deepgram key or a failed pass keeps the live Me/Them transcript; no text
//! provider or a failed summary keeps the transcript without notes; a failed
//! archive keeps the WAVs. Each of those says why in a notice.

use super::recorder::Recorded;
use super::{LiveSegment, MeetingEvent};
use hark_config::{CleanupKeySource, CleanupResolution, FinalPass, Settings, VoiceName};
use hark_meeting::{advance, export, Channel, Event, SessionState, Transcript};
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
const DEEPGRAM_BASE_URL: &str = "https://api.deepgram.com";
/// Written into a meeting's folder when it starts, removed only once its
/// after-call work is done. A folder that still has it at startup belongs to
/// a meeting whose finishing was cut short (Hark quit or crashed mid-call or
/// mid-finish), and is finished again then: nothing about a meeting may
/// depend on the app staying open for the 15 minutes a final pass can take.
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
}

pub(super) fn spawn(job: FinishJob) -> Option<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("hark-meeting-finish".to_string())
        .spawn(move || finish(job))
        .map_err(|e| log::error!("cannot start the meeting finisher: {e}"))
        .ok()
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

    // The last chunks were queued at stop; their lines belong in the notes.
    if let Some(live) = live {
        let _ = live.join();
    }
    let audio_ms =
        recorded.me_samples.max(recorded.them_samples) * 1000 / hark_meeting::SAMPLE_RATE as u64;

    let mut lines = live_lines(&transcript);
    if audio_ms > 0 {
        if let Some(refined) = final_pass(&recorded.dir, audio_ms, &settings, &notice) {
            lines = refined
                .iter()
                .map(|s| line(s.start_ms, s.channel, s.speaker, &s.text))
                .collect();
            let _ = events.send(MeetingEvent::Refined {
                id: id.clone(),
                segments: refined,
            });
        }
    }
    // Refined or not, the transcript is now settled (plan D2).
    step(Event::Finalized);

    if settings.meeting.summary && !lines.is_empty() {
        match summarize(&settings, &lines.join("\n")) {
            Ok(notes) => {
                let _ = events.send(MeetingEvent::Notes {
                    id: id.clone(),
                    title: notes.title.clone(),
                    notes_json: notes.to_json(),
                });
            }
            Err(detail) => notice(format!("No notes for this meeting: {detail}")),
        }
    }
    // Notes saved, or none to make: either way the meeting is complete.
    step(Event::Summarized);

    if settings.meeting.compress_audio && settings.meeting.audio_cap_mb > 0 {
        match hark_audio::compress_meeting_dir(&recorded.dir) {
            Ok(outcome) => log::info!("meeting {id}: archive {}", compress_label(&outcome)),
            // The WAVs are untouched on failure; they are still the audio.
            Err(e) => log::warn!("meeting {id}: compression failed ({e}); keeping the WAV files"),
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
fn live_lines(transcript: &Mutex<Transcript>) -> Vec<String> {
    let Ok(t) = transcript.lock() else {
        return Vec::new();
    };
    t.segments()
        .iter()
        .map(|s| {
            let to_ms = |x: u64| x * 1000 / hark_meeting::SAMPLE_RATE as u64;
            let channel = match s.channel {
                Channel::Me => 0,
                Channel::Them => 1,
            };
            line(to_ms(s.start), channel, None, &s.text)
        })
        .collect()
}

/// `[mm:ss] Label: text`, the transcript shape the summary prompt expects.
fn line(start_ms: u64, channel: u8, speaker: Option<u32>, text: &str) -> String {
    let channel = if channel == 0 {
        Channel::Me
    } else {
        Channel::Them
    };
    format!(
        "[{}] {}: {}",
        export::format_timestamp(start_ms),
        export::speaker_label(channel, speaker, &[]),
        text.trim()
    )
}

/// The Deepgram pass, or `None` (with a notice) when it cannot or did not run.
fn final_pass(
    dir: &Path,
    audio_ms: u64,
    settings: &Settings,
    notice: &dyn Fn(String),
) -> Option<Vec<LiveSegment>> {
    if settings.meeting.final_pass == FinalPass::Gemini {
        return match super::gemini_final::run(dir, settings) {
            Ok(segments) => Some(segments),
            Err(error) => {
                notice(format!("Gemini final pass unavailable: {error}"));
                None
            }
        };
    }
    if settings.meeting.final_pass != FinalPass::Deepgram {
        return None;
    }
    let key = match hark_keychain::resolve_key_for(DEEPGRAM_ENV_OVERRIDE, DEEPGRAM_ACCOUNT) {
        Ok(key) => key,
        Err(_) => {
            notice(
                "Add a Deepgram key in Settings > Meetings to label each speaker. \
                 This meeting keeps its Me/Them transcript."
                    .to_string(),
            );
            return None;
        }
    };
    let run = || -> Result<Vec<hark_stt::meeting::FinalSegment>, String> {
        let (body, len) = hark_audio::stereo_wav_reader(
            &dir.join(hark_audio::spool::ME_FILE),
            &dir.join(hark_audio::spool::THEM_FILE),
        )
        .map_err(|e| format!("cannot read the recording: {e}"))?;
        let client = hark_stt::shared_client().map_err(|e| e.to_string())?;
        hark_stt::meeting::deepgram_final_pass(
            &client,
            DEEPGRAM_BASE_URL,
            &key,
            body,
            len,
            &settings.spellbook.terms(),
            audio_ms,
        )
        .map_err(|e| e.to_string())
    };
    // Plan §4.2: the spellbook's phonetic fixes apply to meeting lines too,
    // the refined ones as much as the live ones.
    let corrector = hark_spellbook::Corrector::new(&settings.spellbook.corrector_entries());
    match run() {
        Ok(segments) if !segments.is_empty() => Some(
            segments
                .into_iter()
                .map(|s| LiveSegment {
                    channel: s.channel.min(1),
                    speaker: s.speaker,
                    start_ms: s.start_ms,
                    end_ms: s.end_ms,
                    text: corrector.correct(&s.text).0,
                })
                .collect(),
        ),
        // Nothing heard: keep whatever the live pass found.
        Ok(_) => None,
        Err(detail) => {
            notice(format!(
                "Speaker labels are unavailable for this meeting ({detail}). \
                 It keeps its Me/Them transcript."
            ));
            None
        }
    }
}

/// Notes from the user's text provider: the same one voice cleanup resolves
/// to, asked for as if a non-Verbatim voice were selected (a Verbatim
/// dictation setup still has a provider that can write notes).
fn summarize(settings: &Settings, transcript: &str) -> Result<hark_voice::MeetingNotes, String> {
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
        transcript,
        settings.meeting.summary_template.as_deref(),
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
