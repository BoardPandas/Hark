//! Pure chord edge detection: a stream of raw per-key events becomes clean
//! `PttEvent::Down` / `PttEvent::Up` edges for the configured chord.
//!
//! Semantics (default chord LCtrl+LWin, confirmed 2026-07-16):
//! - `Down` fires when the LAST chord member goes down (all held).
//! - `Up` fires when the FIRST chord member is released.
//! - Auto-repeat (down while already down) never re-fires an edge. It is still
//!   evidence the key is held, which is how an intercepted key is caught.
//! - Injected events (our own synthesized Ctrl+V) are ignored entirely, or
//!   dictation would paste-inject into an infinite PTT loop.
//! - Keys outside the chord are ignored (we observe, never swallow).

mod chord;

pub use chord::{pretty_chord, ChordParseError, PttChord};

use crate::keycode::PttKeyCode;
use std::time::{Duration, Instant};

/// A push-to-talk edge, sent to the pipeline worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttEvent {
    Down,
    Up,
    /// The chord is no longer physically held, but the release event itself
    /// never arrived: [`ChordTracker::resync_released`] found the keys up.
    /// Same meaning as `Up` for the state machine, but the moment of release
    /// is unknown — it can be anywhere between the last poll and the press —
    /// so the pipeline treats an over-long one as abandoned rather than
    /// injecting whatever the room said after the user let go.
    UpMissed,
    /// Not an edge: this chord member is being intercepted by another
    /// program. The hook saw it auto-repeat, which a released key cannot do,
    /// while the platform's key state said it was up. That happens when a hook
    /// later in the chain (a key remapper, typically) swallows the press after
    /// Hark sees it. Sent at most once per member per tracker. The chord keeps
    /// working while the hook keeps seeing the key, but the other program is
    /// acting on every press too, so the user needs to hear about it.
    Intercepted(PttKeyCode),
}

/// Which chord member, if any, Hark may swallow so its lock does not toggle.
///
/// A pure function of the chord, resolved once. Every clause exists because
/// something worse happens without it:
/// - fewer than 2 keys: a lone Caps Lock chord would make Caps Lock unusable
///   everywhere, forever. The vacuous "all other members are held" is exactly
///   the trap here — there are no other members.
/// - any Alt or Win: see [`PttKeyCode::is_menu_modifier`].
/// - two suppressible locks: the swallowed one reads "up" for the whole hold,
///   so the chord needs at least one member the watchdog can still read
///   truthfully. One lock guarantees a non-lock member remains.
fn swallowable_lock(chord: &PttChord, enabled: bool) -> Option<usize> {
    if !enabled || chord.keys.len() < 2 {
        return None;
    }
    if chord.keys.iter().any(|k| k.is_menu_modifier()) {
        return None;
    }
    let mut found = None;
    for (i, key) in chord.keys.iter().enumerate() {
        if key.is_suppressible_lock() {
            if found.is_some() {
                return None;
            }
            found = Some(i);
        }
    }
    found
}

/// How long the hook's evidence that a key is held outranks the key state.
///
/// A held key auto-repeats through the hook: first after the keyboard delay
/// (250-1000 ms, per the user's setting), then every 33-400 ms. So a chord
/// member the hook saw go down within this window is still held, whatever the
/// key state says. 1.5 s clears the slowest delay Windows offers, with room for
/// a busy machine.
pub const HELD_EVIDENCE: Duration = Duration::from_millis(1_500);

/// The chord state machine. Feed every raw key event the platform hook sees;
/// it emits an edge only on engage/disengage transitions.
pub struct ChordTracker {
    chord: PttChord,
    member_down: Vec<bool>,
    /// When the hook last delivered this member going down, auto-repeats
    /// included: the hook's own evidence that the key is still held.
    last_down: Vec<Option<Instant>>,
    /// The platform's key state has read this member down since it was
    /// pressed. Until it has, a poll reading "up" cannot tell a released key
    /// from one another program swallowed before Windows ever registered it.
    confirmed: Vec<bool>,
    /// Members already reported as intercepted, so each is reported once.
    intercepted: Vec<bool>,
    engaged: bool,
    /// The one member whose lock toggle may be suppressed. A pure function of
    /// the chord, fixed at construction: there is deliberately NO per-press
    /// bookkeeping, so there is no flag to strand and no way to represent a
    /// key stuck swallowed. That is what makes a second return path in the
    /// hook defensible at all.
    lock: Option<usize>,
}

