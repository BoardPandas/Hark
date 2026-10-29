//! Meeting writes on the storage worker: the one writer for both the database
//! rows and the meeting audio folders, so the storage cap sees a consistent
//! view of both (plan §4.9: sizes from the filesystem, ids from the database).
//!
//! Commands come from the meeting event pump (`crate::meeting`) and from the
//! Meetings page. Labels, ids and byte counts only in logs; never text.

use hark_meeting::storage_fs;
use hark_meeting::{plan_eviction, Roster, StoredAudio};
use hark_store::{MeetingDetail, MeetingSegment, NewMeeting, SpeakerName, Store, StoreError};
use std::path::Path;

pub const ASSIGN_FAILED: &str = "This line could not be moved. Try again.";

/// The rewritten notes with the stored notes' ticks carried over. Default
/// labels are ignored when matching items, so "Speaker 2 sends the deck"
/// still matches "{{them:1}} sends the deck".
fn carry_ticks(fresh: &str, stored: &str, roster: &Roster) -> String {
    use hark_voice::MeetingNotes;
    let (Ok(mut fresh_notes), Ok(old)) = (
        MeetingNotes::from_json(fresh),
        MeetingNotes::from_json(stored),
    ) else {
        return fresh.to_string();
    };
    let names = roster.notes_names();
    let strip: Vec<&str> = names
        .iter()
        .flat_map(|(_, default, display)| [default.as_str(), display.as_str()])
        .collect();
    fresh_notes.carry_done_from(&old, &strip);
    fresh_notes.to_json()
}

/// A generated title with its `{{key}}` speaker tokens written out.
fn resolve_title(title: &str, roster: &Roster) -> String {
    hark_voice::MeetingNotes {
        title: title.to_string(),
        speaker_refs: hark_voice::SpeakerRefs::Tokens,
        ..Default::default()
    }
    .render(&roster.notes_names())
    .title
}

/// The one way to read who said what in a stored meeting: every screen and
/// export builds its labels and colors from this.
pub fn roster_of(detail: &MeetingDetail) -> Roster {
    Roster::new(
        detail
            .segments
            .iter()
            .map(|s| (s.channel, s.speaker, s.speaker_override.as_deref())),
        detail
            .speakers
            .iter()
            .map(|n| (n.key.as_str(), n.display_name.as_str())),
    )
}

pub enum MeetingCmd {
    Reprocessed {
        id: String,
        segments: Vec<MeetingSegment>,
        reply: std::sync::mpsc::Sender<Result<(), String>>,
    },
    Started(NewMeeting),
    Segment {
        id: String,
        segment: MeetingSegment,
    },
    Stopped {
        id: String,
        ended_ms: i64,
    },
    /// The final pass replaced the live transcript.
    Refined {
        id: String,
        segments: Vec<MeetingSegment>,
    },
    /// Notes from the summary; `title` applies only while the meeting has none.
    Notes {
        id: String,
        notes_json: String,
        title: String,
    },
    /// Notes rewritten after moved lines or a re-run. Ticks carry over from
    /// the notes as stored now; the stale stamp clears only if it is not
    /// newer than `requested_ms`; the title never changes; a meeting with no
    /// notes is left without.
    RefreshedNotes {
        id: String,
        notes_json: String,
        requested_ms: i64,
    },
    /// Notes edited in the UI (an action item ticked).
    UpdateNotes {
        id: String,
        notes_json: String,
    },
    Rename {
        id: String,
        title: String,
    },
    /// The user named a speaker (`key` is a `SpeakerKey` as text); an empty
    /// name reverts it to its default label. Confirms a guess.
    SetSpeakerName {
        id: String,
        key: String,
        name: String,
    },
    /// Names from the pipeline after a final pass: guesses, and the "Your
    /// name" row. Never overwrites a name the user set.
    SpeakerNames {
        id: String,
        names: Vec<SpeakerName>,
    },
    /// Move one line to `key`, or back as transcribed (`None`). The reply is
    /// whether the notes are now stale, or a sentence for the user.
    AssignSpeaker {
        id: String,
        segment_id: i64,
        key: Option<String>,
        reply: std::sync::mpsc::Sender<Result<bool, String>>,
    },
    Finished {
        id: String,
        audio_bytes: u64,
    },
    /// The final pass's status (`meetings.transcript_status`); `None` clears it.
    TranscriptStatus {
        id: String,
        status: Option<String>,
    },
    /// The meeting's row and its audio.
    Delete {
        id: String,
        reply: std::sync::mpsc::Sender<Result<(), String>>,
    },
    /// Settings > Meetings "Delete all meeting audio": transcripts stay.
    DeleteAllAudio {
        protected: Vec<String>,
    },
    EnforceCap {
        cap_bytes: u64,
        protected: Vec<String>,
    },
    /// Startup: close out meetings the last run never finished.
    Recover,
}

