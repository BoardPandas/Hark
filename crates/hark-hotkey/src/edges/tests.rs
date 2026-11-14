use super::*;

pub(super) fn chord(text: &str) -> PttChord {
    PttChord::parse(text).unwrap()
}

#[test]
fn single_key_chord_edges() {
    let mut t = ChordTracker::new(chord("RCtrl"));
    assert_eq!(
        t.on_event(PttKeyCode::RCtrl, true, false),
        Some(PttEvent::Down)
    );
    assert_eq!(
        t.on_event(PttKeyCode::RCtrl, false, false),
        Some(PttEvent::Up)
    );
}

#[test]
fn chord_engages_on_last_member_down_releases_on_first_up() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    // First member down: nothing yet.
    assert_eq!(t.on_event(PttKeyCode::LCtrl, true, false), None);
    // Second member down: chord engaged.
    assert_eq!(
        t.on_event(PttKeyCode::LWin, true, false),
        Some(PttEvent::Down)
    );
    // First release disengages...
    assert_eq!(
        t.on_event(PttKeyCode::LCtrl, false, false),
        Some(PttEvent::Up)
    );
    // ...and the second release is silent.
    assert_eq!(t.on_event(PttKeyCode::LWin, false, false), None);
}

#[test]
fn order_of_presses_does_not_matter() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    assert_eq!(t.on_event(PttKeyCode::LWin, true, false), None);
    assert_eq!(
        t.on_event(PttKeyCode::LCtrl, true, false),
        Some(PttEvent::Down)
    );
}

#[test]
fn auto_repeat_is_suppressed() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    t.on_event(PttKeyCode::LCtrl, true, false);
    t.on_event(PttKeyCode::LWin, true, false);
    // Holding keys makes Windows repeat WM_KEYDOWN for them.
    assert_eq!(t.on_event(PttKeyCode::LCtrl, true, false), None);
    assert_eq!(t.on_event(PttKeyCode::LWin, true, false), None);
    assert_eq!(t.on_event(PttKeyCode::LWin, true, false), None);
    // The chord is still engaged: release works normally.
    assert_eq!(
        t.on_event(PttKeyCode::LWin, false, false),
        Some(PttEvent::Up)
    );
}

#[test]
fn injected_events_are_ignored() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    t.on_event(PttKeyCode::LWin, true, false);
    // An injected LCtrl press (e.g. enigo synthesizing Ctrl+V while the
    // user still holds LWin) must NOT complete the chord.
    assert_eq!(t.on_event(PttKeyCode::LCtrl, true, true), None);
    // Nor may an injected release tear it down once engaged.
    assert_eq!(
        t.on_event(PttKeyCode::LCtrl, true, false),
        Some(PttEvent::Down)
    );
    assert_eq!(t.on_event(PttKeyCode::LCtrl, false, true), None);
    assert_eq!(
        t.on_event(PttKeyCode::LCtrl, false, false),
        Some(PttEvent::Up)
    );
}

#[test]
fn non_chord_keys_are_ignored() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    t.on_event(PttKeyCode::LCtrl, true, false);
    assert_eq!(t.on_event(PttKeyCode::RShift, true, false), None);
    assert_eq!(t.on_event(PttKeyCode::F5, true, false), None);
    // Chord still completes normally afterwards.
    assert_eq!(
        t.on_event(PttKeyCode::LWin, true, false),
        Some(PttEvent::Down)
    );
}

#[test]
fn resync_emits_the_release_the_hook_never_delivered() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    t.on_event(PttKeyCode::LCtrl, true, false);
    assert_eq!(
        t.on_event(PttKeyCode::LWin, true, false),
        Some(PttEvent::Down)
    );
    // The watchdog's first tick during the hold sees both keys held...
    assert_eq!(t.resync_released(|_| true, Instant::now()), None);
    // ...then the user let go on the lock screen, so no release arrived.
    assert_eq!(
        t.resync_released(|_| false, Instant::now()),
        Some(PttEvent::UpMissed)
    );
    // Once healed the tracker is idle again: a second poll adds nothing,
    // and the next press engages normally rather than being swallowed as
    // a duplicate.
    assert_eq!(t.resync_released(|_| false, Instant::now()), None);
    assert_eq!(t.on_event(PttKeyCode::LCtrl, true, false), None);
    assert_eq!(
        t.on_event(PttKeyCode::LWin, true, false),
        Some(PttEvent::Down)
    );
}