impl ChordTracker {
    pub fn is_engaged(&self) -> bool {
        self.engaged
    }

    pub fn new(chord: PttChord) -> ChordTracker {
        ChordTracker::with_lock_suppression(chord, false)
    }

    pub fn with_lock_suppression(chord: PttChord, swallow_locks: bool) -> ChordTracker {
        let n = chord.keys.len();
        let lock = swallowable_lock(&chord, swallow_locks);
        ChordTracker {
            chord,
            member_down: vec![false; n],
            last_down: vec![None; n],
            confirmed: vec![false; n],
            intercepted: vec![false; n],
            engaged: false,
            lock,
        }
    }

    /// Should the hook swallow the event it has just fed in, so the lock does
    /// not toggle? Call AFTER [`Self::on_event_verified`] has consumed it.
    ///
    /// Key-down only, on purpose. The toggle rides the make code, so
    /// swallowing releases buys nothing — and a swallowed release is the only
    /// way a lock key could end up stuck from the system's point of view.
    /// With down-only, that state is not representable.
    pub fn swallow(&self, key: PttKeyCode, down: bool, injected: bool) -> bool {
        if !down || injected {
            return false;
        }
        let Some(lock) = self.lock else {
            return false;
        };
        // Engaged with this member down means the press that just started the
        // dictation, or a repeat inside it. Derived from the tracker's own
        // state rather than from a physical poll: a poll is strictly weaker
        // than the engage condition and would fire with no dictation running,
        // and the invariant that makes this safe is that every swallowed
        // keystroke has a visible dictation attached to it.
        self.engaged && self.chord.keys[lock] == key && self.member_down[lock]
    }

    /// Process one raw key event, trusting the tracker's own record of what is
    /// held. Pure: the platform is never consulted. Production feeds
    /// [`Self::on_event_verified`] instead; this is the seam the edge-semantics
    /// tests drive.
    pub fn on_event(&mut self, key: PttKeyCode, down: bool, injected: bool) -> Option<PttEvent> {
        self.on_event_verified(key, down, injected, |_| true, Instant::now())
    }

    /// Process one raw key event. `injected` marks synthesized input
    /// (LLKHF_INJECTED on Windows): always ignored, so our own Ctrl+V can
    /// never re-trigger PTT.
    ///
    /// `physically_down` answers "is this key really held right now?" and is
    /// consulted for the *other* chord members only, and only on a press that
    /// is about to engage the chord. A hook does not see every release (see
    /// [`Self::resync_released`]), and while the watchdog heals a lost release
    /// during a hold, a release lost with the chord already disengaged leaves
    /// its member stuck `true` with nothing polling — which quietly demotes the
    /// chord to whichever key is left. With LCtrl+F12 configured and F12's
    /// release missed, a bare Left Ctrl press then starts a dictation. The
    /// engage edge is the only place a stale `true` can do harm, and checking
    /// only there costs at most three key-state reads on the press that starts
    /// a dictation — nothing on any other key, and never a synthesized press
    /// (the poll can clear members, never set them).
    ///
    /// An auto-repeat of a member the key state has not yet confirmed costs one
    /// more read, on the repeat only: see [`PttEvent::Intercepted`]. `now` is
    /// when the hook delivered the event.
    pub fn on_event_verified(
        &mut self,
        key: PttKeyCode,
        down: bool,
        injected: bool,
        mut physically_down: impl FnMut(PttKeyCode) -> bool,
        now: Instant,
    ) -> Option<PttEvent> {
        if injected {
            return None;
        }
        let idx = self.chord.keys.iter().position(|k| *k == key)?;

        if self.member_down[idx] == down {
            // Auto-repeat (down while down) or a stray duplicate release:
            // no state change, no edge.
            if down {
                self.last_down[idx] = Some(now);
                return self.check_intercepted(idx, physically_down);
            }
            return None;
        }
        self.member_down[idx] = down;
        self.confirmed[idx] = false;
        self.last_down[idx] = down.then_some(now);

        // About to engage: confirm the members we did not just see pressed are
        // genuinely still held. Skipped while engaged (the watchdog owns that
        // window) and on releases (they only ever clear state).
        if down && !self.engaged && self.member_down.iter().all(|d| *d) {
            for (i, member) in self.chord.keys.iter().enumerate() {
                if i == idx {
                    continue;
                }
                if physically_down(*member) {
                    self.confirmed[i] = true;
                } else {
                    log::warn!("chord member {member} was marked held but is up; ignoring");
                    self.member_down[i] = false;
                }
            }
        }

        let all_down = self.member_down.iter().all(|d| *d);
        match (self.engaged, all_down) {
            (false, true) => {
                self.engaged = true;
                Some(PttEvent::Down)
            }
            (true, false) => {
                self.engaged = false;
                Some(PttEvent::Up)
            }
            _ => None,
        }
    }

