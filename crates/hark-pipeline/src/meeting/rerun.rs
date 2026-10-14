//! The Deepgram pass every final pass shares (the finisher's first one, the
//! scheduled retries, and the user's explicit re-run), plus that re-run's
//! worker. Never changes audio, and never changes notes a meeting has: the
//! one exception is making them for a meeting still waiting on a retry
//! whose finisher had no transcript to make them from.
use super::finish::{DEEPGRAM_ACCOUNT, DEEPGRAM_ENV_OVERRIDE};
use super::status::{Failure, TranscriptStatus};
use super::{pending, silence, LiveSegment, MeetingEvent};
use hark_config::Settings;
use hark_stt::meeting::{deepgram_final_pass_encoded, FailureKind, MeetingEncoding};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{mpsc::Sender, Arc, Mutex};

pub(super) struct Job {
    pub id: String,
    pub root: PathBuf,
    pub audio_ms: u64,
    pub settings: Settings,
    pub events: Sender<MeetingEvent>,
    pub protected: Arc<Mutex<Vec<String>>>,
}

pub(super) fn spawn(job: Job) {
    let events = job.events.clone();
    let id = job.id.clone();
    let protected = job.protected.clone();
    if std::thread::Builder::new()
        .name("hark-meeting-rerun".into())
        .spawn(move || {
            let result = deepgram_pass(&job.root, &job.id, job.audio_ms, &job.settings);
            let needs_notes = valid_id(&job.id)
                && pending::read(&job.root.join(&job.id)).is_some_and(|p| p.needs_notes);
            let status = settle(&job.root, &job.id, result.as_ref().err().copied());
            let error = match result {
                Ok(segments) => {
                    let lines: Vec<String> = segments
                        .iter()
                        .map(|s| super::finish::line(s.start_ms, s.channel, s.speaker, &s.text))
                        .collect();
                    // Before the status: the pump commits the replacement
                    // first, so a failed save is still reported below.
                    let _ = job.events.send(MeetingEvent::Reprocessed {
                        id: job.id.clone(),
                        segments,
                    });
                    if needs_notes && job.settings.meeting.summary {
                        super::finish::send_notes(&job.settings, &job.id, &lines, &job.events);
                    }
                    None
                }
                Err(failure) => Some(format!(
                    "{} Your previous transcript is unchanged.",
                    failure.message()
                )),
            };
            if let Some(status) = status {
                let _ = job.events.send(MeetingEvent::TranscriptStatus {
                    id: job.id.clone(),
                    status,
                });
            }
            complete(&job.events, &job.protected, &job.id, error);
        })
        .is_err()
    {
        complete(
            &events,
            &protected,
            &id,
            Some("Cannot start the final-pass worker.".into()),
        );
    }
}

/// The status an explicit re-run leaves behind (`None`: leave it alone). A
/// success or a failure no retry can fix ends any schedule; a network
/// failure while retries are scheduled keeps them going.
fn settle(root: &Path, id: &str, failure: Option<Failure>) -> Option<Option<TranscriptStatus>> {
    if !valid_id(id) {
        return None;
    }
    let dir = root.join(id);
    match failure {
        None => {
            pending::remove(&dir);
            Some(None)
        }
        Some(failure) => match pending::read(&dir) {
            Some(scheduled) if failure.retryable() => Some(Some(scheduled.status())),
            _ => {
                pending::remove(&dir);
                Some(Some(TranscriptStatus::Failed(failure)))
            }
        },
    }
}

fn complete(
    events: &Sender<MeetingEvent>,
    protected: &Mutex<Vec<String>>,
    id: &str,
    error: Option<String>,
) {
    let _ = events.send(MeetingEvent::ReprocessFinished {
        id: id.into(),
        error,
    });
    if let Ok(mut ids) = protected.lock() {
        ids.retain(|x| x != id);
        let _ = events.send(MeetingEvent::EnforceAudioCap {
            protected: ids.clone(),
        });
    }
}