/// Execute one command; `Ok(true)` when anything changed.
pub fn apply(store: &mut Store, dir: &Path, cmd: MeetingCmd) -> Result<bool, StoreError> {
    match cmd {
        MeetingCmd::Reprocessed {
            id,
            segments,
            reply,
        } => {
            // The old names go with the old diarization: bake them into the
            // notes in the same transaction that replaces the transcript.
            let baked = store.meeting(&id)?.and_then(|m| {
                let notes = hark_voice::MeetingNotes::from_json(m.notes_json.as_deref()?).ok()?;
                Some(notes.bake(&roster_of(&m).notes_names()).to_json())
            });
            let result = store.reprocess_meeting_segments(&id, &segments, baked.as_deref());
            let _ = reply.send(result.as_ref().map(|_| ()).map_err(|_| "The replacement transcript could not be saved. The previous transcript was kept.".into()));
            result.map(|_| true)
        }
        MeetingCmd::Started(m) => store.create_meeting(&m).map(|_| true),
        MeetingCmd::Segment { id, segment } => {
            store.append_meeting_segment(&id, &segment).map(|_| true)
        }
        MeetingCmd::Stopped { id, ended_ms } => store.finish_meeting(&id, ended_ms).map(|_| true),
        MeetingCmd::Refined { id, segments } => store
            .replace_meeting_segments(&id, &segments, true)
            .map(|_| true),
        MeetingCmd::Notes {
            id,
            notes_json,
            title,
        } => {
            store.set_meeting_notes(&id, Some(&notes_json))?;
            let Some(meeting) = store.meeting(&id)? else {
                return Ok(true);
            };
            let untitled = meeting
                .summary
                .title
                .as_deref()
                .is_none_or(|t| t.trim().is_empty());
            // A title is the user's own text once set: its speaker tokens are
            // resolved now, and it does not follow later renames.
            let title = resolve_title(&title, &roster_of(&meeting));
            if untitled && !title.trim().is_empty() {
                store.set_meeting_title(&id, title.trim())?;
            }
            Ok(true)
        }
        MeetingCmd::RefreshedNotes {
            id,
            notes_json,
            requested_ms,
        } => {
            let Some(meeting) = store.meeting(&id)? else {
                return Ok(false);
            };
            let Some(old) = meeting.notes_json.as_deref() else {
                log::info!("meeting {id}: rewritten notes dropped; the meeting has none");
                return Ok(false);
            };
            let json = carry_ticks(&notes_json, old, &roster_of(&meeting));
            store.set_refreshed_notes(&id, &json, requested_ms)
        }
        MeetingCmd::UpdateNotes { id, notes_json } => store
            .set_meeting_notes(&id, Some(&notes_json))
            .map(|_| true),
        MeetingCmd::Rename { id, title } => store.set_meeting_title(&id, title.trim()),
        MeetingCmd::SetSpeakerName { id, key, name } => {
            store.set_speaker_name(&id, &key, &name).map(|_| true)
        }
        MeetingCmd::SpeakerNames { id, names } => {
            let written = store.apply_speaker_names(&id, &names)?;
            log::info!(
                "meeting {id}: {written} of {} speaker names applied",
                names.len()
            );
            Ok(written > 0)
        }
        MeetingCmd::AssignSpeaker {
            id,
            segment_id,
            key,
            reply,
        } => {
            let now = jiff::Timestamp::now().as_millisecond();
            let result = store.assign_segment_speaker(&id, segment_id, key.as_deref(), now);
            let answer = match &result {
                Ok(Some(stale)) => Ok(*stale),
                // The line is gone (a re-run replaced the transcript) or the
                // write failed: the transaction left nothing half-done.
                Ok(None) | Err(_) => Err(ASSIGN_FAILED.to_string()),
            };
            if !matches!(result, Ok(Some(_))) {
                log::warn!("meeting {id}: segment {segment_id} not reassigned");
            }
            let _ = reply.send(answer);
            result.map(|moved| moved.is_some())
        }
        MeetingCmd::Finished { id, audio_bytes } => store
            .set_meeting_audio_bytes(&id, audio_bytes as i64)
            .map(|_| true),
        MeetingCmd::TranscriptStatus { id, status } => store
            .set_meeting_transcript_status(&id, status.as_deref())
            .map(|_| true),
        MeetingCmd::Delete { id, reply } => {
            let result = delete(store, dir, &id);
            let _ = reply.send(result.as_ref().map(|_| ()).map_err(Clone::clone));
            match result {
                Ok(changed) => Ok(changed),
                Err(error) => {
                    log::warn!("meeting {id}: {error}");
                    Ok(false)
                }
            }
        }
        MeetingCmd::DeleteAllAudio { protected } => evict(store, dir, 0, &protected, true),
        MeetingCmd::EnforceCap {
            cap_bytes,
            protected,
        } => evict(store, dir, cap_bytes, &protected, false),
        MeetingCmd::Recover => recover(store, dir),
    }
}

