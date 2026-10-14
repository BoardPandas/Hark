//! The live transcriber: one thread per meeting, taking chunks from both
//! channels in arrival order (so each channel stays sequential) and sending
//! each transcribed line to the UI as it lands.
//!
//! It uses the dictation provider (or the on-device engine in primary local
//! mode). A failed chunk costs one live line, never audio: the spool has it,
//! and the final pass (if configured) transcribes the whole call again.
//!
//! On a weak uplink the cloud path backs off ([`Backoff`]): after three
//! network failures in a row it stops uploading for two minutes, so Hark is
//! not competing with the call itself for bandwidth. Chunks that failed or
//! arrived during the pause are held (up to five minutes of audio) and sent
//! once a request gets through again. Lines arriving late still land in
//! timeline order: the transcript and the live pane both insert by start.

use super::recorder::LiveJob;
use super::{LiveSegment, MeetingEvent};
use hark_meeting::{Channel, Chunk, Segment, Transcript};
use hark_stt::meeting::FailureKind;
use hark_stt::{SttError, SttProvider};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Network failures in a row that pause live uploads.
const PAUSE_AFTER: u32 = 3;
const PAUSE: Duration = Duration::from_secs(120);
/// Audio held for resending, in samples across both channels: 5 minutes.
const HOLD_SAMPLES: usize = 5 * 60 * hark_meeting::SAMPLE_RATE as usize;
const PAUSED_NOTICE: &str =
    "Live transcript paused (slow connection). The full transcript is made after the call.";

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
        .spawn(move || {
            Transcriber {
                id,
                engine,
                local: None,
                corrector,
                events,
                transcript: shared,
                failures: 0,
                paused_notice: false,
            }
            .run(rx)
        })
        .map_err(|e| format!("cannot start the live transcriber: {e}"))?;
    Ok(Live {
        jobs,
        thread,
        transcript,
    })
}

/// When to send live chunks, and which to keep for later. Generic over the
/// held item so the policy is testable without audio or a provider.
struct Backoff<T> {
    /// Network failures since the last success.
    in_a_row: u32,
    paused_until: Option<Instant>,
    held: VecDeque<(T, usize)>,
    held_samples: usize,
}

impl<T> Backoff<T> {
    fn new() -> Self {
        Backoff {
            in_a_row: 0,
            paused_until: None,
            held: VecDeque::new(),
            held_samples: 0,
        }
    }

    fn paused(&self, now: Instant) -> bool {
        self.paused_until.is_some_and(|until| now < until)
    }

    /// Keep a chunk to send later; past the cap, the oldest go first (the
    /// final pass still has them). Returns how many were dropped.
    fn hold(&mut self, item: T, samples: usize) -> usize {
        self.held.push_back((item, samples));
        self.held_samples += samples;
        let mut dropped = 0;
        while self.held_samples > HOLD_SAMPLES && self.held.len() > 1 {
            if let Some((_, n)) = self.held.pop_front() {
                self.held_samples -= n;
                dropped += 1;
            }
        }
        dropped
    }

    /// A network failure. True when it starts a pause, including the one
    /// that follows a failed first request after a pause ends.
    fn failed(&mut self, now: Instant) -> bool {
        self.in_a_row += 1;
        if self.in_a_row >= PAUSE_AFTER && !self.paused(now) {
            self.paused_until = Some(now + PAUSE);
            return true;
        }
        false
    }

    /// A request got through: the held chunks to resend, oldest first.
    fn succeeded(&mut self) -> Vec<T> {
        self.in_a_row = 0;
        self.paused_until = None;
        self.held_samples = 0;
        self.held.drain(..).map(|(item, _)| item).collect()
    }
}

struct Transcriber {
    id: String,
    engine: Engine,
    local: Option<crate::local::LocalPlan>,
    corrector: hark_spellbook::Corrector,
    events: Sender<MeetingEvent>,
    transcript: Arc<Mutex<Transcript>>,
    failures: u32,
    paused_notice: bool,
}

