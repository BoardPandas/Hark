//! The pipeline worker loop: receive chord edges, assemble the window,
//! encode, transcribe (one retry max), inject. Runs on its own thread; all
//! logging is lengths/counts/millis, never key material, audio, or text.

use crate::events::{DictationRecord, FailStage, PipelineEvent};
use crate::local::{LocalPlan, Source, Transcriber};
use crate::retry::should_retry;
use crate::state::{advance, Action, Event, PipelineState};
use hark_audio::ring::Consumer;
use hark_audio::WindowParams;
use hark_dictionary::Corrector;
use hark_hotkey::PttEvent;
use hark_inject::InjectSettings;
use hark_stt::{SttError, SttProvider, Transcript};
use hark_voice::{over_expanded, skips_cleanup, CleanupProvider, Voice};
use std::sync::mpsc::{Receiver, Sender};
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
    /// Output longer than this multiple of the input word count is discarded
    /// and the uncleaned transcript injected; 0 disables the check.
    pub max_expansion_ratio: f32,
    /// Set only when the cleanup endpoint differs from the STT one, so the
    /// first cleaned dictation also skips a cold TLS handshake.
    pub prewarm_url: Option<String>,
}

pub(crate) struct Worker {
    pub consumer: Consumer,
    pub sample_rate: u32,
    pub window: WindowParams,
    pub inject: InjectSettings,
    /// The cloud adapter. `None` when `[local_stt] mode = "primary"`: that
    /// mode never contacts a provider and does not even require an API key.
    pub provider: Option<Box<dyn SttProvider>>,
    /// Provider label for the dictation record. Kept separately because
    /// `provider` may be absent while the record still needs a label.
    pub cloud_label: String,
    /// On-device engine plan; `None` when local STT is off or unavailable.
    pub local: Option<LocalPlan>,
    /// Dictionary post-correction, built once from the configured terms.
    pub corrector: Corrector,
    /// Optional voice cleanup between dictionary pass 1 and injection.
    pub cleanup: Option<CleanupPlan>,
    /// Base URL to pre-warm (DNS + TCP + TLS) before the first dictation.
    pub prewarm_url: String,
    /// The shared HTTP client (same instance the provider adapter holds),
    /// used only for the pre-warm requests.
    pub client: reqwest::blocking::Client,
    /// STT model label, for the dictation record (the provider label comes
    /// from the adapter itself).
    pub stt_model: String,
    /// Advisory events toward the UI; every send is `let _ =` best-effort.
    pub events: Sender<PipelineEvent>,
}

