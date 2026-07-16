//! The pipeline worker loop: receive chord edges, assemble the window,
//! encode, transcribe (one retry max), inject. Runs on its own thread; all
//! logging is lengths/counts/millis, never key material, audio, or text.

use crate::retry::should_retry;
use crate::state::{advance, Action, Event, PipelineState};
use hark_audio::ring::Consumer;
use hark_audio::WindowParams;
use hark_hotkey::PttEvent;
use hark_inject::InjectSettings;
use hark_stt::{SttError, SttProvider, Transcript};
use std::sync::mpsc::Receiver;
use std::time::Instant;

pub(crate) struct Worker {
    pub consumer: Consumer,
    pub sample_rate: u32,
    pub window: WindowParams,
    pub inject: InjectSettings,
    pub provider: Box<dyn SttProvider>,
    /// Base URL to pre-warm (DNS + TCP + TLS) before the first dictation.
    pub prewarm_url: String,
    /// The shared HTTP client (same instance the provider adapter holds),
    /// used only for the pre-warm request.
    pub client: reqwest::blocking::Client,
}

/// The one long-lived worker loop. Exits when the hotkey listener drops its
/// sender (pipeline shutdown).
pub(crate) fn run(worker: Worker, rx: Receiver<PttEvent>) {
    prewarm(&worker);

    let mut state = PipelineState::Idle;
    while let Ok(event) = rx.recv() {
        // Correlate the edge with the audio clock at processing time. The
        // pre-roll absorbs hook->worker latency; a Down processed late while
        // a previous dictation was still in flight is ignored by the state
        // machine (spec §3.19).
        let at_abs = worker.consumer.total_written();
        let ev = match event {
            PttEvent::Down => Event::PttDown { at_abs },
            PttEvent::Up => Event::PttUp { at_abs },
        };
        let (next, action) = advance(state, ev);
        state = next;
        if let Action::Dictate { down_abs, up_abs } = action {
            state = dictate(&worker, down_abs, up_abs, state);
        }
    }
    log::debug!("ptt channel closed; pipeline worker exiting");
}

/// Warm the connection pool so the first dictation skips the 0.4-0.9 s
/// cold-start the spike measured (DNS + TCP + TLS handshake). The response
/// itself is irrelevant; failures only cost the warmth.
fn prewarm(worker: &Worker) {
    let started = Instant::now();
    match worker.client.get(&worker.prewarm_url).send() {
        Ok(_) => log::info!(
            "http client pre-warmed in {} ms",
            started.elapsed().as_millis()
        ),
        Err(e) => log::warn!(
            "http pre-warm failed after {} ms (first dictation will pay the cold cost): {e}",
            started.elapsed().as_millis()
        ),
    }
}

/// One full dictation: assemble -> gate -> encode -> transcribe -> inject.
/// Always returns the post-dictation state (Idle via Injected or Aborted).
fn dictate(worker: &Worker, down_abs: u64, up_abs: u64, state: PipelineState) -> PipelineState {
    let released = Instant::now();

    let clip = match hark_audio::assemble_window(
        &worker.consumer,
        worker.sample_rate,
        down_abs,
        up_abs,
        &worker.window,
    ) {
        Ok(Some(clip)) => clip,
        Ok(None) => {
            log::info!("dictation gated (too short or silent); no request sent");
            return advance(state, Event::Aborted).0;
        }
        Err(e) => {
            log::error!("window assembly failed: {e}");
            return advance(state, Event::Aborted).0;
        }
    };

    let encode_started = Instant::now();
    let wav = hark_stt::wav::encode_wav_16k_mono(&clip.samples_16k);
    log::debug!(
        "clip: {} samples at 16 kHz ({} bytes WAV, encoded in {} ms)",
        clip.samples_16k.len(),
        wav.len(),
        encode_started.elapsed().as_millis()
    );

    let transcript = match transcribe_with_retry(worker.provider.as_ref(), &wav) {
        Ok(t) => t,
        Err(e) => {
            log::error!("transcription failed: {e}");
            return advance(state, Event::Aborted).0;
        }
    };
    let state = advance(state, Event::TranscriptReady).0;

    if transcript.text.trim().is_empty() {
        log::info!("provider returned an empty transcript; nothing to inject");
        return advance(state, Event::Aborted).0;
    }

    match hark_inject::inject(&transcript.text, &worker.inject) {
        Ok(()) => {
            log::info!(
                "dictation injected: {} chars, request {} ms, release-to-inject {} ms",
                transcript.text.chars().count(),
                transcript.request_ms,
                released.elapsed().as_millis()
            );
            advance(state, Event::Injected).0
        }
        Err(e) => {
            log::error!("injection failed: {e}");
            advance(state, Event::Aborted).0
        }
    }
}