impl Transcriber {
    fn run(mut self, rx: Receiver<LiveJob>) {
        if let Engine::Local(settings) = &self.engine {
            self.local = crate::build_local(settings);
        }
        let mut backoff = Backoff::new();
        // Ends when the recorder drops its sender (meeting stopped) and the
        // queue is drained.
        for (channel, chunk) in rx {
            let cloud = matches!(self.engine, Engine::Cloud(_));
            if cloud && backoff.paused(Instant::now()) {
                let samples = chunk.samples.len();
                self.hold(&mut backoff, (channel, chunk), samples);
                continue;
            }
            match self.transcribe(&chunk) {
                Ok(text) => {
                    if !self.emit(channel, &chunk, &text) {
                        return;
                    }
                    if cloud && !self.resend(&mut backoff) {
                        return;
                    }
                }
                Err(error) => self.failed(&mut backoff, cloud, (channel, chunk), &error),
            }
        }
        // One last try for what is held, unless the link is known to be
        // down: the finisher is waiting on this thread, and the final pass
        // transcribes the whole call anyway.
        if !backoff.paused(Instant::now()) && !backoff.held.is_empty() {
            self.resend(&mut backoff);
        }
        if !backoff.held.is_empty() {
            log::info!(
                "meeting {}: {} held live chunks left to the final pass",
                self.id,
                backoff.held.len()
            );
        }
    }

    /// Send the held chunks after a success. Stops at the first failure and
    /// holds the rest again. False when the UI has gone.
    fn resend(&mut self, backoff: &mut Backoff<LiveJob>) -> bool {
        let mut held = backoff.succeeded().into_iter();
        while let Some((channel, chunk)) = held.next() {
            match self.transcribe(&chunk) {
                Ok(text) => {
                    if !self.emit(channel, &chunk, &text) {
                        return false;
                    }
                }
                Err(error) => {
                    self.failed(backoff, true, (channel, chunk), &error);
                    for (channel, chunk) in held {
                        let samples = chunk.samples.len();
                        self.hold(backoff, (channel, chunk), samples);
                    }
                    break;
                }
            }
        }
        true
    }

    fn failed(
        &mut self,
        backoff: &mut Backoff<LiveJob>,
        cloud: bool,
        job: LiveJob,
        error: &SttError,
    ) {
        self.failures += 1;
        log::warn!(
            "meeting live chunk failed ({} so far): {error}",
            self.failures
        );
        if self.failures == 1 {
            self.notice(format!(
                "Some live lines could not be transcribed ({error}). \
                 The recording itself is unaffected."
            ));
        }
        // Only the network gets better by waiting; a refused key does not.
        if !cloud || !FailureKind::of(error).retryable() {
            return;
        }
        let samples = job.1.samples.len();
        self.hold(backoff, job, samples);
        if backoff.failed(Instant::now()) {
            log::info!(
                "meeting {}: live uploads paused for {} s",
                self.id,
                PAUSE.as_secs()
            );
            if !self.paused_notice {
                self.paused_notice = true;
                self.notice(PAUSED_NOTICE.to_string());
            }
        }
    }

    fn hold(&self, backoff: &mut Backoff<LiveJob>, job: LiveJob, samples: usize) {
        let dropped = backoff.hold(job, samples);
        if dropped > 0 {
            log::info!(
                "meeting {}: {dropped} held live chunks dropped (over 5 minutes held)",
                self.id
            );
        }
    }

    fn transcribe(&mut self, chunk: &Chunk) -> Result<String, SttError> {
        match &self.engine {
            Engine::Cloud(provider) => transcribe_cloud(provider.as_ref(), &chunk.samples),
            Engine::Local(_) => match self.local.as_mut() {
                Some(plan) => plan
                    .engine()
                    .and_then(|e| e.transcribe(&chunk.samples))
                    .map(|t| t.text)
                    .map_err(|e| SttError::BadAudio(e.to_string())),
                None => Err(SttError::BadAudio(
                    "the on-device model is not available".to_string(),
                )),
            },
        }
    }

