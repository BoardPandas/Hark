//! One turn's progress through the protocol: which item is this turn's, and
//! whether its transcript has arrived.

use super::server_error::{remote_error, RemoteError};
use super::wire::{parse_server_message, ServerEvent};
use crate::error::SttError;

/// How many pre-commit outcomes [`Turn`] keeps. A single-turn session with no
/// VAD never has another item, so this only bounds a misbehaving server.
const EARLY_OUTCOMES_MAX: usize = 8;

/// One turn's progress, fed every frame the server sends.
///
/// The single place a transcript is collected: the drain that runs during the
/// hold and the loop that finalises the turn both go through [`Turn::absorb`],
/// because Gemini Live once lost every word before the last pause to one read
/// path keeping frames and the other dropping them.
#[derive(Default)]
pub struct Turn {
    /// Whether our own `input_audio_buffer.commit` has gone out. A
    /// `committed` before it answers something else (a server-side commit),
    /// and taking that item as ours would return its phrase as the dictation.
    commit_sent: bool,
    /// This turn's item, from the first `input_audio_buffer.committed` after
    /// our commit.
    item_id: Option<String>,
    /// Outcomes that named an item before `committed` said which one is ours.
    /// The server sends `committed` first (CP0), but transcription is
    /// asynchronous to the other events, so this does not rely on it.
    early: Vec<(String, Result<String, RemoteError>)>,
    transcript: Option<String>,
    configured: bool,
    frames: u32,
    deltas: u32,
}

impl Turn {
    /// Record one frame. `Ok(true)` once this turn's transcript has arrived;
    /// an `error` event, or a failure for this turn's item, is an `Err`.
    pub fn absorb(&mut self, body: &str) -> Result<bool, SttError> {
        self.frames += 1;
        match parse_server_message(body)? {
            ServerEvent::SessionCreated | ServerEvent::Other => {}
            ServerEvent::SessionUpdated => self.configured = true,
            ServerEvent::Delta => self.deltas += 1,
            ServerEvent::Committed { item_id } => {
                if !self.commit_sent {
                    // `turn_detection` is null, so the server should never
                    // commit on its own. If it ever does, that item holds only
                    // the audio before it: accepting it would inject the first
                    // phrase as if it were the whole dictation.
                    log::warn!(
                        "openai live: the server committed audio before our commit; \
                         ignoring that item"
                    );
                } else if self.item_id.is_some() {
                    log::debug!("openai live: a second commit acknowledgement; keeping the first");
                } else {
                    if let Some(at) = self.early.iter().position(|(id, _)| *id == item_id) {
                        let (_, outcome) = self.early.swap_remove(at);
                        self.settle(outcome)?;
                    }
                    self.item_id = Some(item_id);
                }
            }
            ServerEvent::Completed {
                item_id,
                transcript,
            } => self.outcome(item_id, Ok(transcript))?,
            ServerEvent::Failed { item_id, error } => self.outcome(item_id, Err(error))?,
            ServerEvent::Error(error) => {
                let context = if self.configured {
                    "server error"
                } else {
                    "server error before the session was configured"
                };
                return Err(remote_error(context, &error));
            }
        }
        Ok(self.transcript.is_some())
    }

    fn outcome(
        &mut self,
        item_id: String,
        outcome: Result<String, RemoteError>,
    ) -> Result<(), SttError> {
        match &self.item_id {
            Some(ours) if *ours == item_id => self.settle(outcome),
            Some(_) => {
                log::debug!("openai live: ignoring an outcome for another item");
                Ok(())
            }
            None => {
                if self.early.len() == EARLY_OUTCOMES_MAX {
                    drop(self.early.remove(0));
                }
                self.early.push((item_id, outcome));
                Ok(())
            }
        }
    }

    fn settle(&mut self, outcome: Result<String, RemoteError>) -> Result<(), SttError> {
        match outcome {
            Ok(text) => {
                self.transcript = Some(text);
                Ok(())
            }
            Err(error) => Err(remote_error("transcription failed", &error)),
        }
    }

    /// Record that our commit has been sent: only a `committed` from here on
    /// can name this turn's item.
    pub fn sent_commit(&mut self) {
        self.commit_sent = true;
    }

    /// This turn's transcript, once it has arrived.
    pub fn take_transcript(&mut self) -> Option<String> {
        self.transcript.take()
    }

    /// Whether `committed` has named this turn's item yet.
    pub fn committed(&self) -> bool {
        self.item_id.is_some()
    }

    /// Whether `session.updated` has arrived.
    pub fn configured(&self) -> bool {
        self.configured
    }

    /// Server frames seen so far, for diagnostics.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// Partial transcripts seen so far, for diagnostics.
    pub fn deltas(&self) -> u32 {
        self.deltas
    }
}
