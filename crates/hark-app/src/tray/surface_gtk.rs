//! The tray surface on Linux: a dedicated GTK thread owns the tray.
//!
//! **Why a whole thread.** `tray-icon` renders a Linux tray through
//! libappindicator, and the indicator, the muda menu and every `CheckMenuItem`
//! are GTK widgets. GTK widgets are not `Send`, they require `gtk::init()` on
//! their thread, and they only do anything while a GTK main loop iterates.
//! Hark's main thread is already spoken for: eframe/winit owns it and runs its
//! own event loop, which is not a GTK one. Two event loops cannot share a
//! thread, so the tray gets its own.
//!
//! That is safe here in a way it would not be on macOS: the AppKit
//! main-thread-only rule (the hard rule in CLAUDE.md) is a *macOS* rule, and
//! this file never compiles there. On Linux nothing requires the tray to be on
//! the thread that owns the window.
//!
//! **Manual iteration, not `gtk::main()`.** `gtk::main()` never returns, so the
//! thread could not also drain the update channel. Instead the loop pumps GTK
//! until it has nothing pending, applies whatever the UI thread queued, and
//! sleeps. A tray icon has no animation and no latency budget, so a [`PUMP`]
//! cadence is imperceptible and costs nothing measurable when idle.
//!
//! **Teardown rides the channel.** Dropping the [`Surface`] drops the sender;
//! the next `try_recv` reports `Disconnected`, the loop breaks, and the
//! `TrayIcon` drops on the GTK thread that made it — which is what removes the
//! icon from the panel. Quitting Hark therefore takes the tray with it instead
//! of leaving a dead icon behind.

use super::{build_menu, TrayUpdate};
use crate::pipeline::PipelineStatus;
use crate::tray::icon;
use hark_config::VoiceName;
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::time::Duration;
use tray_icon::TrayIconBuilder;

/// How often the GTK thread pumps its loop and drains queued updates. Fast
/// enough that a voice change or a state icon lands with no perceptible lag,
/// slow enough to be free when nothing is happening.
const PUMP: Duration = Duration::from_millis(50);

pub(super) struct Surface {
    /// Dropping this stops the GTK thread and removes the tray icon.
    updates: Sender<TrayUpdate>,
    /// Joined on drop so the icon is gone before the process continues to
    /// exit; a detached thread could outlive the panel's redraw and leave a
    /// ghost icon until the panel next refreshed.
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Surface {
    pub(super) fn create(
        status: &PipelineStatus,
        chord: &str,
        voice: VoiceName,
    ) -> Result<Surface, String> {
        let (updates, rx) = mpsc::channel();
        // Creation happens on the GTK thread (the widgets cannot be built
        // anywhere else), so the outcome has to come back over a channel
        // rather than as this function's return value.
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let state = icon::state(status);
        let tooltip = icon::tooltip(status, chord);
        let thread = std::thread::Builder::new()
            .name("hark-tray-gtk".to_string())
            .spawn(move || {
                if let Err(e) = gtk::init() {
                    // No display to put a tray on (a headless session, a
                    // broken DISPLAY). The app runs on without one.
                    let _ = ready_tx.send(Err(format!("cannot initialize GTK: {e}")));
                    return;
                }
                let built = build_menu(voice).and_then(|(menu, voices)| {
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu))
                        .with_icon(icon::build(state))
                        .with_tooltip(&tooltip)
                        .build()
                        .map(|tray| (tray, voices))
                        .map_err(|e| e.to_string())
                });
                let (mut tray, voices) = match built {
                    Ok(built) => built,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                pump(&mut tray, &voices, &rx);
                // `tray` drops here, on the thread that created it, which is
                // what actually removes the icon from the panel.
                log::info!("tray: GTK thread stopped");
            })
            .map_err(|e| format!("cannot spawn the tray thread: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Surface {
                updates,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("the tray thread died before reporting readiness".to_string()),
        }
    }

    /// Queue an update for the GTK thread. A send that fails means that thread
    /// is gone; the tray is already absent, so there is nothing to retry and
    /// the in-window footer remains the authoritative status surface.
    pub(super) fn apply(&mut self, update: TrayUpdate) {
        if self.updates.send(update).is_err() {
            log::warn!("tray update dropped: the GTK thread is no longer running");
        }
    }
}

/// Iterate GTK and apply queued updates until the sender goes away.
fn pump(
    tray: &mut tray_icon::TrayIcon,
    voices: &[(VoiceName, tray_icon::menu::CheckMenuItem)],
    rx: &mpsc::Receiver<TrayUpdate>,
) {
    loop {
        // `false` = do not block when the queue is empty, which is what keeps
        // this loop able to reach the channel at all.
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        loop {
            match rx.try_recv() {
                Ok(update) => apply_on_gtk_thread(tray, voices, update),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        std::thread::sleep(PUMP);
    }
}

fn apply_on_gtk_thread(
    tray: &mut tray_icon::TrayIcon,
    voices: &[(VoiceName, tray_icon::menu::CheckMenuItem)],
    update: TrayUpdate,
) {
    match update {
        TrayUpdate::Icon(state) => {
            if let Err(e) = tray.set_icon(Some(icon::build(state))) {
                log::warn!("tray icon update failed: {e}");
            }
        }
        TrayUpdate::Tooltip(tooltip) => {
            // libappindicator has no tooltip concept, so tray-icon's Linux
            // `set_tooltip` is a no-op that returns Ok. Called anyway: the
            // day it grows one, this is already wired, and the branch keeps
            // the two surfaces reading the same.
            if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
                log::warn!("tray tooltip update failed: {e}");
            }
        }
        TrayUpdate::Voice(voice) => {
            for (v, item) in voices {
                item.set_checked(*v == voice);
            }
        }
    }
}