#[test]
fn resync_leaves_a_chord_that_is_really_held_alone() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    t.on_event(PttKeyCode::LCtrl, true, false);
    t.on_event(PttKeyCode::LWin, true, false);
    // Polled mid-hold, every 250 ms, for as long as the user talks.
    for _ in 0..8 {
        assert_eq!(t.resync_released(|_| true, Instant::now()), None);
    }
    // The real release still produces the normal edge.
    assert_eq!(
        t.on_event(PttKeyCode::LWin, false, false),
        Some(PttEvent::Up)
    );
}

#[test]
fn resync_never_starts_a_dictation() {
    // Keys held before the chord was ever tracked (or a press whose event
    // went missing) must not engage from a poll alone.
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    assert_eq!(t.resync_released(|_| true, Instant::now()), None);
    assert_eq!(t.resync_released(|_| false, Instant::now()), None);
    // One member down is not the chord either.
    t.on_event(PttKeyCode::LCtrl, true, false);
    assert_eq!(t.resync_released(|_| true, Instant::now()), None);
}

#[test]
fn resync_after_a_normal_release_is_silent() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    t.on_event(PttKeyCode::LCtrl, true, false);
    t.on_event(PttKeyCode::LWin, true, false);
    assert_eq!(
        t.on_event(PttKeyCode::LCtrl, false, false),
        Some(PttEvent::Up)
    );
    // The other member is still physically down; no second edge.
    assert_eq!(
        t.resync_released(|k| k == PttKeyCode::LWin, Instant::now()),
        None
    );
}

/// The bug this guards: a release the hook never saw, with the chord
/// already disengaged, leaves that member stuck `true`. Nothing polls in
/// that state (the watchdog runs only during a hold), so the chord silently
/// becomes a one-key hotkey — press Left Ctrl alone and a dictation starts.
#[test]
fn a_member_whose_release_was_missed_cannot_engage_the_chord_alone() {
    let mut t = ChordTracker::new(chord("LCtrl+F12"));
    t.on_event(PttKeyCode::LCtrl, true, false);
    assert_eq!(
        t.on_event(PttKeyCode::F12, true, false),
        Some(PttEvent::Down)
    );
    // LCtrl's release arrives; F12's never does (an Fn-layer keyboard
    // reporting the break under a different key, a lock screen, a sleep).
    assert_eq!(
        t.on_event(PttKeyCode::LCtrl, false, false),
        Some(PttEvent::Up)
    );

    // Later, with nothing physically held, Left Ctrl alone must do nothing.
    assert_eq!(
        t.on_event_verified(PttKeyCode::LCtrl, true, false, |_| false, Instant::now()),
        None
    );
    // ...and the chord still works normally afterwards: the stale member
    // was cleared, not left to poison every future press.
    assert_eq!(
        t.on_event_verified(PttKeyCode::F12, true, false, |_| true, Instant::now()),
        Some(PttEvent::Down)
    );
}

#[test]
fn a_genuinely_held_member_still_engages() {
    let mut t = ChordTracker::new(chord("LCtrl+F12"));
    // The user really is holding LCtrl when F12 goes down. The key in the
    // event itself is never polled — a low-level hook runs before the
    // platform's key state catches up, so F12 may still read as up here.
    t.on_event_verified(PttKeyCode::LCtrl, true, false, |_| false, Instant::now());
    assert_eq!(
        t.on_event_verified(
            PttKeyCode::F12,
            true,
            false,
            |k| k == PttKeyCode::LCtrl,
            Instant::now()
        ),
        Some(PttEvent::Down)
    );
}

/// The poll may clear members, never set them: a chord the user was already
/// holding before Hark started must not launch a dictation on its own.
#[test]
fn verification_never_manufactures_an_engage() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    assert_eq!(
        t.on_event_verified(PttKeyCode::LCtrl, true, false, |_| true, Instant::now()),
        None
    );
}

fn engaged_tracker(text: &str) -> ChordTracker {
    let mut t = ChordTracker::with_lock_suppression(chord(text), true);
    let keys: Vec<_> = chord(text).keys().to_vec();
    for key in &keys {
        t.on_event(*key, true, false);
    }
    assert!(t.engaged, "{text} should be engaged");
    t
}

/// THE catastrophic case. A lone Caps Lock chord has no "other members",
/// so any rule phrased as "all the others are held" is vacuously true and
/// would swallow Caps Lock everywhere, forever, with no way back.
#[test]
fn a_lone_lock_key_chord_is_never_swallowed() {
    for text in ["CapsLock", "ScrollLock"] {
        let mut t = ChordTracker::with_lock_suppression(chord(text), true);
        let key = chord(text).keys()[0];
        assert_eq!(t.on_event(key, true, false), Some(PttEvent::Down));
        assert!(
            !t.swallow(key, true, false),
            "{text} alone must keep working as a lock key"
        );
    }
}

