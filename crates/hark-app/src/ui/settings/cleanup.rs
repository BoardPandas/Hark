//! The cleanup-provider section (spec §3.6): inherited display by default,
//! explicit override expandable, and its own key section when the keychain
//! account differs from the STT provider's.

use crate::theme;
use crate::ui::settings::form::{inline_error, none_if_empty, FormBufs};
use crate::ui::settings::keys::KeySection;
use crate::ui::settings::test::{cleanup_test, CleanupPass};
use egui::{CollapsingHeader, RichText, TextEdit, Ui};
use hark_config::{CleanupResolution, ProviderKind, Settings, VoiceName, VoiceProvider};
use std::sync::mpsc::{self, Receiver};

pub fn section(
    ui: &mut Ui,
    draft: &mut Settings,
    bufs: &mut FormBufs,
    cleanup_keys: &mut Option<KeySection>,
    test: &mut CleanupTest,
) {
    CollapsingHeader::new("Cleanup provider")
        .default_open(false)
        .show(ui, |ui| {
            let mut override_on = draft.voice.provider.is_some();
            if ui
                .checkbox(&mut override_on, "Use a different provider for cleanup")
                .changed()
            {
                draft.voice.provider = if override_on {
                    Some(VoiceProvider {
                        kind: ProviderKind::Openai,
                        base_url: None,
                        model: None,
                        temperature: None,
                        reasoning_effort: None,
                        key_account: None,
                    })
                } else {
                    None
                };
                bufs.cleanup_model.clear();
                bufs.cleanup_base_url.clear();
                bufs.cleanup_key_account.clear();
            }

            let stt_account = draft.provider.kind.label().to_string();
            if let Some(p) = &mut draft.voice.provider {
                ui.horizontal_wrapped(|ui| {
                    for (kind, label) in [
                        (ProviderKind::Openai, "OpenAI"),
                        (ProviderKind::Groq, "Groq"),
                        (ProviderKind::OpenaiCompatible, "OpenAI-compatible"),
                    ] {
                        // Deepgram is absent by design: no chat product.
                        ui.radio_value(&mut p.kind, kind, label);
                    }
                });

                ui.label(RichText::new("Chat model").weak());
                let default_model = VoiceProvider {
                    kind: p.kind,
                    base_url: None,
                    model: None,
                    temperature: None,
                    reasoning_effort: None,
                    key_account: None,
                }
                .resolved_model();
                ui.add(
                    TextEdit::singleline(&mut bufs.cleanup_model)
                        .hint_text(format!("{default_model} (default)")),
                );
                p.model = none_if_empty(&bufs.cleanup_model);

                ui.label(RichText::new("Base URL").weak());
                ui.add(TextEdit::singleline(&mut bufs.cleanup_base_url).hint_text(
                    if p.kind == ProviderKind::OpenaiCompatible {
                        "https://host/v1 (required)"
                    } else {
                        "provider default"
                    },
                ));
                p.base_url = none_if_empty(&bufs.cleanup_base_url);
                if p.kind == ProviderKind::OpenaiCompatible && p.base_url.is_none() {
                    inline_error(ui, "OpenAI-compatible endpoints need a base URL.");
                }

                ui.label(RichText::new("Keychain account").weak());
                ui.add(
                    TextEdit::singleline(&mut bufs.cleanup_key_account)
                        .hint_text(format!("{} (default)", p.kind.label())),
                );
                p.key_account = none_if_empty(&bufs.cleanup_key_account);

                let account = p
                    .key_account
                    .clone()
                    .unwrap_or_else(|| p.kind.label().to_string());
                if account == stt_account {
                    ui.label(
                        RichText::new(format!(
                            "Uses the \"{account}\" key already stored for speech to text."
                        ))
                        .small()
                        .weak(),
                    );
                    *cleanup_keys = None;
                } else {
                    let section =
                        cleanup_keys.get_or_insert_with(|| KeySection::new("cleanup", &account));
                    section.sync_account(&account);
                    section.show(ui);
                }
            } else {
                *cleanup_keys = None;
                inherited_line(ui, draft);
            }

            ui.add_space(6.0);
            test.show(ui, draft);
        });
}

/// The cleanup section's own test button: one tiny chat call against
/// whatever this section is currently configured to use.
///
/// Separate from the provider-level "Test connection" (which only probes
/// cleanup when the *default voice* would run one). A user with an explicit
/// endpoint and a Verbatim default has no other way to find out whether the
/// key, base URL, and model actually work.
pub struct CleanupTest {
    state: State,
}

