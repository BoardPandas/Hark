//! Provider-specific rows under the Speech to text radios: Gemini's transcript
//! mode and OpenAI's Live transcribe toggle. Each is drawn only while its own
//! provider is selected, rather than greyed out everywhere.

use crate::theme;
use egui::{RichText, Ui};
use hark_config::{LiveMode, Provider, ProviderKind, Settings};

/// Whether this build links the WebSocket client that both streaming paths
/// share. Without it, Gemini is not offered and neither is the toggle.
pub(crate) const LIVE_BUILD: bool = cfg!(feature = "live-stt");

/// The cost line under the toggle. Live is the pricier OpenAI option, and the
/// one place a user decides to pay for it is here.
pub(crate) const LIVE_COST_NOTE: &str =
    "Streams while you hold the key. Billed per audio minute, about 4× gpt-transcribe.";

/// Whether the Live transcribe toggle is offered: OpenAI only, and only in a
/// build that can stream. A slim build would just fall back to batch, so
/// offering the switch there would be a control that does nothing.
pub(crate) fn offers_live_transcribe(kind: ProviderKind) -> bool {
    kind == ProviderKind::Openai && LIVE_BUILD
}

/// Whether the model field gives way to the fixed realtime model. The batch
/// pin itself stays in the draft, untouched, for when the toggle goes off.
///
/// Keyed on the flag, not on `uses_live_transcribe` like the checkbox: a
/// `gpt-live-*` model typed into the field does tick the box, but hiding the
/// field mid-word would pull the text out from under the cursor.
pub(crate) fn model_is_fixed(provider: &Provider) -> bool {
    offers_live_transcribe(provider.kind) && provider.live
}

/// OpenAI's Live transcribe toggle. Same provider, same key slot: nothing to
/// enter twice. Returns true when the box was toggled.
///
/// The box shows [`Provider::uses_live_transcribe`], what the pipeline will
/// actually run, rather than the `live` flag alone: a hand-edited
/// `model = "gpt-live-*"` streams with `live = false`, and a box bound to the
/// flag showed unticked while it did, with no way to turn it off. `model_buf`
/// is the model field's text, which writes back into the draft every frame.
pub(crate) fn live_transcribe_row(
    ui: &mut Ui,
    draft: &mut Settings,
    model_buf: &mut String,
) -> bool {
    ui.add_space(theme::SUBROW_GAP);
    let mut on = draft.provider.uses_live_transcribe();
    let toggled = ui
        .checkbox(&mut on, "Live transcribe")
        .on_hover_text(
            "Opens a realtime session when you press the key and streams your audio while \
             you hold it, so only the last moment is left to transcribe after release. Uses \
             your OpenAI key and the gpt-live-transcribe model. Your cleanup voice still runs.",
        )
        .changed();
    if toggled {
        set_live_transcribe(&mut draft.provider, model_buf, on);
    }
    ui.label(RichText::new(LIVE_COST_NOTE).small().weak());
    toggled
}

/// Apply the checkbox. Off clears `live` and any `gpt-live-*` model, from
/// the draft and from the model field (or the field would write it straight
/// back), so that off really stops the stream. A pinned batch model stays: it
/// is what off runs.
pub(crate) fn set_live_transcribe(provider: &mut Provider, model_buf: &mut String, on: bool) {
    provider.live = on;
    if on {
        return;
    }
    let is_live_model = |m: &str| m.trim().starts_with("gpt-live-");
    if provider.model.as_deref().is_some_and(is_live_model) {
        provider.model = None;
    }
    if is_live_model(model_buf) {
        model_buf.clear();
    }
}

/// Gemini Live's transcript mode.
pub(crate) fn live_mode_row(ui: &mut Ui, draft: &mut Settings) {
    ui.add_space(theme::SUBROW_GAP);
    ui.label(RichText::new("Transcript").weak());
    ui.horizontal_wrapped(|ui| {
        ui.radio_value(
            &mut draft.provider.live_mode,
            LiveMode::Verbatim,
            "Verbatim",
        )
        .on_hover_text(
            "Literal transcript from Gemini, then your cleanup voice runs as a second call on \
             Gemini Flash Lite using the same key. Slower, but your voice and its prompt apply.",
        );
        ui.radio_value(&mut draft.provider.live_mode, LiveMode::Smart, "Smart")
            .on_hover_text(
                "Gemini tidies the text inside the transcription call — one round trip instead of \
                 two, so it is faster. Your cleanup voice does NOT run: Gemini applies its own \
                 generic cleanup, not your prompt. It drops openers like \"so basically\", \
                 carries out spoken edits like \"drop the other bullet points\" instead of \
                 typing them, and can answer a dictation that sounds like a request. Use \
                 Verbatim when you dictate prompts or instructions. History stores the tidied \
                 text rather than what you said, and invocation triggers match against it.",
            );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(kind: ProviderKind, live: bool) -> Provider {
        Provider {
            kind,
            live,
            ..Provider::default()
        }
    }

    #[test]
    fn the_toggle_is_offered_for_openai_only_and_only_when_the_build_can_stream() {
        assert_eq!(offers_live_transcribe(ProviderKind::Openai), LIVE_BUILD);
        for kind in [
            ProviderKind::Deepgram,
            ProviderKind::Gemini,
            ProviderKind::OpenaiCompatible,
        ] {
            assert!(!offers_live_transcribe(kind), "{kind:?}");
        }
    }

    #[test]
    fn the_model_field_is_fixed_only_while_the_toggle_is_on() {
        assert_eq!(
            model_is_fixed(&provider(ProviderKind::Openai, true)),
            LIVE_BUILD
        );
        assert!(!model_is_fixed(&provider(ProviderKind::Openai, false)));
        // A stale flag left on another provider must not hide its model box.
        assert!(!model_is_fixed(&provider(ProviderKind::Deepgram, true)));
    }

    #[test]
    fn unticking_a_hand_edited_live_model_really_turns_live_off() {
        // `model = "gpt-live-transcribe"` with `live = false` streams. The box
        // shows it on, and unticking must stop it rather than do nothing.
        let mut p = Provider {
            kind: ProviderKind::Openai,
            model: Some("gpt-live-transcribe".into()),
            ..Provider::default()
        };
        let mut buf = "gpt-live-transcribe".to_string();
        assert!(p.uses_live_transcribe(), "what the box shows");
        set_live_transcribe(&mut p, &mut buf, false);
        assert!(!p.uses_live_transcribe());
        assert!(!p.live);
        assert_eq!(p.model, None);
        assert!(
            buf.is_empty(),
            "the field would write the model straight back"
        );
    }

    #[test]
    fn unticking_keeps_a_pinned_batch_model_and_ticking_touches_no_model() {
        let mut p = Provider {
            kind: ProviderKind::Openai,
            live: true,
            model: Some("whisper-1".into()),
            ..Provider::default()
        };
        let mut buf = "whisper-1".to_string();
        set_live_transcribe(&mut p, &mut buf, false);
        assert!(!p.uses_live_transcribe());
        assert_eq!(p.model.as_deref(), Some("whisper-1"));
        assert_eq!(buf, "whisper-1");

        set_live_transcribe(&mut p, &mut buf, true);
        assert!(p.live && p.uses_live_transcribe());
        assert_eq!(p.model.as_deref(), Some("whisper-1"));
        assert_eq!(buf, "whisper-1");
    }

    #[test]
    fn the_cost_note_names_the_billing_unit_and_the_comparison() {
        assert!(LIVE_COST_NOTE.contains("per audio minute"));
        assert!(LIVE_COST_NOTE.contains("gpt-transcribe"));
    }
}
