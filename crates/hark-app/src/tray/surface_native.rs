//! The tray surface on Windows and macOS: the `TrayIcon` and its menu items
//! live right here, on the main thread, and updates are direct calls.
//!
//! macOS is why this is the main thread and not some worker: AppKit requires
//! every UI object on it. Windows only requires that the creating thread pump
//! messages, which the eframe event loop does. The Linux surface cannot use
//! this shape at all -- see `surface_gtk.rs`.

use super::{build_menu, TrayUpdate};
use crate::pipeline::PipelineStatus;
use crate::tray::icon;
use hark_config::VoiceName;
use tray_icon::menu::CheckMenuItem;
use tray_icon::{TrayIcon, TrayIconBuilder};

pub(super) struct Surface {
    /// Keeps the OS icon alive; dropping it removes the tray entry.
    tray: TrayIcon,
    voices: Vec<(VoiceName, CheckMenuItem)>,
}

impl Surface {
    pub(super) fn create(
        status: &PipelineStatus,
        chord: &str,
        voice: VoiceName,
    ) -> Result<Surface, String> {
        let (menu, voices) = build_menu(voice)?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon::build(icon::state(status)))
            .with_tooltip(icon::tooltip(status, chord))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Surface { tray, voices })
    }

    /// OS-level set failures are logged and retried on the next state change;
    /// the in-window footer stays the authoritative status surface either way.
    pub(super) fn apply(&mut self, update: TrayUpdate) {
        match update {
            TrayUpdate::Icon(state) => {
                if let Err(e) = self.tray.set_icon(Some(icon::build(state))) {
                    log::warn!("tray icon update failed: {e}");
                }
            }
            TrayUpdate::Tooltip(tooltip) => {
                if let Err(e) = self.tray.set_tooltip(Some(&tooltip)) {
                    log::warn!("tray tooltip update failed: {e}");
                }
            }
            TrayUpdate::Voice(voice) => {
                for (v, item) in &self.voices {
                    item.set_checked(*v == voice);
                }
            }
        }
    }
}