/// The one long-lived worker loop. Exits when the hotkey listener drops its
/// sender (pipeline shutdown).
pub(crate) fn run(mut worker: Worker, rx: Receiver<PttEvent>) {
    // Primary-mode never issues a cloud request, so warming a connection to a
    // provider we will not call would just be a pointless network round trip.
    if worker.provider.is_some() {
        prewarm(&worker.client, &worker.prewarm_url);
    }
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
        // Surface the two UI-visible edges: capture started, request in
        // flight. Everything after that is reported from dictate itself.
        if matches!(state, PipelineState::Idle) && matches!(next, PipelineState::Recording { .. }) {
            let _ = worker.events.send(PipelineEvent::Recording);
        }
        state = next;
        if let Action::Dictate { down_abs, up_abs } = action {
            let _ = worker.events.send(PipelineEvent::Processing);
            state = dictate(&mut worker, down_abs, up_abs, state);
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
/// Every exit reports its outcome on the events channel (best-effort).
fn dictate(worker: &mut Worker, down_abs: u64, up_abs: u64, state: PipelineState) -> PipelineState {
    let released = Instant::now();
    // Cloned up front so reporting a failure does not hold a borrow of
    // `worker` across the transcription step, which needs `&mut` for the
    // lazily-loaded on-device engine. `Sender` clones are cheap.
    let events = worker.events.clone();
    let fail = |stage: FailStage, detail: String| {
        let _ = events.send(PipelineEvent::Failed { stage, detail });
    };

    let clip = match hark_audio::assemble_window(
        &worker.consumer,
        worker.sample_rate,
        down_abs,
        up_abs,
        &worker.window,
    ) {
        Ok(hark_audio::Assembled::Clip(clip)) => clip,
        Ok(hark_audio::Assembled::Gated(verdict)) => {
            // Report WHICH gate fired: a tap is the user's own doing and needs
            // no response, while "we heard nothing" may mean their microphone
            // is misconfigured and is worth offering help for.
            let (stage, detail) = match verdict {
                hark_audio::GateVerdict::TooShort => (FailStage::GatedTooShort, "hold too short"),
                // Speech never reaches here; treat it as too-quiet defensively
                // rather than inventing a third outcome.
                _ => (FailStage::GatedTooQuiet, "no speech detected"),
            };
            log::info!("dictation gated ({detail}); no request sent");
            fail(stage, detail.to_string());
            return advance(state, Event::Aborted).0;
        }
        Err(e) => {
            log::error!("window assembly failed: {e}");
            fail(FailStage::Audio, e.to_string());
            return advance(state, Event::Aborted).0;
        }
    };
    let audio_ms = (clip.samples_16k.len() as u64) * 1_000 / 16_000;

    // Only the cloud adapters need a WAV; the on-device engine consumes the
    // f32 samples directly. In primary mode there is no cloud adapter, so
    // encoding one would be pure waste on the hot path.
    let encode_started = Instant::now();
    let wav = match worker.provider {
        Some(_) => hark_stt::wav::encode_wav_16k_mono(&clip.samples_16k),
        None => Vec::new(),
    };
    log::debug!(
        "clip: {} samples at 16 kHz ({} bytes WAV, encoded in {} ms)",
        clip.samples_16k.len(),
        wav.len(),
        encode_started.elapsed().as_millis()
    );

    let mode = worker
        .local
        .as_ref()
        .map_or(hark_config::LocalMode::Off, |p| p.mode);
    let local_ready = worker.local.as_ref().is_some_and(|p| p.is_ready());
    let mut engines = Engines {
        // Disjoint field borrows: `provider` shared, `local` mutable.
        provider: worker.provider.as_deref(),
        wav: &wav,
        local: worker.local.as_mut(),
        samples: &clip.samples_16k,
        events: &events,
    };
    let outcome = match crate::local::transcribe(mode, local_ready, &mut engines) {
        Ok(o) => o,
        Err(e) => {
            log::error!("transcription failed: {e}");
            fail(FailStage::Transcribe, e.to_string());
            return advance(state, Event::Aborted).0;
        }
    };
    let source = outcome.source;
    let transcript = outcome.transcript;
    let state = advance(state, Event::TranscriptReady).0;

    if transcript.text.trim().is_empty() {
        log::info!("provider returned an empty transcript; nothing to inject");
        fail(
            FailStage::EmptyTranscript,
            "the provider returned an empty transcript".to_string(),
        );
        return advance(state, Event::Aborted).0;
    }

    let text = corrected_text(&worker.corrector, &transcript.text);
    let cleaned = cleaned_text(worker.cleanup.as_ref(), &worker.corrector, text);
    match hark_inject::inject(&cleaned.text, &worker.inject) {
        Ok(()) => {
            let total_ms = released.elapsed().as_millis() as u64;
            log::info!(
                "dictation injected: {} chars, {} {} ms, release-to-inject {total_ms} ms",
                cleaned.text.chars().count(),
                match source {
                    Source::Cloud => "request",
                    _ => "on-device decode",
                },
                transcript.request_ms,
            );
            // The labels state what actually shaped the text: a skipped,
            // gated, or failed cleanup call means the entry is verbatim, so
            // a disappointing result never blames the wrong model.
            let cleanup_ran = cleaned.request_ms.is_some();
            let (voice, cleanup_model) = match worker.cleanup.as_ref().filter(|_| cleanup_ran) {
                Some(plan) => (plan.voice.name().to_string(), Some(plan.model.clone())),
                None => (Voice::Verbatim.name().to_string(), None),
            };
            // Name the engine that actually produced this line. A fallback
            // dictation credited to the cloud provider would make history
            // lie about why a transcript reads the way it does.
            let stt_model = match source {
                Source::Cloud => worker.stt_model.clone(),
                _ => worker
                    .local
                    .as_ref()
                    .map_or_else(String::new, |p| p.spec.id.to_string()),
            };
            let _ = events.send(PipelineEvent::Injected(DictationRecord {
                raw_text: transcript.text,
                final_text: cleaned.text,
                voice,
                stt_provider: source.label(&worker.cloud_label),
                stt_model,
                cleanup_model,
                audio_ms,
                stt_ms: transcript.request_ms as u64,
                cleanup_ms: cleaned.request_ms,
                total_ms,
            }));
            advance(state, Event::Injected).0
        }
        Err(e) => {
            log::error!("injection failed: {e}");
            fail(FailStage::Inject, e.to_string());
            advance(state, Event::Aborted).0
        }
    }
}

/// Outcome of the cleanup pass. `request_ms` is present only when a cleanup
/// response actually shaped `text`; every degraded path (no plan, gate,
/// provider error) leaves it `None` so the dictation record stays honest.
pub(crate) struct CleanupOutcome {
    pub text: String,
    pub request_ms: Option<u64>,
}

/// The optional voice-cleanup pass between dictionary pass 1 and injection.
/// Fail-open at every layer: no plan (Verbatim or degraded), a gated short
/// utterance, or any cleanup error all inject the pass-1 text unchanged; a
/// dictation is never lost to the optional feature. Dictionary pass 2 runs
/// only when cleanup actually rewrote the text, repairing any term the model
/// re-mangled. Logs counts, millis, and config labels only.
fn cleaned_text(plan: Option<&CleanupPlan>, corrector: &Corrector, text: String) -> CleanupOutcome {
    let passthrough = |text: String| CleanupOutcome {
        text,
        request_ms: None,
    };
    let Some(plan) = plan else {
        return passthrough(text);
    };
    if skips_cleanup(&text, plan.skip_below_words) {
        log::info!(
            "cleanup skipped (short utterance: {} words)",
            text.split_whitespace().count()
        );
        return passthrough(text);
    }
    match plan.cleaner.clean(&text) {
        // Custom is the user's own prompt, so an expansion there was asked
        // for; every built-in voice is an edit and must not grow the text.
        Ok(cleaned)
            if plan.voice != Voice::Custom
                && over_expanded(&text, &cleaned.text, plan.max_expansion_ratio) =>
        {
            log::warn!(
                "cleanup rejected: voice={} model={} expanded {} -> {} words (limit {}x); \
                 injecting uncleaned transcript",
                plan.voice.name(),
                plan.model,
                text.split_whitespace().count(),
                cleaned.text.split_whitespace().count(),
                plan.max_expansion_ratio
            );
            passthrough(text)
        }
        Ok(cleaned) => {
            log::info!(
                "cleanup: voice={} model={} {}->{} chars, request {} ms",
                plan.voice.name(),
                plan.model,
                text.chars().count(),
                cleaned.text.chars().count(),
                cleaned.request_ms
            );
            CleanupOutcome {
                text: corrected_text(corrector, &cleaned.text),
                request_ms: Some(cleaned.request_ms as u64),
            }
        }
        Err(e) => {
            log::warn!("cleanup failed ({e}); injecting uncleaned transcript");
            passthrough(text)
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

/// Binds one dictation's audio to whichever engines this worker has, so the
/// cloud/local policy in [`crate::local`] can stay pure and testable.
struct Engines<'a> {
    provider: Option<&'a dyn SttProvider>,
    /// Empty in primary mode, where no WAV is ever encoded.
    wav: &'a [u8],
    local: Option<&'a mut LocalPlan>,
    samples: &'a [f32],
    events: &'a Sender<PipelineEvent>,
}

impl Transcriber for Engines<'_> {
    fn cloud(&mut self) -> Option<Result<Transcript, SttError>> {
        let provider = self.provider?;
        Some(transcribe_with_retry(provider, self.wav))
    }

    fn local(&mut self) -> Result<Transcript, hark_local_stt::LocalSttError> {
        let plan = self
            .local
            .as_mut()
            .ok_or(hark_local_stt::LocalSttError::EngineUnavailable)?;
        // First use reads ~670 MB of weights into RAM, which takes seconds.
        // Say so before blocking, or the app just looks hung.
        if !plan.is_loaded() {
            let _ = self.events.send(PipelineEvent::LoadingLocalModel);
        }
        let decoded = plan.engine()?.transcribe(self.samples)?;
        Ok(Transcript {
            text: decoded.text,
            // The local engine transcribes only; cleanup stays a separate step.
            cleaned: None,
            request_ms: decoded.request_ms,
        })
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
                cleaned: None,
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
                max_expansion_ratio: 1.4,
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
        assert_eq!(out.text, "Cleaned and polished.");
        // A successful cleanup reports its wall time for the record.
        assert!(out.request_ms.is_some());
    }

    #[test]
    fn an_over_expanded_cleanup_is_discarded_for_the_pass_1_text() {
        let corrector = Corrector::new(&[]);
        // Five words in, a paragraph out: the reported Professional/Clean
        // failure. The call still happened, so the cost was paid, but the
        // user gets what they said rather than what the model wrote.
        let plan = MockCleaner::plan(
            vec![MockCleaner::ok(
                "I wanted to follow up regarding our release timeline. After giving it some \
                 thought, I believe we should aim to ship this coming Friday. Please let me \
                 know if that works for you.",
            )],
            0,
        );
        let out = cleaned_text(
            Some(&plan),
            &corrector,
            "we should ship it friday".to_string(),
        );
        assert_eq!(out.text, "we should ship it friday");
        // Rejected cleanup must not claim a model shaped the record.
        assert_eq!(out.request_ms, None);
    }

    #[test]
    fn a_same_length_cleanup_is_kept() {
        let corrector = Corrector::new(&[]);
        let plan = MockCleaner::plan(vec![MockCleaner::ok("We should ship it Friday.")], 0);
        let out = cleaned_text(
            Some(&plan),
            &corrector,
            "so um we should ship it friday".to_string(),
        );
        assert_eq!(out.text, "We should ship it Friday.");
        assert!(out.request_ms.is_some());
    }

    #[test]
    fn custom_voice_may_expand_freely() {
        let corrector = Corrector::new(&[]);
        let long = "Dear team, I wanted to write and let you know that we should aim to ship \
                    this coming Friday. Please let me know if that works. Best regards.";
        let mut plan = MockCleaner::plan(vec![MockCleaner::ok(long)], 0);
        plan.voice = hark_voice::Voice::Custom;
        let out = cleaned_text(
            Some(&plan),
            &corrector,
            "we should ship it friday".to_string(),
        );
        // The user's own prompt asked for this; the guard must not fight it.
        assert_eq!(out.text, long);
        assert!(out.request_ms.is_some());
    }

    #[test]
    fn ratio_of_zero_disables_the_expansion_guard() {
        let corrector = Corrector::new(&[]);
        let long = vec!["word"; 200].join(" ");
        let mut plan = MockCleaner::plan(vec![MockCleaner::ok(&long)], 0);
        plan.max_expansion_ratio = 0.0;
        let out = cleaned_text(
            Some(&plan),
            &corrector,
            "we should ship it friday".to_string(),
        );
        assert_eq!(out.text, long);
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
        assert_eq!(out.text, "the original transcript stands");
        // Degraded: the record must not claim a cleanup model shaped this.
        assert_eq!(out.request_ms, None);
    }

    #[test]
    fn no_plan_means_verbatim_and_no_calls() {
        let corrector = Corrector::new(&[]);
        let out = cleaned_text(None, &corrector, "exactly as spoken".to_string());
        assert_eq!(out.text, "exactly as spoken");
        assert_eq!(out.request_ms, None);
    }

    #[test]
    fn short_utterance_never_calls_the_cleaner() {
        let corrector = Corrector::new(&[]);
        // An empty script would panic on any call; not panicking IS the
        // assertion that the gate short-circuited before the cleaner.
        let plan = MockCleaner::plan(vec![], 5);
        let out = cleaned_text(Some(&plan), &corrector, "um send it".to_string());
        assert_eq!(out.text, "um send it");
        assert_eq!(out.request_ms, None);
    }

    #[test]
    fn gate_threshold_zero_always_calls() {
        let corrector = Corrector::new(&[]);
        let plan = MockCleaner::plan(vec![MockCleaner::ok("Hi.")], 0);
        // One word, but the gate is disabled: the (scripted) cleaner runs.
        assert_eq!(
            cleaned_text(Some(&plan), &corrector, "hi".to_string()).text,
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
        assert_eq!(out.text, "Tell Vossburg the Modero build is green.");
    }
}