    /// Reconcile the tracker with the keyboard's real state while the chord is
    /// engaged, and emit the release that never arrived.
    ///
    /// The tracker only ever learns about a release from a hook callback, and
    /// a low-level hook does not see every one: releases on another desktop
    /// (lock screen, UAC, Ctrl+Alt+Del), across a sleep/resume, or after
    /// Windows quietly unhooks a callback that ran long, all go missing. One
    /// lost release is not one lost dictation — it wedges the tracker
    /// `engaged` forever, so the recording never ends, the overlay never goes
    /// away, and no later press produces an edge either (the members it needs
    /// are already marked down). This is the way out: ask the platform which
    /// chord keys are *actually* held and heal the difference.
    ///
    /// Release-only on purpose. A missing press is harmless (nothing started),
    /// while synthesizing one from a poll would let a chord the user had
    /// already been holding before the hook existed start a dictation nobody
    /// asked for.
    ///
    /// A member the key state has never read down is the one exception: it may
    /// have been swallowed by another program before Windows registered it, in
    /// which case it reads "up" for the whole hold. Healing it on that reading
    /// alone ended every hold at the first tick, and auto-repeat re-engaged it
    /// straight away — a dictation that flickered on and off four times a
    /// second. Such a member is released only once the hook has also gone
    /// quiet on it for [`HELD_EVIDENCE`]. `now` is when the poll ran.
    pub fn resync_released(
        &mut self,
        mut physically_down: impl FnMut(PttKeyCode) -> bool,
        now: Instant,
    ) -> Option<PttEvent> {
        if !self.engaged {
            return None;
        }
        let mut released = false;
        for (idx, key) in self.chord.keys.iter().enumerate() {
            // A swallowed key-down never reaches the system, so this member
            // reads "up" for the whole hold. Polling it would fire a bogus
            // UpMissed at the first tick and paste a quarter second of audio
            // at the cursor on every dictation. `swallowable_lock` guarantees
            // a non-lock member remains, so the watchdog still has an anchor.
            if Some(idx) == self.lock || !self.member_down[idx] {
                continue;
            }
            if physically_down(*key) {
                self.confirmed[idx] = true;
                continue;
            }
            let hook_says_held = self.last_down[idx]
                .is_some_and(|at| now.saturating_duration_since(at) < HELD_EVIDENCE);
            if !self.confirmed[idx] && hook_says_held {
                continue;
            }
            self.member_down[idx] = false;
            self.confirmed[idx] = false;
            self.last_down[idx] = None;
            released = true;
        }
        if !released {
            return None;
        }
        // Engaged means every member was down, so any member now up breaks
        // the chord — the same "first release wins" rule `on_event` applies.
        self.engaged = false;
        Some(PttEvent::UpMissed)
    }

    /// An auto-repeat just proved this member is held. If the key state
    /// disagrees, the press never reached Windows: a hook later in the chain
    /// swallowed it. Only a repeat can be checked — on the first press the hook
    /// runs before Windows registers the key, so it reads "up" regardless.
    ///
    /// Reads the key state at most until it confirms the member, so a normal
    /// hold pays one read, on its first repeat. Hark's own suppressed lock
    /// member is exempt: it reads "up" because Hark swallowed it.
    fn check_intercepted(
        &mut self,
        idx: usize,
        mut physically_down: impl FnMut(PttKeyCode) -> bool,
    ) -> Option<PttEvent> {
        if Some(idx) == self.lock || self.confirmed[idx] || self.intercepted[idx] {
            return None;
        }
        let key = self.chord.keys[idx];
        if physically_down(key) {
            self.confirmed[idx] = true;
            return None;
        }
        self.intercepted[idx] = true;
        Some(PttEvent::Intercepted(key))
    }
}

#[cfg(test)]
mod tests;
