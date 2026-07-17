//! The system tray (spec §3.8): a menu with the voice radio group, Open
//! Settings, and Quit; a state icon + tooltip mapped 1:1 from
//! `PipelineStatus`. Created lazily on the first `App` callback: main
//! thread with the event loop already running, which is the macOS hard
//! requirement (invisible on a Windows-only dev loop; right by
//! construction).
//!
//! Event delivery while the window is hidden: `MenuEvent::receiver()` and
//! `TrayIconEvent::receiver()` are global static channels, and a hidden,
//! idle window paints no frames to drain them. Same pattern as the
//! pipeline pump (CP2): one thread per receiver `recv()`s, forwards onto a
//! UI-side channel, and wakes the event loop per event. The global
//! channels never disconnect, so the pumps park in `recv` holding only a
//! `Context` clone and a sender, and die with the process.

pub mod icon;

use crate::pipeline::PipelineStatus;
use crate::ui::settings::form::{voice_display, VOICES};
use hark_config::VoiceName;
use std::sync::mpsc::{self, Receiver};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

const OPEN_SETTINGS_ID: &str = "open-settings";
const QUIT_ID: &str = "quit";
const VOICE_ID_PREFIX: &str = "voice:";

/// What a tray interaction asks of the app.
#[derive(Debug, PartialEq, Eq)]
pub enum TrayAction {
    SelectVoice(VoiceName),
    OpenSettings,
    /// Double-click on the icon: bring the window back, current page.
    ShowWindow,
    Quit,
}

pub struct Tray {
    /// Keeps the OS icon alive; dropping it removes the tray entry.
    tray: TrayIcon,
    voices: Vec<(VoiceName, CheckMenuItem)>,
    actions: Receiver<TrayAction>,
    shown: icon::TrayState,
    tooltip: String,
    checked: VoiceName,
}

impl Tray {
    /// Build the menu + icon and start the event pumps. Called at most once
    /// per process (the pumps drain global channels; a second set would
    /// double-deliver every event).
    pub fn create(
        ctx: &egui::Context,
        status: &PipelineStatus,
        chord: &str,
        voice: VoiceName,
    ) -> Result<Tray, String> {
        let err = |e: &dyn std::fmt::Display| e.to_string();
        let menu = Menu::new();
        let mut voices = Vec::new();
        for v in VOICES {
            let item = CheckMenuItem::with_id(
                format!("{VOICE_ID_PREFIX}{}", v.label()),
                voice_display(v),
                true,
                v == voice,
                None,
            );
            menu.append(&item).map_err(|e| err(&e))?;
            voices.push((v, item));
        }
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| err(&e))?;
        menu.append(&MenuItem::with_id(
            OPEN_SETTINGS_ID,
            "Open Settings",
            true,
            None,
        ))
        .map_err(|e| err(&e))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| err(&e))?;
        menu.append(&MenuItem::with_id(QUIT_ID, "Quit Hark", true, None))
            .map_err(|e| err(&e))?;

        let state = icon::state(status);
        let tooltip = icon::tooltip(status, chord);
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(build_icon(state))
            .with_tooltip(&tooltip)
            .build()
            .map_err(|e| err(&e))?;

        Ok(Tray {
            tray,
            voices,
            actions: spawn_pumps(ctx.clone()),
            shown: state,
            tooltip,
            checked: voice,
        })
    }

    /// Drain pending tray interactions; called from `App::logic`.
    pub fn take_actions(&self) -> Vec<TrayAction> {
        let mut out = Vec::new();
        while let Ok(action) = self.actions.try_recv() {
            out.push(action);
        }
        out
    }

    /// Reconcile the OS tray with the pipeline + settings; cheap no-op
    /// unless something changed. OS-level set failures are logged and
    /// retried on the next state change; the in-window footer stays the
    /// authoritative status surface either way.
    pub fn apply(&mut self, status: &PipelineStatus, chord: &str, voice: VoiceName) {
        let state = icon::state(status);
        if state != self.shown {
            if let Err(e) = self.tray.set_icon(Some(build_icon(state))) {
                log::warn!("tray icon update failed: {e}");
            }
            self.shown = state;
        }
        let tooltip = icon::tooltip(status, chord);
        if tooltip != self.tooltip {
            if let Err(e) = self.tray.set_tooltip(Some(&tooltip)) {
                log::warn!("tray tooltip update failed: {e}");
            }
            self.tooltip = tooltip;
        }
        if voice != self.checked {
            self.set_voice(voice);
        }
    }

    /// Force every check item to the radio-group truth. Unconditional on
    /// purpose: a native `CheckMenuItem` toggles itself on click, so even a
    /// click on the already-selected voice needs its checkmark restored.
    pub fn set_voice(&mut self, voice: VoiceName) {
        for (v, item) in &self.voices {
            item.set_checked(*v == voice);
        }
        self.checked = voice;
    }
}

fn build_icon(state: icon::TrayState) -> Icon {
    Icon::from_rgba(icon::rgba(state), icon::SIZE, icon::SIZE)
        .expect("the RGBA buffer is SIZE*SIZE*4 by construction")
}

/// One pump thread per global receiver (module docs). Menu clicks arrive
/// as ids; icon double-clicks bring the window back.
fn spawn_pumps(ctx: egui::Context) -> Receiver<TrayAction> {
    let (tx, rx) = mpsc::channel();
    let menu_tx = tx.clone();
    let menu_ctx = ctx.clone();
    std::thread::Builder::new()
        .name("hark-tray-menu-pump".to_string())
        .spawn(move || {
            for event in MenuEvent::receiver() {
                let Some(action) = action_for_id(event.id.as_ref()) else {
                    continue;
                };
                if menu_tx.send(action).is_err() {
                    break;
                }
                menu_ctx.request_repaint();
            }
        })
        .expect("spawning the tray menu pump cannot fail");
    std::thread::Builder::new()
        .name("hark-tray-icon-pump".to_string())
        .spawn(move || {
            for event in TrayIconEvent::receiver() {
                if !matches!(event, TrayIconEvent::DoubleClick { .. }) {
                    continue;
                }
                if tx.send(TrayAction::ShowWindow).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        })
        .expect("spawning the tray icon pump cannot fail");
    rx
}

/// Menu id -> action (the testable seam). Unknown ids are ignored, not
/// errors: muda can synthesize events we never subscribed to.
fn action_for_id(id: &str) -> Option<TrayAction> {
    if id == OPEN_SETTINGS_ID {
        return Some(TrayAction::OpenSettings);
    }
    if id == QUIT_ID {
        return Some(TrayAction::Quit);
    }
    let label = id.strip_prefix(VOICE_ID_PREFIX)?;
    VOICES
        .into_iter()
        .find(|v| v.label() == label)
        .map(TrayAction::SelectVoice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_voice_menu_id_round_trips_to_its_select_action() {
        for v in VOICES {
            let id = format!("{VOICE_ID_PREFIX}{}", v.label());
            assert_eq!(action_for_id(&id), Some(TrayAction::SelectVoice(v)));
        }
    }

    #[test]
    fn fixed_menu_ids_map_to_their_actions() {
        assert_eq!(
            action_for_id("open-settings"),
            Some(TrayAction::OpenSettings)
        );
        assert_eq!(action_for_id("quit"), Some(TrayAction::Quit));
    }

    #[test]
    fn unknown_ids_are_ignored() {
        assert_eq!(action_for_id("voice:robotic"), None);
        assert_eq!(action_for_id("nonsense"), None);
        assert_eq!(action_for_id(""), None);
    }
}
