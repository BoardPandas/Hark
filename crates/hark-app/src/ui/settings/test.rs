//! Test connection (spec §3.6): transcribe the bundled fixture clip through
//! the configured provider on a background thread, plus one tiny chat call
//! when a cleanup provider would run. Results render inline under the
//! button and persist until the next test (never a vanishing toast).
//!
//! Error strings come verbatim from hark-stt / hark-voice / hark-keychain:
//! they already name the cause (rejected key vs timeout vs unknown model)
//! and never carry key material. No transport-error classification happens
//! here (LL-G: reqwest multipart masks transport errors).

use crate::theme;
use egui::{RichText, Ui};
use hark_config::{ProviderKind, Settings};
use std::sync::mpsc::{self, Receiver};

pub struct SttPass {
    pub text: String,
    pub ms: u128,
}

pub struct CleanupPass {
    pub model: String,
    pub ms: u128,
}

pub struct TestReport {
    pub provider: String,
    pub model: String,
    pub stt: Result<SttPass, String>,
    /// `None` when no cleanup call would run for the draft settings.
    pub cleanup: Option<Result<CleanupPass, String>>,
}

enum State {
    Idle,
    Running(Receiver<TestReport>),
    Done(TestReport),
}

pub struct TestConnection {
    state: State,
}

impl TestConnection {
    pub fn new() -> Self {
        TestConnection { state: State::Idle }
    }

    pub fn stt_passed(&self) -> bool {
        matches!(&self.state, State::Done(r) if r.stt.is_ok())
    }

    /// Poll + render. Returns true on the frame a test completes.
    pub fn show(&mut self, ui: &mut Ui, draft: &Settings) -> bool {
        let mut finished = false;
        if let State::Running(rx) = &self.state {
            if let Ok(report) = rx.try_recv() {
                self.state = State::Done(report);
                finished = true;
            }
        }

        let running = matches!(self.state, State::Running(_));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!running, egui::Button::new("Test connection"))
                .clicked()
            {
                self.state = State::Running(spawn(draft.clone(), ui.ctx().clone()));
            }
            if running {
                ui.add(egui::Spinner::new().size(16.0));
                ui.label(RichText::new("Testing against the provider").small());
            }
        });
        if draft.provider.kind == ProviderKind::Groq {
            ui.label(
                RichText::new(
                    "Groq bills a 10 second minimum per request; each test (like each \
                     short dictation) is billed as 10 s.",
                )
                .small()
                .weak(),
            );
        }

        if let State::Done(report) = &self.state {
            show_report(ui, report);
        }
        finished
    }
}

fn show_report(ui: &mut Ui, report: &TestReport) {
    match &report.stt {
        Ok(pass) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(theme::icons::CHECK).color(theme::SUCCESS));
                ui.add(
                    egui::Label::new(RichText::new(format!("\u{201C}{}\u{201D}", pass.text)))
                        .truncate(),
                );
            });
            ui.label(
                RichText::new(format!(
                    "{} ms · {} · {}",
                    pass.ms, report.provider, report.model
                ))
                .monospace()
                .weak(),
            );
        }
        Err(detail) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::X).color(theme::DANGER));
                ui.label(detail);
            });
        }
    }
    match &report.cleanup {
        Some(Ok(pass)) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(theme::icons::CHECK).color(theme::SUCCESS));
                ui.label(format!("Cleanup {}", pass.model));
                ui.label(RichText::new(format!("{} ms", pass.ms)).monospace().weak());
            });
        }
        Some(Err(detail)) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::X).color(theme::DANGER));
                ui.label(format!("Cleanup: {detail}"));
            });
        }
        None => {}
    }
}

/// Run the test on a named background thread; the UI never blocks a frame.
fn spawn(settings: Settings, ctx: egui::Context) -> Receiver<TestReport> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hark-test-connection".to_string())
        .spawn(move || {
            let report = execute(&settings);
            let _ = tx.send(report);
            ctx.request_repaint();
        })
        .expect("spawning the test-connection thread cannot fail");
    rx
}