enum State {
    Idle,
    Running(Receiver<Outcome>),
    Done(Outcome),
}

struct Outcome {
    /// The voice the probe ran under; worth naming when it is not the
    /// user's default (see `probe_voice`).
    voice: VoiceName,
    result: Result<CleanupPass, String>,
}

impl CleanupTest {
    pub fn new() -> Self {
        CleanupTest { state: State::Idle }
    }

    fn show(&mut self, ui: &mut Ui, draft: &Settings) {
        if let State::Running(rx) = &self.state {
            if let Ok(outcome) = rx.try_recv() {
                self.state = State::Done(outcome);
            }
        }

        // Resolution is pure and cheap, so the button can say up front why
        // it would do nothing instead of running and reporting silence.
        let voice = probe_voice(draft);
        let resolvable = matches!(
            hark_config::resolve_cleanup_provider(&draft.provider, &draft.voice, voice),
            CleanupResolution::Resolved(_)
        );

        let running = matches!(self.state, State::Running(_));
        ui.horizontal(|ui| {
            let enabled = resolvable && !running;
            let button = ui.add_enabled(enabled, egui::Button::new("Test cleanup"));
            if button.clicked() {
                self.state = State::Running(spawn(draft.clone(), voice, ui.ctx().clone()));
            }
            if !resolvable {
                button.on_hover_text("No cleanup provider resolves from these settings.");
            }
            if running {
                ui.add(egui::Spinner::new().size(16.0));
                ui.label(RichText::new("Sending one short cleanup request").small());
            }
        });

        if let State::Done(outcome) = &self.state {
            report(ui, outcome, draft.voice.default);
        }
    }
}

/// Which voice the probe runs under. A Verbatim default makes no cleanup
/// call, so probing it would only ever report "nothing happened" — Clean
/// exercises the same endpoint, key, and model the user is configuring.
fn probe_voice(draft: &Settings) -> VoiceName {
    match draft.voice.default {
        VoiceName::Verbatim => VoiceName::Clean,
        other => other,
    }
}

fn report(ui: &mut Ui, outcome: &Outcome, default_voice: VoiceName) {
    match &outcome.result {
        Ok(pass) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::CHECK).color(theme::SUCCESS));
                ui.label(format!("Cleanup reached {}", pass.model));
                ui.label(RichText::new(format!("{} ms", pass.ms)).monospace().weak());
            });
        }
        Err(detail) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::X).color(theme::DANGER));
                ui.label(detail);
            });
        }
    }
    if default_voice == VoiceName::Verbatim {
        ui.label(
            RichText::new(format!(
                "Tested with the {} voice; your default (verbatim) makes no cleanup call.",
                outcome.voice.label()
            ))
            .small()
            .weak(),
        );
    }
}

/// Run the probe on a named background thread; the UI never blocks a frame.
/// The STT key is resolved here (not on the UI thread) because the inherit
/// path reuses it and keychain reads are OS I/O.
fn spawn(settings: Settings, voice: VoiceName, ctx: egui::Context) -> Receiver<Outcome> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hark-test-cleanup".to_string())
        .spawn(move || {
            let stt_key = hark_keychain::resolve_key(settings.provider.kind.label()).ok();
            let result = cleanup_test(&settings, stt_key.as_deref(), voice).unwrap_or_else(|| {
                Err("No cleanup provider resolves from these settings.".to_string())
            });
            let _ = tx.send(Outcome { voice, result });
            ctx.request_repaint();
        })
        .expect("spawning the cleanup-test thread cannot fail");
    rx
}

/// What actually runs when no override is set: honest about degradation.
fn inherited_line(ui: &mut Ui, draft: &Settings) {
    use hark_config::CleanupResolution;
    match hark_config::resolve_cleanup_provider(&draft.provider, &draft.voice, draft.voice.default)
    {
        CleanupResolution::Verbatim => {
            ui.label(
                RichText::new("Verbatim voice: no cleanup call runs.")
                    .small()
                    .weak(),
            );
        }
        CleanupResolution::VerbatimWithWarning { reason } => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::WARNING).color(theme::WARNING));
                ui.label(RichText::new(format!(
                    "{reason} Text is injected as transcribed."
                )));
            });
        }
        CleanupResolution::Resolved(r) => {
            ui.label(
                RichText::new(format!(
                    "Inherited from STT ({}) · {}",
                    draft.provider.kind.label(),
                    r.model
                ))
                .small()
                .weak(),
            );
        }
    }
}
