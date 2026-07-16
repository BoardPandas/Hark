//! The cleanup-provider section (spec §3.6): inherited display by default,
//! explicit override expandable, and its own key section when the keychain
//! account differs from the STT provider's.

use crate::theme;
use crate::ui::settings::form::{inline_error, none_if_empty, FormBufs};
use crate::ui::settings::keys::KeySection;
use egui::{CollapsingHeader, RichText, TextEdit, Ui};
use hark_config::{ProviderKind, Settings, VoiceProvider};

pub fn section(
    ui: &mut Ui,
    draft: &mut Settings,
    bufs: &mut FormBufs,
    cleanup_keys: &mut Option<KeySection>,
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
        });
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
