//! Explicit refinement of a retained recording. Never changes notes or audio.
use super::finish::{DEEPGRAM_ACCOUNT, DEEPGRAM_ENV_OVERRIDE};
use super::{LiveSegment, MeetingEvent};
use hark_config::Settings;
use hark_stt::meeting::{deepgram_final_pass_encoded, MeetingEncoding};
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
            let result = run(&job);
            let error = match result {
                Ok(segments) => {
                    let _ = job.events.send(MeetingEvent::Reprocessed {
                        id: job.id.clone(),
                        segments,
                    });
                    None
                }
                Err(error) => Some(error),
            };
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

fn run(job: &Job) -> Result<Vec<LiveSegment>, String> {
    let key =
        hark_keychain::resolve_key_for(DEEPGRAM_ENV_OVERRIDE, DEEPGRAM_ACCOUNT).map_err(|_| {
            "Add a Deepgram key in Settings > Meetings before re-running the final pass."
                .to_string()
        })?;
    if job.audio_ms == 0 {
        return Err("This recording has no duration.".into());
    }
    let client =
        hark_stt::shared_client().map_err(|_| "Cannot initialize transcription.".to_string())?;
    let audio = open_audio(&job.root, &job.id)
        .map_err(|_| "The retained recording is missing, unsafe, or unreadable.".to_string())?;
    let segments = deepgram_final_pass_encoded(
        &client,
        "https://api.deepgram.com",
        &key,
        audio.reader,
        audio.len,
        &job.settings.spellbook.terms(),
        job.audio_ms,
        audio.encoding,
    )
    .map_err(|_| {
        "Deepgram could not complete the final pass. Your previous transcript is unchanged."
            .to_string()
    })?;
    if segments.is_empty() {
        return Err(
            "Deepgram returned no transcript. Your previous transcript is unchanged.".into(),
        );
    }
    let corrector = hark_spellbook::Corrector::new(&job.settings.spellbook.corrector_entries());
    Ok(segments
        .into_iter()
        .map(|s| LiveSegment {
            channel: s.channel,
            speaker: s.speaker,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: corrector.correct(&s.text).0,
        })
        .collect())
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
    if id.is_empty() || id == "." || id == ".." || id.contains(['/', '\\', ':', '\0']) {
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