/// One Deepgram final pass over a retained recording: the archive when there
/// is one (8x smaller than the WAVs on a slow uplink), else the WAVs. Checks
/// for silence before anything is uploaded. Blocking; worker threads only.
pub(super) fn deepgram_pass(
    root: &Path,
    id: &str,
    audio_ms: u64,
    settings: &Settings,
) -> Result<Vec<LiveSegment>, Failure> {
    let audio = open_audio(root, id).map_err(|_| Failure::NoAudio)?;
    if audio_ms == 0 {
        return Err(Failure::NoAudio);
    }
    if silence::recording_is_silent(&root.join(id)) {
        log::info!("meeting {id}: recording is silent; nothing uploaded");
        return Err(Failure::Silent);
    }
    let key = hark_keychain::resolve_key_for(DEEPGRAM_ENV_OVERRIDE, DEEPGRAM_ACCOUNT)
        .map_err(|_| Failure::NoKey)?;
    let client = hark_stt::meeting::final_pass_client().map_err(|_| Failure::Setup)?;
    log::info!(
        "meeting {id}: final pass uploading {} bytes ({:?})",
        audio.len,
        audio.encoding
    );
    let segments = deepgram_final_pass_encoded(
        &client,
        "https://api.deepgram.com",
        &key,
        audio.reader,
        audio.len,
        &settings.spellbook.terms(),
        audio_ms,
        audio.encoding,
    )
    .map_err(|e| {
        // Safe to log: no SttError variant carries a key, a body or audio.
        log::warn!("meeting {id}: final pass failed ({e})");
        Failure::Stt(FailureKind::of(&e))
    })?;
    if segments.is_empty() {
        return Err(Failure::Empty);
    }
    let corrector = hark_spellbook::Corrector::new(&settings.spellbook.corrector_entries());
    Ok(segments
        .into_iter()
        .map(|s| LiveSegment {
            channel: s.channel.min(1),
            speaker: s.speaker,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: corrector.correct(&s.text).0,
        })
        .collect())
}

/// A bare folder name: no separators, drive prefixes or dot entries.
fn valid_id(id: &str) -> bool {
    !(id.is_empty() || id == "." || id == ".." || id.contains(['/', '\\', ':', '\0']))
}

struct AudioBody {
    reader: Box<dyn Read + Send>,
    len: u64,
    encoding: MeetingEncoding,
}

