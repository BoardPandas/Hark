//! The Speech to text card: the cloud provider first, the on-device toggle
//! below it, and the tips that connect the two (spec §2).
//!
//! The decisions are pure functions so `--no-default-features` tests reach
//! both branches: there `LocalEngine::is_available()` is always false, so
//! availability is passed in rather than read here.

use super::{form, local, provider_help, SettingsPage};
use crate::{pipeline::PipelineController, theme};
use egui::{RichText, Ui};
use hark_config::{
    resolve_cleanup_provider, CleanupKeySource, CleanupResolution, LocalMode, Settings, VoiceName,
};
use hark_keychain::KeyStatus;
use hark_local_stt::format_bytes;

impl SettingsPage {
    /// Cloud first because it stays the default; in Primary the cloud block
    /// hides and the toggle leads (spec §2.1).
    pub(super) fn speech_to_text_card(
        &mut self,
        ui: &mut Ui,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
    ) {
        theme::card(ui, |ui| {
            let primary = cloud_hidden(
                hark_local_stt::LocalEngine::is_available(),
                self.draft.local_stt.mode,
            );
            if primary {
                form::subhead(ui, "Speech to text");
            } else {
                self.cloud_block(ui, saved, pipeline);
                ui.add_space(theme::GAP);
                theme::fading_rule(ui, theme::GAP);
            }
            local::on_device_block(
                ui,
                &mut self.draft,
                &mut self.download,
                self.stt_keys.stored(),
            );
            if primary {
                ui.add_space(theme::GAP);
                ui.label(
                    RichText::new(
                        "Cloud speech-to-text is off. Choose Backup to use a cloud provider first.",
                    )
                    .small()
                    .weak(),
                );
            }
        });
    }

    /// R3. Hidden completely in Primary, including Test connection, which
    /// only tests the cloud.
    fn cloud_block(&mut self, ui: &mut Ui, saved: &Settings, pipeline: &mut PipelineController) {
        if form::provider_section(ui, &mut self.draft, &mut self.bufs) {
            self.stt_keys.sync_account(self.draft.provider.kind.label());
            self.test.reset();
        }
        let help = provider_help::help(self.draft.provider.kind);
        ui.label(
            RichText::new(provider_help::describe(self.draft.provider.kind))
                .small()
                .weak(),
        );
        ui.add_space(theme::GAP);
        let link = help.key_page.as_ref().map(|page| (page.text, page.url));
        if self.stt_keys.show_with_link(ui, link) {
            self.test.reset();
            if self.draft.provider.kind == saved.provider.kind {
                pipeline.start(saved, ui.ctx());
            }
        }
        if callout_visible(
            hark_local_stt::LocalEngine::is_available(),
            self.stt_keys.status(),
            self.draft.local_stt.mode,
        ) {
            ui.add_space(theme::GAP);
            let size = format_bytes(self.download.spec().total_bytes());
            if no_key_callout(ui, &size) {
                use_on_device(&mut self.draft);
                self.download.refresh_status();
            }
        }
        ui.add_space(theme::GAP);
        self.test.show(ui, &self.draft);
        ui.add_space(theme::GAP);
        form::model_endpoint_section(ui, &mut self.draft, &mut self.bufs);
    }
}

/// Shown as hover text on Primary; the visible line cannot carry all of it.
pub(super) const PRIMARY_HOVER: &str =
    "Meeting final passes and notes have their own settings and can still use cloud providers.";

/// Turning the toggle on keeps a working cloud setup in front (Backup) and
/// only goes on-device-first when there is no key to use (R7).
pub(super) fn mode_when_enabled(key_stored: bool) -> LocalMode {
    if key_stored {
        LocalMode::Fallback
    } else {
        LocalMode::Primary
    }
}

/// Primary hides the cloud block (R3), but only while the on-device block
/// can be drawn: a `primary` config in a build without the engine would
/// otherwise leave the card with no control to change it.
pub(super) fn cloud_hidden(engine_available: bool, mode: LocalMode) -> bool {
    engine_available && mode == LocalMode::Primary
}

/// The no-key callout (R9). `Backend` counts as not stored: on-device needs
/// no keychain, so it is the one path a keychain failure cannot block.
pub(super) fn callout_visible(engine_available: bool, key: &KeyStatus, mode: LocalMode) -> bool {
    engine_available && !matches!(key, KeyStatus::Stored) && mode != LocalMode::Primary
}

/// The no-key callout itself. Returns true on click; the caller applies
/// [`use_on_device`]. A normal button, not the primary one, so on-device is
/// offered without being promoted over the cloud default (spec A2).
pub(super) fn no_key_callout(ui: &mut Ui, size: &str) -> bool {
    theme::tip(ui, theme::icons::WAVEFORM, |ui| {
        ui.label("No paid AI key? Use on-device.");
        ui.label(
            RichText::new(format!(
                "Free and offline after a one-time {size} download."
            ))
            .small()
            .weak(),
        );
        ui.button("Use on-device instead").clicked()
    })
    .inner
}

/// The callout's button. A voice that would reuse the (missing) STT key for
/// cleanup has nothing to call, so it drops to Verbatim as first run does; a
/// voice with its own cleanup account keeps working and is left alone. The
/// download is never started from here (a 670 MB transfer has its own button).
pub(super) fn use_on_device(draft: &mut Settings) {
    draft.local_stt.mode = LocalMode::Primary;
    if let CleanupResolution::Resolved(resolved) =
        resolve_cleanup_provider(&draft.provider, &draft.voice, draft.voice.default)
    {
        if matches!(resolved.key_source, CleanupKeySource::ReuseSttKey) {
            draft.voice.default = VoiceName::Verbatim;
        }
    }
}