#[test]
fn a_lock_key_in_a_chord_is_swallowed_on_the_press_that_engages() {
    let t = engaged_tracker("LCtrl+ScrollLock");
    assert!(t.swallow(PttKeyCode::ScrollLock, true, false));
    // ...but only the lock member, only key-down, and never injected.
    assert!(!t.swallow(PttKeyCode::LCtrl, true, false));
    assert!(!t.swallow(PttKeyCode::ScrollLock, false, false));
    assert!(!t.swallow(PttKeyCode::ScrollLock, true, true));
}

/// Nothing is swallowed unless a dictation is actually running. That is the
/// invariant that makes a second return path in the hook defensible: every
/// swallowed keystroke has a visible dictation attached to it.
#[test]
fn nothing_is_swallowed_while_idle() {
    let mut t = ChordTracker::with_lock_suppression(chord("LCtrl+ScrollLock"), true);
    assert!(!t.swallow(PttKeyCode::ScrollLock, true, false));
    // Lock key first, chord not yet complete: still not engaged.
    t.on_event(PttKeyCode::ScrollLock, true, false);
    assert!(!t.swallow(PttKeyCode::ScrollLock, true, false));
}

#[test]
fn alt_or_win_in_the_chord_disables_suppression() {
    // A swallowed press is invisible to Windows, so it cannot mark Alt or
    // Win "used in a chord" -- the menu would pop on every dictation.
    for text in ["LCtrl+LAlt+ScrollLock", "LWin+CapsLock", "RAlt+CapsLock"] {
        let t = engaged_tracker(text);
        let lock = chord(text)
            .keys()
            .iter()
            .copied()
            .find(|k| k.is_suppressible_lock())
            .unwrap();
        assert!(!t.swallow(lock, true, false), "{text} must not swallow");
    }
}

#[test]
fn num_lock_is_never_swallowed() {
    // Windows applies Num Lock's toggle above the hook, so swallowing
    // would eat the keystroke and flip the lock anyway.
    let t = engaged_tracker("LCtrl+NumLock");
    assert!(!t.swallow(PttKeyCode::NumLock, true, false));
}

#[test]
fn two_lock_keys_disable_suppression() {
    let t = engaged_tracker("CapsLock+ScrollLock");
    assert!(!t.swallow(PttKeyCode::CapsLock, true, false));
    assert!(!t.swallow(PttKeyCode::ScrollLock, true, false));
}

#[test]
fn suppression_is_off_unless_the_setting_asks_for_it() {
    let mut t = ChordTracker::with_lock_suppression(chord("LCtrl+ScrollLock"), false);
    t.on_event(PttKeyCode::LCtrl, true, false);
    t.on_event(PttKeyCode::ScrollLock, true, false);
    assert!(!t.swallow(PttKeyCode::ScrollLock, true, false));
    // ...and ChordTracker::new is the observe-only constructor.
    let plain = ChordTracker::new(chord("LCtrl+ScrollLock"));
    assert!(!plain.swallow(PttKeyCode::ScrollLock, true, false));
}