/// Restrict uploads to real files in the selected recording's real directory.
fn open_audio(root: &Path, id: &str) -> std::io::Result<AudioBody> {
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "invalid recording path",
        )
    };
    if !valid_id(id) {
        return Err(invalid());
    }
    let root = root.canonicalize()?;
    let dir = root.join(id);
    if !std::fs::symlink_metadata(&dir)?.file_type().is_dir()
        || dir.canonicalize()?.parent() != Some(root.as_path())
    {
        return Err(invalid());
    }
    // Prefer the completed archive. No decoding or re-encoding: channel separation stays intact.
    let archive = dir.join(hark_audio::ARCHIVE_FILE);
    if archive.try_exists()? {
        if !std::fs::symlink_metadata(&archive)?.file_type().is_file() {
            return Err(invalid());
        }
        let file = std::fs::File::open(archive)?;
        let len = file.metadata()?.len();
        if len == 0 {
            return Err(invalid());
        }
        return Ok(AudioBody {
            reader: Box::new(file),
            len,
            encoding: MeetingEncoding::Mp3,
        });
    }
    let me = dir.join(hark_audio::spool::ME_FILE);
    let them = dir.join(hark_audio::spool::THEM_FILE);
    for file in [&me, &them] {
        if file.try_exists()? && !std::fs::symlink_metadata(file)?.file_type().is_file() {
            return Err(invalid());
        }
    }
    if hark_audio::spool_pair_frames(&me, &them)? == 0 {
        return Err(invalid());
    }
    let (reader, len) = hark_audio::stereo_wav_reader(&me, &them)?;
    Ok(AudioBody {
        reader,
        len,
        encoding: MeetingEncoding::Wav,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_is_uploaded_byte_for_byte_and_traversal_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("meeting");
        std::fs::create_dir(&dir).unwrap();
        let bytes = b"test stereo archive bytes";
        std::fs::write(dir.join(hark_audio::ARCHIVE_FILE), bytes).unwrap();
        let mut audio = open_audio(root.path(), "meeting").unwrap();
        assert_eq!(audio.encoding, MeetingEncoding::Mp3);
        assert_eq!(audio.len, bytes.len() as u64);
        let mut actual = Vec::new();
        audio.reader.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, bytes);
        for id in ["../meeting", "", ".", "..", "C:meeting", "a\\b"] {
            assert!(open_audio(root.path(), id).is_err());
        }
    }
    #[test]
    fn completion_releases_only_its_own_protection_and_reports_failure() {
        let (tx, rx) = std::sync::mpsc::channel();
        let protected = Mutex::new(vec!["old".into(), "recording".into()]);
        complete(&tx, &protected, "old", Some("failed".into()));
        assert!(matches!(
            rx.recv().unwrap(),
            MeetingEvent::ReprocessFinished { error: Some(_), .. }
        ));
        assert!(
            matches!(rx.recv().unwrap(), MeetingEvent::EnforceAudioCap { protected } if protected == ["recording"])
        );
    }
    #[test]
    fn a_silent_recording_is_never_uploaded() {
        // Silence is decided before the key or the network are touched, so
        // this needs neither.
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("meeting");
        std::fs::create_dir(&dir).unwrap();
        for file in [hark_audio::spool::ME_FILE, hark_audio::spool::THEM_FILE] {
            let mut spool = hark_audio::spool::SpoolWriter::create(&dir.join(file)).unwrap();
            spool.append(&vec![0i16; 32_000]).unwrap();
            spool.close().unwrap();
        }
        let result = deepgram_pass(root.path(), "meeting", 2_000, &Settings::default());
        assert!(matches!(result, Err(Failure::Silent)));
        assert!(matches!(
            deepgram_pass(root.path(), "../meeting", 2_000, &Settings::default()),
            Err(Failure::NoAudio)
        ));
    }

    #[test]
    fn a_rerun_ends_the_schedule_unless_the_network_is_still_down() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("m");
        std::fs::create_dir(&dir).unwrap();
        let scheduled = pending::Pending::first(0, 1_000, false);
        let network = Failure::Stt(FailureKind::Network);

        pending::write(&dir, scheduled).unwrap();
        assert_eq!(
            settle(root.path(), "m", Some(network)),
            Some(Some(scheduled.status())),
            "still offline: the schedule carries on"
        );
        assert!(pending::read(&dir).is_some());

        let auth = Failure::Stt(FailureKind::Auth);
        assert_eq!(
            settle(root.path(), "m", Some(auth)),
            Some(Some(TranscriptStatus::Failed(auth)))
        );
        assert!(pending::read(&dir).is_none(), "a 4xx ends the schedule");

        pending::write(&dir, scheduled).unwrap();
        assert_eq!(settle(root.path(), "m", None), Some(None));
        assert!(pending::read(&dir).is_none(), "success ends the schedule");

        assert_eq!(
            settle(root.path(), "m", Some(network)),
            Some(Some(TranscriptStatus::Failed(network))),
            "no schedule: an explicit re-run reports, it does not start one"
        );
        assert_eq!(settle(root.path(), "../m", None), None);
    }

    #[test]
    fn missing_and_empty_recordings_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        assert!(open_audio(root.path(), "missing").is_err());
        std::fs::create_dir(root.path().join("empty")).unwrap();
        assert!(open_audio(root.path(), "empty").is_err());
        std::fs::write(root.path().join("empty/audio.mp3"), []).unwrap();
        assert!(open_audio(root.path(), "empty").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_archive_cannot_be_uploaded() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("meeting")).unwrap();
        std::fs::write(root.path().join("private"), b"secret").unwrap();
        std::os::unix::fs::symlink(
            root.path().join("private"),
            root.path().join("meeting/audio.mp3"),
        )
        .unwrap();
        assert!(open_audio(root.path(), "meeting").is_err());
    }
}
