//! Meeting writes on the storage worker: the one writer for both the database
//! rows and the meeting audio folders, so the storage cap sees a consistent
//! view of both (plan §4.9: sizes from the filesystem, ids from the database).
//!
//! Commands come from the meeting event pump (`crate::meeting`) and from the
//! Meetings page. Labels, ids and byte counts only in logs; never text.

use hark_meeting::storage_fs;
use hark_meeting::{plan_eviction, StoredAudio};
use hark_store::{MeetingSegment, NewMeeting, Store, StoreError};
use std::path::Path;

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
    /// Notes edited in the UI (an action item ticked).
    UpdateNotes {
        id: String,
        notes_json: String,
    },
    Rename {
        id: String,
        title: String,
    },
    RenameSpeaker {
        id: String,
        speaker: u32,
        name: String,
    },
    Finished {
        id: String,
        audio_bytes: u64,
    },
    /// The meeting's row and its audio.
    Delete {
        id: String,
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
            let result = store.reprocess_meeting_segments(&id, &segments);
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
            let untitled = store.meeting(&id)?.is_some_and(|m| {
                m.summary
                    .title
                    .as_deref()
                    .is_none_or(|t| t.trim().is_empty())
            });
            if untitled && !title.trim().is_empty() {
                store.set_meeting_title(&id, title.trim())?;
            }
            Ok(true)
        }
        MeetingCmd::UpdateNotes { id, notes_json } => store
            .set_meeting_notes(&id, Some(&notes_json))
            .map(|_| true),
        MeetingCmd::Rename { id, title } => store.set_meeting_title(&id, title.trim()),
        MeetingCmd::RenameSpeaker { id, speaker, name } => store
            .rename_meeting_speaker(&id, speaker, &name)
            .map(|_| true),
        MeetingCmd::Finished { id, audio_bytes } => store
            .set_meeting_audio_bytes(&id, audio_bytes as i64)
            .map(|_| true),
        MeetingCmd::Delete { id } => delete(store, dir, &id),
        MeetingCmd::DeleteAllAudio { protected } => evict(store, dir, 0, &protected, true),
        MeetingCmd::EnforceCap {
            cap_bytes,
            protected,
        } => evict(store, dir, cap_bytes, &protected, false),
        MeetingCmd::Recover => recover(store, dir),
    }
}

fn delete(store: &mut Store, dir: &Path, id: &str) -> Result<bool, StoreError> {
    // The guard deletes only folders named by a meeting the database knows.
    if store.meeting(id)?.is_none() {
        return Ok(false);
    }
    match storage_fs::delete_audio(dir, id, &[id]) {
        Ok(bytes) => log::info!("meeting {id}: deleted with {bytes} bytes of audio"),
        // Already evicted, or never recorded anything.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("meeting {id}: audio not deleted ({e}); removing the record anyway"),
    }
    store.delete_meeting(id)
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