fn delete(store: &mut Store, dir: &Path, id: &str) -> Result<bool, String> {
    // The guard deletes only folders named by a meeting the database knows.
    if store
        .meeting(id)
        .map_err(|_| "The meeting could not be read. Try Delete again.")?
        .is_none()
    {
        return Ok(false);
    }
    match storage_fs::delete_audio(dir, id, &[id]) {
        Ok(bytes) => log::info!("meeting {id}: deleted with {bytes} bytes of audio"),
        // Only an absent recording is benign. NotFound can also come from
        // a disappearing nested file while the recording folder remains.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound
            && matches!(std::fs::symlink_metadata(dir.join(id)), Err(missing) if missing.kind() == std::io::ErrorKind::NotFound) => {}
        Err(e) => return Err(format!(
            "Audio could not be removed ({:?}). The meeting record was kept. Close any player using the recording, check file access, and try Delete again.",
            e.kind()
        )),
    }
    store.delete_meeting(id).map_err(|_| "Audio removal completed, but the meeting record could not be removed. Try Delete again.".to_string())
}

/// The storage cap (plan §4.9). `everything` evicts every unprotected
/// recording regardless of the cap (the Settings button).
fn evict(
    store: &mut Store,
    dir: &Path,
    cap_bytes: u64,
    protected: &[String],
    everything: bool,
) -> Result<bool, StoreError> {
    let index = store.meeting_audio_index()?;
    let known: Vec<&str> = index.iter().map(|(id, _)| id.as_str()).collect();
    let usage = match storage_fs::scan(dir, &known) {
        Ok(u) => u,
        Err(e) => {
            log::warn!("meeting storage scan failed ({e}); cap not enforced this time");
            return Ok(false);
        }
    };
    if !usage.stray.is_empty() {
        log::info!(
            "meeting storage: {} stray entries left alone",
            usage.stray.len()
        );
    }
    let recordings: Vec<StoredAudio> = usage
        .meetings
        .iter()
        .map(|(id, bytes)| StoredAudio {
            id: id.clone(),
            bytes: *bytes,
            ended_ms: index
                .iter()
                .find(|(i, _)| i == id)
                .and_then(|(_, ended)| ended.map(|e| e.max(0) as u64)),
        })
        .collect();
    let protected: Vec<&str> = protected.iter().map(String::as_str).collect();
    let plan = plan_eviction(
        &recordings,
        if everything { 0 } else { cap_bytes },
        &protected,
    );
    if plan.over_cap && !everything {
        log::info!(
            "meeting storage over the cap: {} bytes used, protected recordings alone exceed {cap_bytes}",
            plan.used_after
        );
    }
    let now = jiff::Timestamp::now().as_millisecond();
    let mut changed = false;
    for id in &plan.evict {
        match storage_fs::delete_audio(dir, id, &known) {
            Ok(bytes) => log::info!("meeting {id}: audio evicted ({bytes} bytes)"),
            Err(e) => {
                log::warn!("meeting {id}: eviction failed ({e})");
                continue;
            }
        }
        store.mark_meeting_audio_evicted(id, now)?;
        changed = true;
    }
    Ok(changed)
}

