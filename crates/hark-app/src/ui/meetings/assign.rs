//! Moving transcript lines between speakers: the commands a menu pick sends,
//! and the storage worker's answers. A reply that never comes back is a
//! failure, never a success (spec §10).

use super::transcript::Reassign;
use crate::storage::meetings::{MeetingCmd, ASSIGN_FAILED};
use crate::storage::{StorageCmd, StorageHandle};
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(super) struct Assignments {
    /// Reassignments the storage worker has not answered yet.
    pending: Vec<(String, Receiver<Result<bool, String>>)>,
    error: Option<(String, String)>,
}

impl Assignments {
    pub fn new() -> Self {
        Assignments {
            pending: Vec::new(),
            error: None,
        }
    }

    /// The last failure for `id`, for the line under the transcript header.
    pub fn error(&self, id: &str) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(failed, _)| failed == id)
            .map(|(_, e)| e.as_str())
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Drain the answers. Returns the ids whose notes a move made stale.
    pub fn poll(&mut self) -> Vec<String> {
        let mut answered = Vec::new();
        self.pending.retain(|(id, reply)| {
            let result = match reply.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => Err(ASSIGN_FAILED.to_string()),
            };
            answered.push((id.clone(), result));
            false
        });
        let mut stale = Vec::new();
        for (id, result) in answered {
            match result {
                Ok(now_stale) => {
                    if self.error(&id).is_some() {
                        self.error = None;
                    }
                    if now_stale && !stale.contains(&id) {
                        stale.push(id);
                    }
                }
                Err(error) => self.error = Some((id, error)),
            }
        }
        stale
    }

    pub fn send(&mut self, storage: &StorageHandle, id: &str, choice: Reassign) {
        let (segment_id, key) = match choice {
            Reassign::Move { segment_id, key } => (segment_id, key),
            Reassign::New {
                segment_id,
                key,
                name,
            } => {
                // Named first: both go down the one storage queue, in order.
                if !name.is_empty() {
                    storage.send(StorageCmd::Meeting(MeetingCmd::SetSpeakerName {
                        id: id.to_string(),
                        key: key.to_string(),
                        name,
                    }));
                }
                (segment_id, Some(key))
            }
        };
        let (reply, receiver) = mpsc::channel();
        self.pending.push((id.to_string(), receiver));
        storage.send(StorageCmd::Meeting(MeetingCmd::AssignSpeaker {
            id: id.to_string(),
            segment_id,
            key: key.map(|k| k.to_string()),
            reply,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_report_stale_notes_and_failures_and_a_lost_reply_is_a_failure() {
        let mut a = Assignments::new();
        let (ok, rx) = mpsc::channel();
        a.pending.push(("m1".into(), rx));
        let (fresh, rx) = mpsc::channel();
        a.pending.push(("m2".into(), rx));
        let (lost, rx) = mpsc::channel::<Result<bool, String>>();
        a.pending.push(("m3".into(), rx));
        assert!(a.poll().is_empty(), "nothing answered yet");
        ok.send(Ok(true)).unwrap();
        fresh.send(Ok(false)).unwrap();
        drop(lost);
        assert_eq!(a.poll(), ["m1"]);
        assert_eq!(a.error("m3"), Some(ASSIGN_FAILED));
        assert_eq!(a.error("m1"), None);
        assert!(a.pending.is_empty());

        // A later success for the same meeting clears its error line.
        let (ok, rx) = mpsc::channel();
        a.pending.push(("m3".into(), rx));
        ok.send(Ok(false)).unwrap();
        a.poll();
        assert_eq!(a.error("m3"), None);
    }
}