fn execute(settings: &Settings) -> TestReport {
    let provider = settings.provider.kind.label().to_string();
    let model = settings.provider.resolved_model();

    let key = hark_keychain::resolve_key(&provider);
    let stt_key = key.as_ref().ok().cloned();
    let stt = key
        .map_err(|e| e.to_string())
        .and_then(|k| stt_test(settings, k));
    let cleanup = cleanup_test(settings, stt_key.as_deref(), settings.voice.default);

    TestReport {
        provider,
        model,
        stt,
        cleanup,
    }
}

/// The exact path the pipeline runs: same config mapping, same adapter.
fn stt_test(settings: &Settings, api_key: String) -> Result<SttPass, String> {
    let config = hark_pipeline::provider_config(settings, api_key).map_err(|e| e.to_string())?;
    let client = hark_stt::shared_client().map_err(|e| e.to_string())?;
    let adapter = hark_stt::build(&config, client).map_err(|e| e.to_string())?;
    let transcript = adapter
        .transcribe(hark_stt::fixture::SPIKE_WAV)
        .map_err(|e| e.to_string())?;
    Ok(SttPass {
        text: transcript.text,
        ms: transcript.request_ms,
    })
}

/// One tiny chat call when (and only when) `voice` would run cleanup.
/// Unlike the pipeline's fail-open build, a test failure is reported, not
/// silently degraded to verbatim.
///
/// `voice` is a parameter rather than `settings.voice.default` so the
/// cleanup section's own test button can probe a configured endpoint even
/// while the saved default voice is Verbatim (which runs no cleanup at all).
pub(super) fn cleanup_test(
    settings: &Settings,
    stt_key: Option<&str>,
    voice: hark_config::VoiceName,
) -> Option<Result<CleanupPass, String>> {
    use hark_config::{CleanupKeySource, CleanupResolution};

    let resolved =
        match hark_config::resolve_cleanup_provider(&settings.provider, &settings.voice, voice) {
            CleanupResolution::Resolved(r) => r,
            CleanupResolution::Verbatim | CleanupResolution::VerbatimWithWarning { .. } => {
                return None
            }
        };

    let run = || -> Result<CleanupPass, String> {
        let api_key = match &resolved.key_source {
            CleanupKeySource::ReuseSttKey => stt_key
                .ok_or_else(|| "no STT key available to reuse for cleanup".to_string())?
                .to_string(),
            CleanupKeySource::Account(account) => {
                hark_keychain::resolve_key_for(hark_keychain::CLEANUP_ENV_OVERRIDE, account)
                    .map_err(|e| e.to_string())?
            }
        };
        let voice: hark_voice::Voice = voice
            .label()
            .parse()
            .map_err(|e: hark_voice::UnknownVoice| e.to_string())?;
        let config = hark_voice::openai_compatible::CleanupConfig {
            label: resolved.kind.label().to_string(),
            base_url: resolved.base_url.clone(),
            model: resolved.model.clone(),
            api_key,
            temperature: resolved.temperature,
            reasoning_effort: resolved.reasoning_effort.clone(),
            voice,
            custom_prompt: settings.voice.custom_prompt.clone(),
            dictionary_terms: settings.dictionary.terms.clone(),
        };
        let client = hark_stt::shared_client().map_err(|e| e.to_string())?;
        use hark_voice::CleanupProvider as _;
        let chat = hark_voice::openai_compatible::OpenAiCompatibleChat::new(&config, client)
            .map_err(|e| e.to_string())?;
        let cleaned = chat
            .clean("hold the key speak and release")
            .map_err(|e| e.to_string())?;
        Ok(CleanupPass {
            model: resolved.model.clone(),
            ms: cleaned.request_ms,
        })
    };
    Some(run())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Only the resolution gate is testable without network or keychain:
    // both no-cleanup outcomes must return None before any key lookup.

    #[test]
    fn verbatim_voice_runs_no_cleanup_test() {
        let settings =
            Settings::from_toml("[provider]\nkind = \"openai\"\n[voice]\ndefault = \"verbatim\"")
                .unwrap();
        assert!(cleanup_test(&settings, Some("k"), settings.voice.default).is_none());
    }

    #[test]
    fn deepgram_without_override_runs_no_cleanup_test() {
        // Deepgram has no chat product; resolution degrades with a warning
        // and the test section stays honest by showing nothing.
        let settings = Settings::default();
        assert!(cleanup_test(&settings, Some("k"), settings.voice.default).is_none());
    }
}