/// The footer under the Dictation section (R16, spec §2.4).
pub(super) fn privacy_line(mode: LocalMode) -> &'static str {
    match mode {
        LocalMode::Off => {
            "Cloud transcription sends audio to your chosen provider. \
             Non-Verbatim voices may also send text to your cleanup provider."
        }
        LocalMode::Fallback => {
            "Audio goes to your cloud provider first; the on-device backup runs on this computer. \
             Non-Verbatim voices may also send text to your cleanup provider."
        }
        LocalMode::Primary => {
            "On-device transcription keeps audio on this computer. \
             Non-Verbatim voices may still send text to your cleanup provider."
        }
    }
}

/// The visible line under the selected mode radio (R6). The timeout comes
/// from config so the copy cannot drift from what the pipeline does.
pub(super) fn mode_line(mode: LocalMode, fallback_after_ms: u64) -> Option<String> {
    match mode {
        LocalMode::Off => None,
        LocalMode::Fallback => Some(format!(
            "Audio goes to your cloud provider first. If it fails or takes longer than {} s, \
             Hark transcribes the clip on this computer.",
            seconds(fallback_after_ms)
        )),
        LocalMode::Primary => {
            Some("Audio stays on this computer. Live meeting transcripts run here too.".into())
        }
    }
}

fn seconds(ms: u64) -> String {
    if ms.is_multiple_of(1000) {
        (ms / 1000).to_string()
    } else {
        format!("{:.1}", ms as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_config::{ProviderKind, VoiceProvider};

    const MODES: [LocalMode; 3] = [LocalMode::Off, LocalMode::Fallback, LocalMode::Primary];

    #[test]
    fn enabling_backs_up_a_stored_key_and_otherwise_goes_on_device() {
        assert_eq!(mode_when_enabled(true), LocalMode::Fallback);
        assert_eq!(mode_when_enabled(false), LocalMode::Primary);
    }

    #[test]
    fn primary_hides_the_cloud_only_where_the_toggle_can_undo_it() {
        assert!(cloud_hidden(true, LocalMode::Primary));
        assert!(!cloud_hidden(false, LocalMode::Primary));
        assert!(!cloud_hidden(true, LocalMode::Fallback));
        assert!(!cloud_hidden(true, LocalMode::Off));
    }

    #[test]
    fn the_callout_shows_only_for_an_unusable_key_outside_primary() {
        let keys = [
            KeyStatus::Missing,
            KeyStatus::Stored,
            KeyStatus::Backend("locked".into()),
        ];
        for available in [true, false] {
            for key in &keys {
                for mode in MODES {
                    let expected = available
                        && !matches!(key, KeyStatus::Stored)
                        && mode != LocalMode::Primary;
                    assert_eq!(
                        callout_visible(available, key, mode),
                        expected,
                        "available={available} key={key:?} mode={mode:?}"
                    );
                }
            }
        }
        assert!(callout_visible(true, &KeyStatus::Missing, LocalMode::Off));
        assert!(callout_visible(
            true,
            &KeyStatus::Backend("x".into()),
            LocalMode::Fallback
        ));
        assert!(!callout_visible(true, &KeyStatus::Stored, LocalMode::Off));
        assert!(!callout_visible(
            true,
            &KeyStatus::Missing,
            LocalMode::Primary
        ));
        assert!(!callout_visible(false, &KeyStatus::Missing, LocalMode::Off));
    }

    #[test]
    fn use_on_device_drops_a_voice_that_reuses_the_stt_key_to_verbatim() {
        let mut draft = Settings::default();
        draft.provider.kind = ProviderKind::Openai;
        draft.voice.default = VoiceName::Clean;
        use_on_device(&mut draft);
        assert_eq!(draft.local_stt.mode, LocalMode::Primary);
        assert_eq!(draft.voice.default, VoiceName::Verbatim);
    }

    #[test]
    fn use_on_device_leaves_a_voice_with_its_own_cleanup_account_alone() {
        let mut draft = Settings::default();
        draft.provider.kind = ProviderKind::Deepgram;
        draft.voice.default = VoiceName::Professional;
        draft.voice.provider = Some(VoiceProvider {
            kind: ProviderKind::Openai,
            base_url: None,
            model: None,
            temperature: None,
            reasoning_effort: None,
            key_account: None,
        });
        let before = draft.clone();
        use_on_device(&mut draft);
        assert_eq!(draft.local_stt.mode, LocalMode::Primary);
        assert_eq!(draft.voice, before.voice);
        // Nothing but the mode moves; the model choice in particular stays.
        draft.local_stt.mode = before.local_stt.mode;
        assert_eq!(draft, before);
    }

    #[test]
    fn privacy_and_mode_lines_follow_the_mode() {
        assert!(privacy_line(LocalMode::Off).starts_with("Cloud transcription sends audio"));
        assert!(privacy_line(LocalMode::Fallback).contains("on-device backup"));
        assert!(privacy_line(LocalMode::Primary).contains("keeps audio on this computer"));
        for mode in MODES {
            assert!(privacy_line(mode).contains("cleanup provider"), "{mode:?}");
        }

        assert_eq!(mode_line(LocalMode::Off, 6_000), None);
        assert!(mode_line(LocalMode::Primary, 6_000)
            .unwrap()
            .starts_with("Audio stays on this computer."));
        assert!(mode_line(LocalMode::Fallback, 6_000)
            .unwrap()
            .contains("longer than 6 s,"));
        assert!(mode_line(LocalMode::Fallback, 2_500)
            .unwrap()
            .contains("longer than 2.5 s,"));
    }
}
