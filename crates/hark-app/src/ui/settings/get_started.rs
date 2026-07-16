//! The Get Started card (spec §3.11): shown at the top of Settings when no
//! STT key resolved at startup. Three numbered inline steps earn checks;
//! a passing test swaps the card to its success state; the card goes away
//! on the first successful dictation or an explicit dismiss. No wizard
//! screens, nothing modal.

use crate::theme;
use egui::{Frame, RichText, Sides, Ui};

pub struct GetStarted {
    /// Latched at startup when no key resolved; stays true (even after the
    /// key lands) so the success state gets its moment on screen.
    pub active: bool,
    /// The user touched the provider picker this session.
    pub provider_touched: bool,
    /// The last test-connection run passed STT.
    pub test_passed: bool,
    pub dismissed: bool,
}

impl GetStarted {
    pub fn new(active: bool) -> Self {
        GetStarted {
            active,
            provider_touched: false,
            test_passed: false,
            dismissed: false,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CardView {
    Hidden,
    Steps {
        provider: bool,
        key: bool,
        test: bool,
    },
    Success,
}

/// Pure state -> view mapping (the testable seam). A first successful
/// dictation retires the card permanently: on the next launch the key
/// resolves, so `active` never latches again.
pub fn view(
    card: &GetStarted,
    key_stored: bool,
    pipeline_running: bool,
    injected: bool,
) -> CardView {
    if !card.active || card.dismissed || injected {
        return CardView::Hidden;
    }
    if card.test_passed && pipeline_running {
        return CardView::Success;
    }
    CardView::Steps {
        provider: card.provider_touched || key_stored,
        key: key_stored,
        test: card.test_passed,
    }
}

/// Render. Returns true when the user dismissed the card.
pub fn show(ui: &mut Ui, card_view: &CardView, chord: &str) -> bool {
    if *card_view == CardView::Hidden {
        return false;
    }
    let mut dismissed = false;
    Frame::default()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().window_stroke)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| match card_view {
            CardView::Hidden => {}
            CardView::Steps {
                provider,
                key,
                test,
            } => {
                Sides::new().show(
                    ui,
                    |ui| {
                        ui.label(RichText::new("Get started").text_style(theme::subheading()));
                    },
                    |ui| {
                        dismissed = ui.link(RichText::new("Skip for now").small()).clicked();
                    },
                );
                ui.add_space(2.0);
                for (index, (done, text)) in [
                    (*provider, "Pick your speech-to-text provider"),
                    (*key, "Paste your API key and press Store"),
                    (*test, "Run Test connection"),
                ]
                .into_iter()
                .enumerate()
                {
                    ui.horizontal(|ui| {
                        if done {
                            ui.label(RichText::new(theme::icons::CHECK).color(theme::SUCCESS));
                        } else {
                            ui.label(RichText::new(format!("{}.", index + 1)).weak());
                        }
                        ui.label(text);
                    });
                }
            }
            CardView::Success => {
                Sides::new().show(
                    ui,
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(theme::icons::CHECK).color(theme::SUCCESS));
                            ui.label(RichText::new("You're set").text_style(theme::subheading()));
                        });
                    },
                    |ui| {
                        dismissed = ui.link(RichText::new("Dismiss").small()).clicked();
                    },
                );
                ui.label(format!("Hold {chord} and speak into any text field."));
                ui.label(
                    RichText::new("This card disappears after your first dictation.")
                        .small()
                        .weak(),
                );
            }
        });
    ui.add_space(10.0);
    dismissed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(active: bool) -> GetStarted {
        GetStarted::new(active)
    }

    #[test]
    fn inactive_card_never_shows() {
        // Key resolved at startup: onboarding never latches.
        assert_eq!(view(&card(false), true, true, false), CardView::Hidden);
    }

    #[test]
    fn fresh_onboarding_shows_unchecked_steps() {
        assert_eq!(
            view(&card(true), false, false, false),
            CardView::Steps {
                provider: false,
                key: false,
                test: false
            }
        );
    }

    #[test]
    fn touching_the_provider_or_storing_a_key_checks_step_one() {
        let mut c = card(true);
        c.provider_touched = true;
        assert_eq!(
            view(&c, false, false, false),
            CardView::Steps {
                provider: true,
                key: false,
                test: false
            }
        );
        // A stored key implies the provider was chosen, however it happened.
        assert_eq!(
            view(&card(true), true, false, false),
            CardView::Steps {
                provider: true,
                key: true,
                test: false
            }
        );
    }

    #[test]
    fn success_needs_both_a_passing_test_and_a_running_pipeline() {
        let mut c = card(true);
        c.test_passed = true;
        // Test passed but the pipeline is not up yet (save pending): the
        // card must not promise "hold and speak".
        assert!(matches!(
            view(&c, true, false, false),
            CardView::Steps { test: true, .. }
        ));
        assert_eq!(view(&c, true, true, false), CardView::Success);
    }

    #[test]
    fn dismissal_and_first_dictation_hide_the_card() {
        let mut c = card(true);
        c.test_passed = true;
        c.dismissed = true;
        assert_eq!(view(&c, true, true, false), CardView::Hidden);

        let c = card(true);
        assert_eq!(view(&c, true, true, true), CardView::Hidden);
    }
}