/// At most one retry, and only when `should_retry` says the failure class
/// warrants it (timeout / connect-class).
fn transcribe_with_retry(provider: &dyn SttProvider, wav: &[u8]) -> Result<Transcript, SttError> {
    match provider.transcribe(wav) {
        Ok(t) => Ok(t),
        Err(e) if should_retry(&e) => {
            log::warn!("transcription failed ({e}); retrying once");
            provider.transcribe(wav)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Scripted provider: pops one result per call, counts calls.
    struct MockProvider {
        calls: Cell<u32>,
        script: std::cell::RefCell<Vec<Result<Transcript, SttError>>>,
    }

    impl MockProvider {
        fn new(script: Vec<Result<Transcript, SttError>>) -> Self {
            MockProvider {
                calls: Cell::new(0),
                script: std::cell::RefCell::new(script),
            }
        }
        fn ok(text: &str) -> Result<Transcript, SttError> {
            Ok(Transcript {
                text: text.to_string(),
                request_ms: 1,
            })
        }
        fn timeout() -> Result<Transcript, SttError> {
            Err(SttError::Timeout {
                provider: "mock".to_string(),
                configured_ms: 15_000,
            })
        }
        fn auth() -> Result<Transcript, SttError> {
            Err(SttError::Auth {
                provider: "mock".to_string(),
            })
        }
    }

    impl SttProvider for MockProvider {
        fn transcribe(&self, _wav: &[u8]) -> Result<Transcript, SttError> {
            self.calls.set(self.calls.get() + 1);
            self.script.borrow_mut().remove(0)
        }
        fn label(&self) -> &str {
            "mock"
        }
    }

    #[test]
    fn success_needs_one_call() {
        let p = MockProvider::new(vec![MockProvider::ok("hello")]);
        let t = transcribe_with_retry(&p, b"wav").unwrap();
        assert_eq!(t.text, "hello");
        assert_eq!(p.calls.get(), 1);
    }

    #[test]
    fn timeout_retries_exactly_once_then_succeeds() {
        let p = MockProvider::new(vec![
            MockProvider::timeout(),
            MockProvider::ok("second try"),
        ]);
        let t = transcribe_with_retry(&p, b"wav").unwrap();
        assert_eq!(t.text, "second try");
        assert_eq!(p.calls.get(), 2);
    }

    // Transcript (frozen hark-stt) has no Debug impl, so unwrap_err is
    // unavailable; unwrap the error by hand.
    fn expect_err(result: Result<Transcript, SttError>) -> SttError {
        match result {
            Err(e) => e,
            Ok(t) => panic!(
                "expected an error, got a transcript of {} chars",
                t.text.len()
            ),
        }
    }

    #[test]
    fn double_timeout_fails_after_two_calls_total() {
        let p = MockProvider::new(vec![MockProvider::timeout(), MockProvider::timeout()]);
        let err = expect_err(transcribe_with_retry(&p, b"wav"));
        assert!(matches!(err, SttError::Timeout { .. }));
        assert_eq!(p.calls.get(), 2, "never a second retry");
    }

    #[test]
    fn auth_fails_without_any_retry() {
        let p = MockProvider::new(vec![MockProvider::auth()]);
        let err = expect_err(transcribe_with_retry(&p, b"wav"));
        assert!(matches!(err, SttError::Auth { .. }));
        assert_eq!(p.calls.get(), 1, "4xx must never retry");
    }
}
