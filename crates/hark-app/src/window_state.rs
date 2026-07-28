//! Where the main window opens, remembered across launches.
//!
//! eframe can do this itself, and for a single-window app it should. Hark is
//! not one: the recording pill is a second (deferred) viewport, and eframe's
//! auto-save runs at the end of *whichever* viewport just painted, storing that
//! viewport's geometry under the root window's key. Any dictation that outlived
//! the 30 s auto-save interval therefore saved the 160x40 pill as "the Hark
//! window", and the next launch restored a window barely bigger than its own
//! title bar — `min_inner_size` does not rescue it, because eframe clamps a
//! restored size only to a 64x64 floor. So `main.rs` turns eframe's window
//! persistence off and this module keeps the geometry, reading it from the root
//! viewport alone.
//!
//! Sizes and positions here are egui points, the same unit
//! `ViewportCommand::InnerSize` / `OuterPosition` and `ViewportInfo`'s rects
//! use, so a value round-trips exactly on an unchanged display.
//!
//! eframe still reads its own (possibly poisoned) key when it creates the
//! window, which is why [`Memory::tick`] applies our geometry on the first
//! pass rather than trusting what we were handed. The window starts hidden
//! (`main.rs`), so the correction lands before anything is on screen.

use eframe::Storage;

/// Our key in eframe's storage file. Deliberately not eframe's own `window`
/// key: that one stays whatever the last eframe wrote, and is ignored.
const KEY: &str = "hark_window_geometry";

/// Size the window opens at with nothing remembered.
pub const DEFAULT_SIZE: egui::Vec2 = egui::vec2(960.0, 640.0);
/// Smallest window the user can make. Doubles as the plausibility floor for a
/// captured geometry: anything under it is not a size a user chose — it is a
/// minimized window (Windows reports 0x0), or another viewport's.
pub const MIN_SIZE: egui::Vec2 = egui::vec2(720.0, 480.0);

/// A main-window placement: inner (client) size, and the outer top-left when
/// the platform reported one.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Geometry {
    size: egui::Vec2,
    pos: Option<egui::Pos2>,
}

/// The remembered geometry over one app run: restored at startup, refreshed
/// from the live window, written back by `App::save`.
pub struct Memory {
    /// Last plausible geometry the root viewport reported; what `save` writes.
    /// Seeded with the restored value so a run that never shows the window
    /// re-saves what it was given instead of forgetting it.
    last: Option<Geometry>,
    /// Restored geometry, still to be applied to the freshly created window.
    restore: Option<Geometry>,
    applied: bool,
}

impl Memory {
    /// Read the stored geometry (absent, unparsable, or implausible values all
    /// mean "no memory", which opens the default window).
    pub fn load(storage: Option<&dyn Storage>) -> Memory {
        let restore = storage
            .and_then(|s| s.get_string(KEY))
            .and_then(|text| parse(&text));
        Memory {
            last: restore,
            restore,
            applied: false,
        }
    }

    /// Call once per `App::logic` pass — that is, from the root viewport only,
    /// the one viewport whose geometry is the main window's.
    pub fn tick(&mut self, ctx: &egui::Context) {
        if !self.applied {
            self.applied = true;
            apply(ctx, self.restore);
            // The command reaches the window after this pass, so the rect we
            // could read now is the one eframe created, not the one we want.
            return;
        }
        if let Some(geometry) = capture(ctx) {
            self.last = Some(geometry);
        }
    }

    pub fn save(&self, storage: &mut dyn Storage) {
        if let Some(g) = self.last {
            storage.set_string(KEY, format(g));
        }
    }
}

/// Place and size the freshly created window. With nothing remembered — or a
/// remembered spot no longer on any monitor — it opens at the default size,
/// centred: the window eframe created may sit wherever a stale key put it (for
/// upgrading users, that is the recording pill's spot near the bottom edge,
/// where a full-size window would hang off the screen).
fn apply(ctx: &egui::Context, remembered: Option<Geometry>) {
    let size = remembered.map_or(DEFAULT_SIZE, |g| g.size);
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    let pos = remembered
        .and_then(|g| g.pos)
        .filter(|pos| on_a_monitor(ctx, *pos))
        .or_else(|| centered(ctx, size));
    if let Some(pos) = pos {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
}

/// Would a window placed here be reachable? The remembered position can name a
/// monitor that is no longer attached (an undocked laptop, a screen unplugged),
/// and Hark lives in the tray: a window restored into the void has no visible
/// way back. eframe clamps its own restored positions for the same reason;
/// with its persistence off, the check has to live here.
#[cfg(windows)]
fn on_a_monitor(ctx: &egui::Context, pos: egui::Pos2) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL};

    // `OuterPosition` is applied as `pixels_per_point * pos`, so ask about the
    // physical pixel the window will actually land on. A few points in from
    // the corner keeps rounding from deciding the answer.
    let ppp = ctx.pixels_per_point();
    let point = POINT {
        x: ((pos.x + 8.0) * ppp) as i32,
        y: ((pos.y + 8.0) * ppp) as i32,
    };
    // SAFETY: a pure getter over a local POINT. A null return is the
    // documented MONITOR_DEFAULTTONULL answer for "no monitor covers this".
    !unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) }.is_invalid()
}