/// Close out meetings left open by a crash or a quit mid-meeting: the end is
/// the start plus what the spools hold.
fn recover(store: &mut Store, dir: &Path) -> Result<bool, StoreError> {
    let mut changed = false;
    for id in store.unfinished_meetings()? {
        let Some(meeting) = store.meeting(&id)? else {
            continue;
        };
        let meeting_dir = dir.join(&id);
        let samples = [hark_audio::spool::ME_FILE, hark_audio::spool::THEM_FILE]
            .iter()
            .filter_map(|f| std::fs::metadata(meeting_dir.join(f)).ok())
            .map(|m| m.len().saturating_sub(44) / 2)
            .max()
            .unwrap_or(0);
        let duration_ms = (samples * 1000 / hark_meeting::SAMPLE_RATE as u64) as i64;
        store.finish_meeting(&id, meeting.summary.started_ms + duration_ms)?;
        let bytes = storage_fs::scan(dir, &[id.as_str()])
            .map(|u| u.total())
            .unwrap_or(0);
        store.set_meeting_audio_bytes(&id, bytes as i64)?;
        log::info!("meeting {id}: closed out after an unfinished run ({duration_ms} ms)");
        changed = true;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_audio_delete_preserves_the_record_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory().unwrap();
        store
            .create_meeting(&NewMeeting {
                id: "m1".into(),
                started_ms: 1,
                trigger: "manual".into(),
                app_hint: None,
                stt_provider: "test".into(),
            })
            .unwrap();
        // A non-directory is refused on every platform, without permission
        // or symlink privileges and without touching real user recordings.
        std::fs::write(dir.path().join("m1"), b"synthetic retained bytes").unwrap();
        assert!(delete(&mut store, dir.path(), "m1").is_err());
        assert!(store.meeting("m1").unwrap().is_some());
        std::fs::remove_file(dir.path().join("m1")).unwrap();
        std::fs::create_dir(dir.path().join("m1")).unwrap();
        std::fs::write(dir.path().join("m1/archive.mp3"), b"synthetic audio").unwrap();
        assert!(delete(&mut store, dir.path(), "m1").unwrap());
        assert!(store.meeting("m1").unwrap().is_none());
        assert!(!dir.path().join("m1").exists());
    }

    #[test]
    fn deletion_command_replies_with_error_then_missing_audio_can_be_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory().unwrap();
        store
            .create_meeting(&NewMeeting {
                id: "m1".into(),
                started_ms: 1,
                trigger: "manual".into(),
                app_hint: None,
                stt_provider: "test".into(),
            })
            .unwrap();
        std::fs::write(dir.path().join("m1"), b"synthetic retained bytes").unwrap();
        let (reply, receiver) = std::sync::mpsc::channel();
        assert!(!apply(
            &mut store,
            dir.path(),
            MeetingCmd::Delete {
                id: "m1".into(),
                reply
            }
        )
        .unwrap());
        assert!(receiver
            .try_recv()
            .unwrap()
            .unwrap_err()
            .contains("meeting record was kept"));
        assert!(store.meeting("m1").unwrap().is_some());
        std::fs::remove_file(dir.path().join("m1")).unwrap();
        let (reply, receiver) = std::sync::mpsc::channel();
        assert!(apply(
            &mut store,
            dir.path(),
            MeetingCmd::Delete {
                id: "m1".into(),
                reply
            }
        )
        .unwrap());
        assert_eq!(receiver.try_recv().unwrap(), Ok(()));
        assert!(store.meeting("m1").unwrap().is_none());
    }

    /// A meeting with one Me line and one far-side line; returns their ids.
    fn two_lines(store: &mut Store, id: &str) -> [i64; 2] {
        store
            .create_meeting(&NewMeeting {
                id: id.into(),
                started_ms: 1,
                trigger: "manual".into(),
                app_hint: None,
                stt_provider: "test".into(),
            })
            .unwrap();
        for (channel, speaker) in [(0, None), (1, Some(0))] {
            let segment = MeetingSegment {
                start_ms: i64::from(channel) * 1_000,
                end_ms: i64::from(channel) * 1_000 + 500,
                channel,
                speaker,
                text: "synthetic line".into(),
            };
            store.append_meeting_segment(id, &segment).unwrap();
        }
        let d = store.meeting(id).unwrap().unwrap();
        [d.segments[0].id, d.segments[1].id]
    }

    fn assign(
        store: &mut Store,
        id: &str,
        segment_id: i64,
        key: Option<&str>,
    ) -> Result<bool, String> {
        let (reply, receiver) = std::sync::mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let cmd = MeetingCmd::AssignSpeaker {
            id: id.into(),
            segment_id,
            key: key.map(str::to_string),
            reply,
        };
        apply(store, dir.path(), cmd).unwrap();
        receiver.try_recv().unwrap()
    }

    #[test]
    fn a_generated_title_is_set_with_its_speaker_tokens_resolved() {
        let mut store = Store::open_in_memory().unwrap();
        two_lines(&mut store, "m");
        store.set_speaker_name("m", "them:0", "Dana").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cmd = MeetingCmd::Notes {
            id: "m".into(),
            notes_json: "{}".into(),
            title: "{{them:0}} and {{me}} planning".into(),
        };
        apply(&mut store, dir.path(), cmd).unwrap();
        let d = store.meeting("m").unwrap().unwrap();
        assert_eq!(d.summary.title.as_deref(), Some("Dana and Me planning"));
    }

    #[test]
    fn pipeline_names_never_replace_a_users_name() {
        let mut store = Store::open_in_memory().unwrap();
        two_lines(&mut store, "m");
        store.set_speaker_name("m", "them:0", "Dana").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let name = |key: &str, display_name: &str, source| SpeakerName {
            key: key.into(),
            display_name: display_name.into(),
            source,
            evidence_ms: None,
        };
        let cmd = MeetingCmd::SpeakerNames {
            id: "m".into(),
            names: vec![
                name("me", "Chaz", hark_store::NameSource::User),
                name("them:0", "Sam", hark_store::NameSource::Guess),
            ],
        };
        assert!(apply(&mut store, dir.path(), cmd).unwrap());
        let roster = roster_of(&store.meeting("m").unwrap().unwrap());
        assert_eq!(roster.label(hark_meeting::SpeakerKey::Me), "Chaz");
        assert_eq!(roster.label(hark_meeting::SpeakerKey::Diarized(0)), "Dana");
    }

    fn notes(items: &[(&str, bool)]) -> String {
        hark_voice::MeetingNotes {
            summary: "s".into(),
            action_items: items
                .iter()
                .map(|(text, done)| hark_voice::ActionItem {
                    text: (*text).into(),
                    owner: None,
                    done: *done,
                })
                .collect(),
            speaker_refs: hark_voice::SpeakerRefs::Tokens,
            ..Default::default()
        }
        .to_json()
    }

    #[test]
    fn rewritten_notes_keep_ticks_and_never_reach_a_meeting_without_notes() {
        let mut store = Store::open_in_memory().unwrap();
        let [_, them] = two_lines(&mut store, "m");
        let dir = tempfile::tempdir().unwrap();
        store.set_meeting_title("m", "Kept").unwrap();
        store
            .set_meeting_notes("m", Some(&notes(&[("Speaker 1 sends the deck", true)])))
            .unwrap();
        store
            .assign_segment_speaker("m", them, Some("me"), 5_000)
            .unwrap();
        let cmd = MeetingCmd::RefreshedNotes {
            id: "m".into(),
            notes_json: notes(&[("{{me}} sends the deck", false), ("Book a room", false)]),
            requested_ms: 6_000,
        };
        assert!(apply(&mut store, dir.path(), cmd).unwrap());
        let d = store.meeting("m").unwrap().unwrap();
        let written =
            hark_voice::MeetingNotes::from_json(d.notes_json.as_deref().unwrap()).unwrap();
        let ticks: Vec<bool> = written.action_items.iter().map(|a| a.done).collect();
        assert_eq!(ticks, [true, false]);
        assert_eq!(d.notes_stale_ms, None);
        assert_eq!(d.summary.title.as_deref(), Some("Kept"));

        two_lines(&mut store, "bare");
        let cmd = MeetingCmd::RefreshedNotes {
            id: "bare".into(),
            notes_json: notes(&[]),
            requested_ms: 1,
        };
        assert!(!apply(&mut store, dir.path(), cmd).unwrap());
        assert!(store.meeting("bare").unwrap().unwrap().notes_json.is_none());
    }

    #[test]
    fn a_reprocess_bakes_the_old_names_into_the_notes() {
        let mut store = Store::open_in_memory().unwrap();
        two_lines(&mut store, "m");
        store.set_speaker_name("m", "them:0", "Dana").unwrap();
        let tokens = hark_voice::parse_notes(
            r#"{"title":"t","summary":"{Speaker 1} presented."}"#,
            &[("Speaker 1".to_string(), "them:0".to_string())],
        )
        .unwrap();
        store
            .set_meeting_notes("m", Some(&tokens.to_json()))
            .unwrap();
        let (reply, done) = std::sync::mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let cmd = MeetingCmd::Reprocessed {
            id: "m".into(),
            segments: vec![MeetingSegment {
                start_ms: 0,
                end_ms: 1,
                channel: 1,
                speaker: Some(4),
                text: "new".into(),
            }],
            reply,
        };
        apply(&mut store, dir.path(), cmd).unwrap();
        assert_eq!(done.try_recv().unwrap(), Ok(()));
        let d = store.meeting("m").unwrap().unwrap();
        assert!(d.speakers.is_empty());
        let baked = hark_voice::MeetingNotes::from_json(d.notes_json.as_deref().unwrap()).unwrap();
        assert_eq!(baked.speaker_refs, hark_voice::SpeakerRefs::Frozen);
        // Speaker 1 is someone else after the re-run; the notes keep Dana.
        assert_eq!(
            baked.render(&roster_of(&d).notes_names()).summary,
            "Dana presented."
        );
    }

    #[test]
    fn a_reassignment_replies_whether_the_notes_went_stale() {
        let mut store = Store::open_in_memory().unwrap();
        let [_, them] = two_lines(&mut store, "m");
        let [other_me, _] = two_lines(&mut store, "other");
        assert_eq!(assign(&mut store, "m", them, Some("me")), Ok(false));
        store.set_meeting_notes("m", Some("{}")).unwrap();
        assert_eq!(assign(&mut store, "m", them, None), Ok(true));
        assert!(store
            .meeting("m")
            .unwrap()
            .unwrap()
            .notes_stale_ms
            .is_some());
        // Another meeting's line: refused, and nothing moved anywhere.
        assert_eq!(
            assign(&mut store, "m", other_me, Some("them:0")),
            Err(ASSIGN_FAILED.to_string())
        );
        let other = store.meeting("other").unwrap().unwrap();
        assert!(other.segments[0].speaker_override.is_none());
    }

    #[test]
    fn a_users_rename_reaches_the_roster_and_an_empty_one_reverts() {
        let mut store = Store::open_in_memory().unwrap();
        two_lines(&mut store, "m");
        let dir = tempfile::tempdir().unwrap();
        let mut rename = |name: &str| {
            let cmd = MeetingCmd::SetSpeakerName {
                id: "m".into(),
                key: "me".into(),
                name: name.into(),
            };
            apply(&mut store, dir.path(), cmd).unwrap();
            roster_of(&store.meeting("m").unwrap().unwrap()).label(hark_meeting::SpeakerKey::Me)
        };
        assert_eq!(rename("Chaz"), "Chaz");
        assert_eq!(rename(" "), "Me");
    }
}
