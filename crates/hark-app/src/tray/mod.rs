//! The system tray (spec §3.8): a menu with the voice radio group, Open
//! Settings, and Quit; a state icon + tooltip mapped 1:1 from
//! `PipelineStatus`. Created lazily on the first `App` callback: main
//! thread with the event loop already running, which is the macOS hard
//! requirement (invisible on a Windows-only dev loop; right by
//! construction).
//!
//! **Where the OS objects live is the platform seam.** This module owns the
//! menu layout, the id-to-action mapping and the change detection — all pure,
//! all shared — and hands single [`TrayUpdate`]s to a `Surface`.
//! `surface_native` (Windows, macOS) holds the `TrayIcon` on the main thread
//! and calls straight through. `surface_gtk` (Linux) cannot: libappindicator
//! makes the tray out of GTK widgets, which need a GTK main loop that would
//! fight winit's for the main thread, so it runs them on a thread of their
//! own. Everything above the seam is identical on all three.
//!
//! Event delivery while the window is hidden: `MenuEvent::receiver()` and
//! `TrayIconEvent::receiver()` are global static channels, and a hidden,
//! idle window paints no frames to drain them. Same pattern as the
//! pipeline pump (CP2): one thread per receiver `recv()`s, forwards onto a
//! UI-side channel, and wakes the event loop per event. The global
//! channels never disconnect, so the pumps park in `recv` holding only a
//! `Context` clone and a sender, and die with the process.

pub mod icon;

#[cfg(target_os = "linux")]
#[path = "surface_gtk.rs"]
mod surface;
#[cfg(not(target_os = "linux"))]
#[path = "surface_native.rs"]
mod surface;

use crate::pipeline::PipelineStatus;
use crate::ui::settings::form::{voice_display, VOICES};
use hark_config::VoiceName;
use std::sync::mpsc::{self, Receiver};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconEvent;

const OPEN_SETTINGS_ID: &str = "open-settings";
const QUIT_ID: &str = "quit";
const VOICE_ID_PREFIX: &str = "voice:";
/// Start or stop meeting notes. Only built where meetings exist (Windows for
/// now): a menu entry that can never work is worse than none.
const MEETING_ID: &str = "meeting-toggle";
/// Linux only, and not a gratuitous difference: libappindicator exposes no
/// click or double-click event at all, so [`TrayAction::ShowWindow`] — which
/// Windows and macOS raise from a double-click on the icon — is unreachable
/// there. Without this item, a hidden window could only be recovered through
/// "Open Settings", which also switches the page out from under the user. The
/// id is still decoded on every platform so [`action_for_id`] stays pure and
/// its tests do not fork.
const SHOW_WINDOW_ID: &str = "show-window";

/// What a tray interaction asks of the app.
#[derive(Debug, PartialEq, Eq)]
pub enum TrayAction {
    SelectVoice(VoiceName),
    OpenSettings,
    /// Double-click on the icon: bring the window back, current page.
    ShowWindow,
    /// "Start meeting notes" / "Stop meeting notes".
    ToggleMeeting,
    Quit,
}

/// What the meeting menu entry says.
#[derive(Clone, PartialEq, Eq)]
pub enum MeetingTray {
    /// Turned off, or failed to start: the entry shows but is disabled.
    Unavailable,
    Idle,
    /// Recording since this local time ("14:30"). A start time needs no
    /// ticking, so a hidden idle window never has to wake just to update it.
    Recording {
        since: String,
        /// Set once the detected app hung up: when the notes stop ("14:31:05").
        stops_at: Option<String>,
    },
}

impl MeetingTray {
    fn label(&self) -> String {
        match self {
            MeetingTray::Unavailable | MeetingTray::Idle => "Start meeting notes".to_string(),
            MeetingTray::Recording { since, .. } => format!("Stop meeting notes (since {since})"),
        }
    }
}

/// One OS-level change to the tray. The diffing that produces these is pure
/// and lives in [`Tray::apply`]; a `Surface` only has to get them to the OS.
pub(crate) enum TrayUpdate {
    Icon(icon::TrayState),
    Tooltip(String),
    Voice(VoiceName),
    /// The meeting entry's text and whether it is clickable.
    Meeting {
        label: String,
        enabled: bool,
    },
}