/// A swallowed press never reaches the system, so the watchdog would read
/// that member as "up" for the whole hold, emit UpMissed at the first
/// 250 ms tick, and paste a quarter second of audio at the cursor on every
/// single dictation. The suppressed member must be exempt from the poll.
#[test]
fn the_watchdog_ignores_the_suppressed_member() {
    let mut t = engaged_tracker("LCtrl+ScrollLock");
    // ScrollLock reads up (it was swallowed); LCtrl is genuinely held.
    assert_eq!(
        t.resync_released(|k| k == PttKeyCode::LCtrl, Instant::now()),
        None,
        "a swallowed member must not look like a lost release"
    );
    // A real release of the member it CAN read still heals normally.
    assert_eq!(
        t.resync_released(|_| false, Instant::now()),
        Some(PttEvent::UpMissed)
    );
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Windows never registers F12: a key remapper's hook, later in the chain
/// than Hark's, swallows every press. Hark's hook still sees the key.
fn f12_intercepted(key: PttKeyCode) -> bool {
    key != PttKeyCode::F12
}

/// `LAlt+F12` engaged at `t0 + 80 ms`, the way HeikaBlade held it: Alt
/// first, so F12 is the key that engages and the key that auto-repeats.
fn engaged_with_f12_intercepted(t0: Instant) -> ChordTracker {
    let mut t = ChordTracker::new(chord("LAlt+F12"));
    let (alt, f12) = (PttKeyCode::LAlt, PttKeyCode::F12);
    assert_eq!(
        t.on_event_verified(alt, true, false, f12_intercepted, t0),
        None
    );
    assert_eq!(
        t.on_event_verified(f12, true, false, f12_intercepted, t0 + ms(80)),
        Some(PttEvent::Down)
    );
    t
}

#[test]
fn a_key_another_program_swallows_is_reported_on_its_first_repeat() {
    let t0 = Instant::now();
    let mut t = engaged_with_f12_intercepted(t0);
    // A released key cannot auto-repeat, so the repeat is the proof.
    assert_eq!(
        t.on_event_verified(PttKeyCode::F12, true, false, f12_intercepted, t0 + ms(580)),
        Some(PttEvent::Intercepted(PttKeyCode::F12))
    );
    // Once. The hold is ~30 repeats a second.
    assert_eq!(
        t.on_event_verified(PttKeyCode::F12, true, false, f12_intercepted, t0 + ms(613)),
        None
    );
}

/// The flicker: every 250 ms tick read F12 as up and ended the hold, and
/// the next auto-repeat started a new one. 91 of them in 76 presses.
#[test]
fn an_intercepted_key_does_not_end_the_hold_while_it_keeps_repeating() {
    let t0 = Instant::now();
    let mut t = engaged_with_f12_intercepted(t0);
    for tick in 1..=16u64 {
        let now = t0 + ms(80 + 250 * tick);
        // Auto-repeat starts after the keyboard delay (500 ms here).
        if now >= t0 + ms(580) {
            t.on_event_verified(PttKeyCode::F12, true, false, f12_intercepted, now - ms(20));
        }
        assert_eq!(
            t.resync_released(f12_intercepted, now),
            None,
            "tick {tick} ended a hold that was still repeating"
        );
    }
    // The real release still ends it, through the hook as always.
    assert_eq!(
        t.on_event_verified(
            PttKeyCode::F12,
            false,
            false,
            f12_intercepted,
            t0 + ms(4_200)
        ),
        Some(PttEvent::Up)
    );
}

/// The tolerance must not become a wedge: once the hook stops vouching for
/// the key, a poll that never saw it held releases it like any other.
#[test]
fn an_intercepted_key_that_stops_repeating_is_still_released() {
    let t0 = Instant::now();
    let mut t = engaged_with_f12_intercepted(t0);
    assert_eq!(t.resync_released(f12_intercepted, t0 + ms(330)), None);
    assert_eq!(
        t.resync_released(f12_intercepted, t0 + ms(80) + HELD_EVIDENCE),
        Some(PttEvent::UpMissed)
    );
}

#[test]
fn a_normal_hold_is_never_reported_and_pays_one_read() {
    let t0 = Instant::now();
    let reads = std::cell::Cell::new(0);
    let held = |_| {
        reads.set(reads.get() + 1);
        true
    };
    let mut t = ChordTracker::new(chord("LAlt+F12"));
    t.on_event_verified(PttKeyCode::LAlt, true, false, held, t0);
    t.on_event_verified(PttKeyCode::F12, true, false, held, t0 + ms(80));
    let before = reads.get();
    for n in 0..30u64 {
        let at = t0 + ms(580 + 33 * n);
        assert_eq!(
            t.on_event_verified(PttKeyCode::F12, true, false, held, at),
            None
        );
    }
    assert_eq!(
        reads.get() - before,
        1,
        "only the first repeat reads key state"
    );
}

#[test]
fn harks_own_swallowed_lock_is_not_reported_as_intercepted() {
    let mut t = engaged_tracker("LCtrl+ScrollLock");
    // ScrollLock reads up because Hark itself swallowed the press.
    assert_eq!(
        t.on_event_verified(
            PttKeyCode::ScrollLock,
            true,
            false,
            |k| k == PttKeyCode::LCtrl,
            Instant::now()
        ),
        None
    );
}

#[test]
fn stray_release_without_press_is_silent() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    assert_eq!(t.on_event(PttKeyCode::LCtrl, false, false), None);
    assert_eq!(t.on_event(PttKeyCode::LWin, false, false), None);
}

#[test]
fn partial_release_and_repress_re_engages() {
    let mut t = ChordTracker::new(chord("LCtrl+LWin"));
    t.on_event(PttKeyCode::LCtrl, true, false);
    assert_eq!(
        t.on_event(PttKeyCode::LWin, true, false),
        Some(PttEvent::Down)
    );
    assert_eq!(
        t.on_event(PttKeyCode::LWin, false, false),
        Some(PttEvent::Up)
    );
    // LCtrl never left: pressing LWin again re-engages (a quick second
    // dictation without fully letting go).
    assert_eq!(
        t.on_event(PttKeyCode::LWin, true, false),
        Some(PttEvent::Down)
    );
}
