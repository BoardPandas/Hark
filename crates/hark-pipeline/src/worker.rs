//! The pipeline worker loop: receive chord edges, assemble the window,
//! encode, transcribe (one retry max), inject. Runs on its own thread; all
//! logging is lengths/counts/millis, never key material, audio, or text.

use crate::retry::should_retry;
use crate::state::{advance, Action, Event, PipelineState};
use hark_audio::ring::Consumer;
use hark_audio::WindowParams;
use hark_dictionary::Corrector;
use hark_hotkey::PttEvent;
use hark_inject::InjectSettings;
use hark_stt::{SttError, SttProvider, Transcript};
use hark_voice::{skips_cleanup, CleanupProvider, Voice};
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// The resolved cleanup step: adapter, effective voice, and gate threshold.
/// `None` on the worker means Verbatim (or degraded-to-Verbatim): no cleanup
/// call is ever made.
pub(crate) struct CleanupPlan {
    pub cleaner: Box<dyn CleanupProvider>,
    /// The effective voice, for log lines only.
    pub voice: Voice,
    /// The chat model, for log lines only (config label, not user content).
    pub model: String,
    /// Fewer words than this skip the cleanup call; 0 disables the gate.
    pub skip_below_words: u32,
    /// Set only when the cleanup endpoint differs from the STT one, so the
    /// first cleaned dictation also skips a cold TLS handshake.
    pub prewarm_url: Option<String>,
}

pub(crate) struct Worker {
    pub consumer: Consumer,
    pub sample_rate: u32,
    pub window: WindowParams,
    pub inject: InjectSettings,
    pub provider: Box<dyn SttProvider>,
    /// Dictionary post-correction, built once from the configured terms.
    pub corrector: Corrector,
    /// Optional voice cleanup between dictionary pass 1 and injection.
    pub cleanup: Option<CleanupPlan>,
    /// Base URL to pre-warm (DNS + TCP + TLS) before the first dictation.
    pub prewarm_url: String,
    /// The shared HTTP client (same instance the provider adapter holds),
    /// used only for the pre-warm requests.
    pub client: reqwest::blocking::Client,
}