/// Off Windows the platform has not been brought up yet (checkpoint 7), and
/// egui alone cannot answer this: it exposes a monitor size but no origin
/// (LL-G `egui-viewport-position-monitor-size`).
#[cfg(not(windows))]
fn on_a_monitor(_ctx: &egui::Context, _pos: egui::Pos2) -> bool {
    true
}

/// Read the live root-viewport geometry, or `None` when it is not a placement
/// worth remembering (see [`MIN_SIZE`]).
fn capture(ctx: &egui::Context) -> Option<Geometry> {
    let (inner, outer, minimized) = ctx.input(|i| {
        let v = i.viewport();
        (v.inner_rect, v.outer_rect, v.minimized)
    });
    // A minimized window's geometry is the taskbar's, not the user's.
    if minimized == Some(true) {
        return None;
    }
    let size = inner?.size();
    if !size.x.is_finite() || !size.y.is_finite() {
        return None;
    }
    // A one-point tolerance: the OS rounds a restored size to whole pixels,
    // and a window sitting exactly at the minimum must still be remembered.
    if size.x + 1.0 < MIN_SIZE.x || size.y + 1.0 < MIN_SIZE.y {
        return None;
    }
    let pos = outer
        .map(|r| r.min)
        .filter(|p| p.x.is_finite() && p.y.is_finite());
    Some(Geometry { size, pos })
}

/// Centre a window of `size` on the current monitor. `monitor_size` is all
/// egui exposes — no origin, no per-monitor DPI (LL-G
/// `egui-viewport-position-monitor-size`) — so on a multi-monitor desktop this
/// can centre on the primary rather than the monitor Hark was last on. That is
/// the accepted trade for a first launch: visible and roughly centred beats
/// precisely placed at coordinates inherited from something else.
fn centered(ctx: &egui::Context, size: egui::Vec2) -> Option<egui::Pos2> {
    let monitor = ctx.input(|i| i.viewport().monitor_size)?;
    if monitor.x <= 1.0 || monitor.y <= 1.0 {
        return None;
    }
    Some(egui::pos2(
        ((monitor.x - size.x) / 2.0).max(0.0),
        ((monitor.y - size.y) / 2.0).max(0.0),
    ))
}

/// `"width height"` plus `" x y"` when a position is known. Plain text on
/// purpose: no serde derive, and a hand-editable line if anyone ever has to
/// rescue a config by hand.
fn format(g: Geometry) -> String {
    match g.pos {
        Some(p) => format!("{} {} {} {}", g.size.x, g.size.y, p.x, p.y),
        None => format!("{} {}", g.size.x, g.size.y),
    }
}

fn parse(text: &str) -> Option<Geometry> {
    let parts: Vec<f32> = text
        .split_whitespace()
        .map(|n| n.parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    let (size, pos) = match parts[..] {
        [w, h] => (egui::vec2(w, h), None),
        [w, h, x, y] => (egui::vec2(w, h), Some(egui::pos2(x, y))),
        _ => return None,
    };
    if !size.x.is_finite() || !size.y.is_finite() || size.x < MIN_SIZE.x || size.y < MIN_SIZE.y {
        // Written by a Hark that had this bug, or hand-edited into nonsense.
        return None;
    }
    if pos.is_some_and(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return None;
    }
    Some(Geometry { size, pos })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_round_trips_through_storage_text() {
        let g = Geometry {
            size: egui::vec2(1280.0, 800.0),
            pos: Some(egui::pos2(120.0, 64.0)),
        };
        assert_eq!(parse(&format(g)), Some(g));

        let sizeless = Geometry {
            size: egui::vec2(1024.0, 768.0),
            pos: None,
        };
        assert_eq!(parse(&format(sizeless)), Some(sizeless));
    }

    #[test]
    fn a_pill_sized_geometry_is_never_restored() {
        // The exact shape of the bug this module exists for: the 160x40
        // recording overlay saved as the main window.
        assert_eq!(parse("160 40 880 1000"), None);
        assert_eq!(parse("160 40"), None);
    }

    #[test]
    fn junk_is_no_memory_rather_than_a_bad_window() {
        for text in [
            "",
            "   ",
            "wide tall",
            "960",
            "960 640 10",
            "960 640 10 20 30",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
        assert_eq!(parse("960 inf 0 0"), None);
        assert_eq!(parse("960 640 NaN 0"), None);
    }

    #[test]
    fn the_minimum_window_size_is_still_worth_remembering() {
        let g = Geometry {
            size: MIN_SIZE,
            pos: Some(egui::pos2(0.0, 0.0)),
        };
        assert_eq!(parse(&format(g)), Some(g));
    }
}