    /// One transcribed line to the transcript and the UI. False when the UI
    /// has gone.
    fn emit(&mut self, channel: Channel, chunk: &Chunk, text: &str) -> bool {
        let (text, _) = self.corrector.correct(text.trim());
        if text.trim().is_empty() {
            return true;
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
        if let Ok(mut t) = self.transcript.lock() {
            t.insert(Segment {
                channel,
                start: chunk.start,
                end: chunk.end(),
                text,
            });
        }
        self.events
            .send(MeetingEvent::Segment {
                id: self.id.clone(),
                segment,
            })
            .is_ok()
    }

    fn notice(&self, text: String) {
        let _ = self.events.send(MeetingEvent::Notice {
            id: Some(self.id.clone()),
            text,
        });
    }
}

/// One request, and one retry for the failures a retry can fix (the same
/// rule dictation uses).
fn transcribe_cloud(provider: &dyn SttProvider, samples: &[f32]) -> Result<String, SttError> {
    let wav = hark_stt::wav::encode_wav_16k_mono(samples);
    let mut result = provider.transcribe(&wav);
    if let Err(e) = &result {
        if crate::should_retry(e) {
            result = provider.transcribe(&wav);
        }
    }
    result.map(|t| t.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: usize = 30 * 16_000;

    #[test]
    fn three_failures_in_a_row_pause_for_two_minutes() {
        let t0 = Instant::now();
        let mut b: Backoff<u32> = Backoff::new();
        assert!(!b.failed(t0));
        assert!(!b.failed(t0));
        assert!(!b.paused(t0));
        assert!(b.failed(t0), "the third starts the pause");
        assert!(b.paused(t0 + Duration::from_secs(119)));
        assert!(!b.paused(t0 + PAUSE));
    }

    #[test]
    fn a_success_resets_the_count() {
        let t0 = Instant::now();
        let mut b: Backoff<u32> = Backoff::new();
        b.failed(t0);
        b.failed(t0);
        b.succeeded();
        assert!(!b.failed(t0));
        assert!(!b.paused(t0));
    }

    #[test]
    fn a_failed_first_request_after_the_pause_pauses_again() {
        let t0 = Instant::now();
        let mut b: Backoff<u32> = Backoff::new();
        for _ in 0..3 {
            b.failed(t0);
        }
        let after = t0 + PAUSE;
        assert!(!b.paused(after));
        assert!(b.failed(after), "still down: straight back to waiting");
        assert!(b.paused(after + Duration::from_secs(1)));
    }

    #[test]
    fn held_chunks_come_back_oldest_first_after_a_success() {
        let mut b: Backoff<u32> = Backoff::new();
        b.hold(1, CHUNK);
        b.hold(2, CHUNK);
        b.hold(3, CHUNK);
        assert_eq!(b.succeeded(), [1, 2, 3]);
        assert!(b.succeeded().is_empty(), "sent once, not twice");
        assert_eq!(b.held_samples, 0);
    }

    #[test]
    fn at_most_five_minutes_of_audio_is_held() {
        let mut b: Backoff<u32> = Backoff::new();
        let mut dropped = 0;
        for i in 0..12 {
            dropped += b.hold(i, CHUNK);
        }
        // Twelve 30 s chunks are six minutes; the two oldest go.
        assert_eq!(dropped, 2);
        assert_eq!(b.held_samples, HOLD_SAMPLES);
        assert_eq!(b.succeeded().first(), Some(&2));
    }

    #[test]
    fn one_oversized_chunk_is_still_held() {
        let mut b: Backoff<u32> = Backoff::new();
        assert_eq!(b.hold(7, HOLD_SAMPLES * 2), 0);
        assert_eq!(b.succeeded(), [7]);
    }

    #[test]
    fn the_paused_notice_says_the_full_transcript_still_comes() {
        assert_eq!(
            PAUSED_NOTICE,
            "Live transcript paused (slow connection). The full transcript is made after the call."
        );
    }
}