/// The one long-lived worker loop. Exits when the hotkey listener drops its
/// sender (pipeline shutdown).
pub(crate) fn run(worker: Worker, rx: Receiver<PttEvent>) {
    prewarm(&worker.client, &worker.prewarm_url);
    if let Some(url) = worker
        .cleanup
        .as_ref()
        .and_then(|p| p.prewarm_url.as_deref())
    {
        prewarm(&worker.client, url);
    }

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
fn prewarm(client: &reqwest::blocking::Client, url: &str) {
    let started = Instant::now();
    match client.get(url).send() {
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

    let text = corrected_text(&worker.corrector, &transcript.text);
    let text = cleaned_text(worker.cleanup.as_ref(), &worker.corrector, text);
    match hark_inject::inject(&text, &worker.inject) {
        Ok(()) => {
            log::info!(
                "dictation injected: {} chars, request {} ms, release-to-inject {} ms",
                text.chars().count(),
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

/// The optional voice-cleanup pass between dictionary pass 1 and injection.
/// Fail-open at every layer: no plan (Verbatim or degraded), a gated short
/// utterance, or any cleanup error all inject the pass-1 text unchanged; a
/// dictation is never lost to the optional feature. Dictionary pass 2 runs
/// only when cleanup actually rewrote the text, repairing any term the model
/// re-mangled. Logs counts, millis, and config labels only.
fn cleaned_text(plan: Option<&CleanupPlan>, corrector: &Corrector, text: String) -> String {
    let Some(plan) = plan else {
        return text;
    };
    if skips_cleanup(&text, plan.skip_below_words) {
        log::info!(
            "cleanup skipped (short utterance: {} words)",
            text.split_whitespace().count()
        );
        return text;
    }
    match plan.cleaner.clean(&text) {
        Ok(cleaned) => {
            log::info!(
                "cleanup: voice={} model={} {}->{} chars, request {} ms",
                plan.voice.name(),
                plan.model,
                text.chars().count(),
                cleaned.text.chars().count(),
                cleaned.request_ms
            );
            corrected_text(corrector, &cleaned.text)
        }
        Err(e) => {
            log::warn!("cleanup failed ({e}); injecting uncleaned transcript");
            text
        }
    }
}

/// The dictionary pass between transcript and injection. Pure (the testable
/// seam); logs counts and millis only, never transcript text or terms.
fn corrected_text(corrector: &Corrector, transcript_text: &str) -> String {
    let started = Instant::now();
    let (text, replacements) = corrector.correct(transcript_text);
    log::info!(
        "dictionary: {replacements} replacements in {} ms",
        started.elapsed().as_millis()
    );
    text
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

    // The worker path from a provider transcript to the text handed to
    // hark_inject::inject: transcribe (with retry) then the dictionary
    // pass. Injection itself is I/O, validated on real hardware at CP6.

    #[test]
    fn misspelled_transcript_is_corrected_before_injection() {
        let p = MockProvider::new(vec![MockProvider::ok(
            "tell vosburg the madero build is green",
        )]);
        let corrector = Corrector::new(&["Vossburg".to_string(), "Modero".to_string()]);

        let transcript = transcribe_with_retry(&p, b"wav").unwrap();
        let text = corrected_text(&corrector, &transcript.text);
        assert_eq!(text, "tell Vossburg the Modero build is green");
    }

    #[test]
    fn empty_dictionary_leaves_the_transcript_untouched() {
        let corrector = Corrector::new(&[]);
        assert_eq!(corrected_text(&corrector, "as it was"), "as it was");
    }

    // --- the cleanup pass (scripted MockCleaner, pattern: MockProvider) ---

    use hark_voice::{Cleaned, CleanupError, CleanupProvider};

    /// Scripted cleaner: pops one result per call, counts calls.
    struct MockCleaner {
        calls: Cell<u32>,
        script: std::cell::RefCell<Vec<Result<Cleaned, CleanupError>>>,
    }

    impl MockCleaner {
        fn ok(text: &str) -> Result<Cleaned, CleanupError> {
            Ok(Cleaned {
                text: text.to_string(),
                request_ms: 1,
            })
        }
        fn provider_err() -> Result<Cleaned, CleanupError> {
            Err(CleanupError::Provider {
                provider: "mock".to_string(),
                detail: "scripted failure".to_string(),
            })
        }
        fn plan(script: Vec<Result<Cleaned, CleanupError>>, skip_below_words: u32) -> CleanupPlan {
            CleanupPlan {
                cleaner: Box::new(MockCleaner {
                    calls: Cell::new(0),
                    script: std::cell::RefCell::new(script),
                }),
                voice: hark_voice::Voice::Clean,
                model: "mock-model".to_string(),
                skip_below_words,
                prewarm_url: None,
            }
        }
    }

    impl CleanupProvider for MockCleaner {
        fn clean(&self, _text: &str) -> Result<Cleaned, CleanupError> {
            self.calls.set(self.calls.get() + 1);
            self.script.borrow_mut().remove(0)
        }
        fn label(&self) -> &str {
            "mock"
        }
    }

    #[test]
    fn cleaned_text_is_injected_after_dictionary_pass_2() {
        let corrector = Corrector::new(&[]);
        let plan = MockCleaner::plan(vec![MockCleaner::ok("Cleaned and polished.")], 0);
        let out = cleaned_text(
            Some(&plan),
            &corrector,
            "so um cleaned and uh polished".to_string(),
        );
        assert_eq!(out, "Cleaned and polished.");
    }

    #[test]
    fn cleanup_error_injects_the_pass_1_text() {
        let corrector = Corrector::new(&[]);
        let plan = MockCleaner::plan(vec![MockCleaner::provider_err()], 0);
        let out = cleaned_text(
            Some(&plan),
            &corrector,
            "the original transcript stands".to_string(),
        );
        assert_eq!(out, "the original transcript stands");
    }

    #[test]
    fn no_plan_means_verbatim_and_no_calls() {
        let corrector = Corrector::new(&[]);
        let out = cleaned_text(None, &corrector, "exactly as spoken".to_string());
        assert_eq!(out, "exactly as spoken");
    }

    #[test]
    fn short_utterance_never_calls_the_cleaner() {
        let corrector = Corrector::new(&[]);
        // An empty script would panic on any call; not panicking IS the
        // assertion that the gate short-circuited before the cleaner.
        let plan = MockCleaner::plan(vec![], 5);
        let out = cleaned_text(Some(&plan), &corrector, "um send it".to_string());
        assert_eq!(out, "um send it");
    }

    #[test]
    fn gate_threshold_zero_always_calls() {
        let corrector = Corrector::new(&[]);
        let plan = MockCleaner::plan(vec![MockCleaner::ok("Hi.")], 0);
        // One word, but the gate is disabled: the (scripted) cleaner runs.
        assert_eq!(
            cleaned_text(Some(&plan), &corrector, "hi".to_string()),
            "Hi."
        );
    }

    #[test]
    fn terms_mangled_by_the_model_are_repaired_by_pass_2() {
        let corrector = Corrector::new(&["Vossburg".to_string(), "Modero".to_string()]);
        // The mock "model" re-mangles both dictionary terms.
        let plan = MockCleaner::plan(
            vec![MockCleaner::ok("Tell vosburg the madero build is green.")],
            0,
        );
        let out = cleaned_text(
            Some(&plan),
            &corrector,
            "tell Vossburg the Modero build is green".to_string(),
        );
        assert_eq!(out, "Tell Vossburg the Modero build is green.");
    }
}
