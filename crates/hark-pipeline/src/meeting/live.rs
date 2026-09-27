//! The live transcriber: one thread per meeting, taking chunks from both
//! channels in arrival order (so each channel stays sequential) and sending
//! each transcribed line to the UI as it lands.
//!
//! It uses the dictation provider (or the on-device engine in primary local
//! mode). A failed chunk costs one live line, never audio: the spool has it,
//! and the final pass (if configured) transcribes the whole call again.

use super::recorder::LiveJob;
use super::{LiveSegment, MeetingEvent};
use hark_meeting::{Channel, Segment, Transcript};
use hark_stt::SttProvider;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// What transcribes the chunks.
pub(super) enum Engine {
    Cloud(Box<dyn SttProvider>),
    /// Built inside the thread: the on-device engine is not `Send`.
    Local(Box<hark_config::Settings>),
}

pub(super) struct Live {
    pub jobs: Sender<LiveJob>,
    pub thread: JoinHandle<()>,
    /// Every line so far, in timeline order, for the summary when no final
    /// pass replaces it.
    pub transcript: Arc<Mutex<Transcript>>,
}

pub(super) fn spawn(
    id: String,
    engine: Engine,
    corrector: hark_spellbook::Corrector,
    events: Sender<MeetingEvent>,
) -> Result<Live, String> {
    let (jobs, rx) = mpsc::channel();
    let transcript = Arc::new(Mutex::new(Transcript::new()));
    let shared = transcript.clone();
    let thread = std::thread::Builder::new()
        .name("hark-meeting-live".to_string())
        .spawn(move || run(id, engine, corrector, events, shared, rx))
        .map_err(|e| format!("cannot start the live transcriber: {e}"))?;
    Ok(Live {
        jobs,
        thread,
        transcript,
    })
}

fn run(
    id: String,
    engine: Engine,
    corrector: hark_spellbook::Corrector,
    events: Sender<MeetingEvent>,
    transcript: Arc<Mutex<Transcript>>,
    rx: Receiver<LiveJob>,
) {
    let mut local = match &engine {
        Engine::Local(settings) => crate::build_local(settings),
        Engine::Cloud(_) => None,
    };
    let mut failures = 0u32;
    // Ends when the recorder drops its sender (meeting stopped) and the
    // queue is drained.
    for (channel, chunk) in rx {
        let text = match &engine {
            Engine::Cloud(provider) => transcribe_cloud(provider.as_ref(), &chunk.samples),
            Engine::Local(_) => match local.as_mut() {
                Some(plan) => plan
                    .engine()
                    .and_then(|e| e.transcribe(&chunk.samples))
                    .map(|t| t.text)
                    .map_err(|e| e.to_string()),
                None => Err("the on-device model is not available".to_string()),
            },
        };
        let text = match text {
            Ok(text) => text,
            Err(detail) => {
                failures += 1;
                log::warn!("meeting live chunk failed ({failures} so far): {detail}");
                if failures == 1 {
                    let _ = events.send(MeetingEvent::Notice {
                        id: Some(id.clone()),
                        text: format!(
                            "Some live lines could not be transcribed ({detail}). \
                             The recording itself is unaffected."
                        ),
                    });
                }
                continue;
            }
        };
        let (text, _) = corrector.correct(text.trim());
        if text.trim().is_empty() {
            continue;
        }
        let to_ms = |s: u64| s * 1000 / hark_meeting::SAMPLE_RATE as u64;
        let segment = LiveSegment {
            channel: match channel {
                Channel::Me => 0,
                Channel::Them => 1,
            },
            speaker: None,
            start_ms: to_ms(chunk.start),
            end_ms: to_ms(chunk.end()),
            text: text.clone(),
        };
        if let Ok(mut t) = transcript.lock() {
            t.insert(Segment {
                channel,
                start: chunk.start,
                end: chunk.end(),
                text,
            });
        }
        if events
            .send(MeetingEvent::Segment {
                id: id.clone(),
                segment,
            })
            .is_err()
        {
            break;
        }
    }
}

/// One request, and one retry for the failures a retry can fix (the same
/// rule dictation uses).
fn transcribe_cloud(provider: &dyn SttProvider, samples: &[f32]) -> Result<String, String> {
    let wav = hark_stt::wav::encode_wav_16k_mono(samples);
    let mut result = provider.transcribe(&wav);
    if let Err(e) = &result {
        if crate::should_retry(e) {
            result = provider.transcribe(&wav);
        }
    }
    result.map(|t| t.text).map_err(|e| e.to_string())
}