pub struct Tray {
    /// Owns the OS objects, wherever this platform requires them to live.
    /// Dropping it removes the tray entry.
    surface: surface::Surface,
    actions: Receiver<TrayAction>,
    shown: icon::TrayState,
    tooltip: String,
    checked: VoiceName,
    meeting: MeetingTray,
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
        let surface = surface::Surface::create(status, chord, voice)?;
        Ok(Tray {
            surface,
            actions: spawn_pumps(ctx.clone()),
            shown: icon::state(status),
            tooltip: icon::tooltip(status, chord),
            checked: voice,
            meeting: MeetingTray::Unavailable,
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
    /// unless something changed. This runs every frame, so the diffing is the
    /// point: on Linux each update also costs a channel hop and a PNG
    /// rewritten to a temp file, which is not something to do 60 times a
    /// second for a state that did not change.
    pub fn apply(
        &mut self,
        status: &PipelineStatus,
        chord: &str,
        voice: VoiceName,
        meeting: &MeetingTray,
    ) {
        let (state, tooltip) = with_meeting(status, chord, meeting);
        if state != self.shown {
            self.surface.apply(TrayUpdate::Icon(state));
            self.shown = state;
        }
        if tooltip != self.tooltip {
            self.surface.apply(TrayUpdate::Tooltip(tooltip.clone()));
            self.tooltip = tooltip;
        }
        if voice != self.checked {
            self.set_voice(voice);
        }
        if *meeting != self.meeting {
            self.surface.apply(TrayUpdate::Meeting {
                label: meeting.label(),
                enabled: *meeting != MeetingTray::Unavailable,
            });
            self.meeting = meeting.clone();
        }
    }

    /// Force every check item to the radio-group truth. Unconditional on
    /// purpose: a native `CheckMenuItem` toggles itself on click, so even a
    /// click on the already-selected voice needs its checkmark restored.
    pub fn set_voice(&mut self, voice: VoiceName) {
        self.surface.apply(TrayUpdate::Voice(voice));
        self.checked = voice;
    }
}

/// The icon and tooltip once a meeting is in the picture. The recording state
/// must be visible at all times while a meeting records (plan §4.7), so an
/// otherwise idle tray shows the recording disc; dictation's own states
/// (recording, processing, an error) still win while they last.
fn with_meeting(
    status: &PipelineStatus,
    chord: &str,
    meeting: &MeetingTray,
) -> (icon::TrayState, String) {
    let state = icon::state(status);
    match meeting {
        MeetingTray::Recording { since, stops_at } if state == icon::TrayState::Idle => (
            icon::TrayState::Recording,
            match stops_at {
                Some(at) => format!("Hark: the call ended; meeting notes stop at {at}"),
                None => format!("Hark: taking meeting notes since {since}"),
            },
        ),
        _ => (state, icon::tooltip(status, chord)),
    }
}

/// Build the menu and hand back the voice items, which stay live so their
/// checkmarks can be set later. Shared by both surfaces: the layout and the
/// ids are what [`action_for_id`] decodes, so they must not diverge by
/// platform.
/// Also hands back the meeting entry where meetings exist.
#[allow(clippy::type_complexity)]
fn build_menu(
    voice: VoiceName,
) -> Result<(Menu, Vec<(VoiceName, CheckMenuItem)>, Option<MenuItem>), String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let menu = Menu::new();
    menu.append(&MenuItem::new("Voice", false, None))
        .map_err(|e| err(&e))?;
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
    let meeting = if hark_pipeline::meeting::meetings_supported() {
        let item = MenuItem::with_id(MEETING_ID, MeetingTray::Unavailable.label(), false, None);
        menu.append(&item).map_err(|e| err(&e))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| err(&e))?;
        Some(item)
    } else {
        None
    };
    menu.append(&MenuItem::with_id(SHOW_WINDOW_ID, "Open Hark", true, None))
        .map_err(|e| err(&e))?;
    menu.append(&MenuItem::with_id(
        OPEN_SETTINGS_ID,
        "Settings…",
        true,
        None,
    ))
    .map_err(|e| err(&e))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| err(&e))?;
    menu.append(&MenuItem::with_id(QUIT_ID, "Quit Hark", true, None))
        .map_err(|e| err(&e))?;
    Ok((menu, voices, meeting))
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
                crate::app::wake_ui(&menu_ctx);
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
                crate::app::wake_ui(&ctx);
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
    if id == SHOW_WINDOW_ID {
        return Some(TrayAction::ShowWindow);
    }
    if id == QUIT_ID {
        return Some(TrayAction::Quit);
    }
    if id == MEETING_ID {
        return Some(TrayAction::ToggleMeeting);
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
        assert_eq!(action_for_id("show-window"), Some(TrayAction::ShowWindow));
        assert_eq!(
            action_for_id("meeting-toggle"),
            Some(TrayAction::ToggleMeeting)
        );
    }

    #[test]
    fn a_recording_meeting_shows_on_an_idle_tray_but_dictation_wins() {
        let recording = MeetingTray::Recording {
            since: "14:30".to_string(),
            stops_at: None,
        };
        let (state, tooltip) = with_meeting(&PipelineStatus::Idle, "F9", &recording);
        assert_eq!(state, icon::TrayState::Recording);
        assert!(tooltip.contains("meeting notes since 14:30"));
        let (state, _) = with_meeting(&PipelineStatus::Processing, "F9", &recording);
        assert_eq!(state, icon::TrayState::Processing);
        let (state, _) = with_meeting(&PipelineStatus::Idle, "F9", &MeetingTray::Idle);
        assert_eq!(state, icon::TrayState::Idle);
    }

    #[test]
    fn a_hung_up_call_says_when_the_notes_stop() {
        let ending = MeetingTray::Recording {
            since: "14:30".to_string(),
            stops_at: Some("14:52:07".to_string()),
        };
        let (state, tooltip) = with_meeting(&PipelineStatus::Idle, "F9", &ending);
        assert_eq!(state, icon::TrayState::Recording);
        assert!(tooltip.contains("stop at 14:52:07"), "{tooltip}");
    }

    #[test]
    fn the_meeting_entry_names_its_start_time() {
        assert_eq!(MeetingTray::Idle.label(), "Start meeting notes");
        let recording = MeetingTray::Recording {
            since: "09:05".to_string(),
            stops_at: None,
        };
        assert_eq!(recording.label(), "Stop meeting notes (since 09:05)");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_menu_can_reach_show_window() {
        // On Linux the menu item is the ONLY route to ShowWindow -- there is
        // no icon click event to fall back on -- so the item's id and the
        // decoder have to agree or a hidden Hark cannot be recovered without
        // changing the page.
        assert_eq!(action_for_id(SHOW_WINDOW_ID), Some(TrayAction::ShowWindow));
    }

    #[test]
    fn unknown_ids_are_ignored() {
        assert_eq!(action_for_id("voice:robotic"), None);
        assert_eq!(action_for_id("nonsense"), None);
        assert_eq!(action_for_id(""), None);
    }
}
