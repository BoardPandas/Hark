//! Main-window preferences and the shared explicit-exit path.

use egui::{Context, ViewportCommand, ViewportId, WindowLevel};
use hark_config::General;

#[derive(Default)]
pub struct Behavior {
    quitting: bool,
    applied_always_on_top: Option<bool>,
}

impl Behavior {
    pub fn apply(&mut self, ctx: &Context, settings: &General) {
        if self.applied_always_on_top == Some(settings.always_on_top) {
            return;
        }
        let level = if settings.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        // Target the main window explicitly; the recording overlay has its
        // own always-on-top policy and must not inherit this preference.
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::WindowLevel(level));
        self.applied_always_on_top = Some(settings.always_on_top);
    }

    /// Let eframe return and drop the pipeline/storage in their normal order.
    pub fn quit(&mut self, ctx: &Context) {
        self.quitting = true;
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
    }

    /// Returns true when the close request was converted into a tray hide.
    pub fn handle_close(&self, ctx: &Context, settings: &General, has_tray: bool) -> bool {
        if !ctx.input(|i| i.viewport().close_requested())
            || self.quitting
            || settings.exit_on_close
            || !has_tray
        {
            return false;
        }
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::CancelClose);
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Visible(false));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        ctx: &Context,
        close_requested: bool,
        mut action: impl FnMut(&Context),
    ) -> Vec<ViewportCommand> {
        let mut input = egui::RawInput::default();
        if close_requested {
            input
                .viewports
                .get_mut(&ViewportId::ROOT)
                .unwrap()
                .events
                .push(egui::ViewportEvent::Close);
        }
        let mut output = ctx.run_ui(input, |ui| action(ui.ctx()));
        // This harness has no renderer to consume font texture uploads.
        output.textures_delta.clear();
        // egui also emits an OS-theme command on its first frame.
        output.viewport_output[&ViewportId::ROOT]
            .commands
            .iter()
            .filter(|command| !matches!(command, ViewportCommand::SetTheme(_)))
            .cloned()
            .collect()
    }

    #[test]
    fn window_close_respects_preference_and_never_hides_without_a_tray() {
        for close_requested in [false, true] {
            for exit_on_close in [false, true] {
                for has_tray in [false, true] {
                    let settings = General {
                        exit_on_close,
                        ..Default::default()
                    };
                    let behavior = Behavior::default();
                    let should_hide = close_requested && !exit_on_close && has_tray;
                    let commands = frame(&Context::default(), close_requested, |ctx| {
                        assert_eq!(behavior.handle_close(ctx, &settings, has_tray), should_hide);
                    });
                    let expected = if should_hide {
                        vec![
                            ViewportCommand::CancelClose,
                            ViewportCommand::Visible(false),
                        ]
                    } else {
                        vec![]
                    };
                    assert_eq!(commands, expected);
                }
            }
        }
    }

    #[test]
    fn explicit_quit_bypasses_close_to_tray_even_on_the_next_frame() {
        let ctx = Context::default();
        let mut behavior = Behavior::default();
        assert_eq!(
            frame(&ctx, false, |ctx| behavior.quit(ctx)),
            vec![ViewportCommand::Close]
        );
        assert!(frame(&ctx, true, |ctx| {
            assert!(!behavior.handle_close(ctx, &General::default(), true));
        })
        .is_empty());
    }

    #[test]
    fn always_on_top_applies_on_startup_and_can_be_turned_off_without_restarting() {
        let ctx = Context::default();
        let mut behavior = Behavior::default();
        let mut settings = General {
            always_on_top: true,
            ..Default::default()
        };
        assert_eq!(
            frame(&ctx, false, |ctx| behavior.apply(ctx, &settings)),
            vec![ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop)]
        );
        assert!(frame(&ctx, false, |ctx| behavior.apply(ctx, &settings)).is_empty());
        settings.always_on_top = false;
        assert_eq!(
            frame(&ctx, false, |ctx| behavior.apply(ctx, &settings)),
            vec![ViewportCommand::WindowLevel(WindowLevel::Normal)]
        );
    }
}
