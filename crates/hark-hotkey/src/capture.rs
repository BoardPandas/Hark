//! "Record a shortcut" support: raw key edges and the buffer that turns them
//! into a chord. Split from `edges.rs` because the two answer different
//! questions — `edges.rs` decides when a *configured* chord engages, this
//! decides what the user just pressed.

use crate::edges::PttChord;
use crate::keycode::{PttKeyCode, ALL_KEYS};

/// A raw key edge streamed while the settings UI is recording a shortcut.
/// Unlike `PttEvent` (already-resolved chord engage/disengage), this is one
/// per chord-capable key press or release, fed to a [`CaptureBuffer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureEvent {
    pub key: PttKeyCode,
    pub down: bool,
}

/// Turns periodic snapshots of real key state into the same [`CaptureEvent`]s
/// the hook emits, so a recorder fed by both cannot be defeated by an edge that
/// never arrives.
///
/// This exists because the hook is not trustworthy as the *only* source. On the
/// hardware this was found on, a shortcut recorded as a bare "Left Shift" while
/// the user held a combination — the shape a missing key-down produces — and no
/// documented Win32 mechanism explains a healthy low-level hook skipping events.
/// Rather than keep guessing at the mechanism, the recorder stops depending on
/// being told: it asks the platform what is held. Duplicate edges are free
/// ([`CaptureBuffer`] ignores a repeated press and a release of a key it is not
/// tracking), so the two sources need no merging.
#[derive(Debug, Default)]
pub struct HeldScan {
    /// Bit i: `ALL_KEYS[i]` was down at the last tick. A bitset rather than a
    /// bool array so a tick is a handful of word operations; 114 keys needs two.
    down: [u64; 2],
    /// Bit i: held when recording began. Emits nothing until released once, so
    /// a key the user was already holding can never enter a shortcut.
    stale: [u64; 2],
}

impl HeldScan {
    /// Baseline the scan. Keys already held are recorded as down *and* stale:
    /// the platform reports absolute physical state, including keys pressed
    /// before Hark started, so without this the first tick would invent presses
    /// the user never made during this recording.
    pub fn new(mut is_down: impl FnMut(PttKeyCode) -> bool) -> HeldScan {
        let mut down = [0u64; 2];
        for (i, key) in ALL_KEYS.iter().enumerate() {
            if is_down(*key) {
                down[i / 64] |= 1 << (i % 64);
            }
        }
        HeldScan { down, stale: down }
    }

    /// One snapshot, diffed against the last. A rising edge emits a press; a
    /// falling edge emits a release, unless the key was stale — in which case
    /// the staleness is cleared and nothing is emitted, and the key behaves
    /// normally from then on.
    pub fn tick(
        &mut self,
        mut is_down: impl FnMut(PttKeyCode) -> bool,
        mut emit: impl FnMut(CaptureEvent),
    ) {
        for (i, key) in ALL_KEYS.iter().enumerate() {
            let (word, bit) = (i / 64, 1u64 << (i % 64));
            let now = is_down(*key);
            let was = self.down[word] & bit != 0;
            if now == was {
                continue;
            }
            if now {
                self.down[word] |= bit;
                emit(CaptureEvent {
                    key: *key,
                    down: true,
                });
            } else {
                self.down[word] &= !bit;
                if self.stale[word] & bit != 0 {
                    self.stale[word] &= !bit;
                } else {
                    emit(CaptureEvent {
                        key: *key,
                        down: false,
                    });
                }
            }
        }
    }
}

/// Why a recorded combination was not accepted as a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// A lone Ctrl/Shift/Alt/Win. Binding push-to-talk to a bare modifier means
    /// every press of it in every app opens the microphone, and it is uniquely
    /// miserable to undo because the fix has to be made in the app it broke.
    LoneModifier(PttKeyCode),
    /// A key that writes or deletes text, with nothing but Shift to lift it out
    /// of ordinary typing.
    TypesText { key: PttKeyCode, with_shift: bool },
    /// A key that moves the caret or the view, likewise.
    MovesTheCaret { key: PttKeyCode, with_shift: bool },
    /// Ctrl+V — the chord Hark itself synthesizes to paste a transcript.
    CollidesWithInjection,
}

impl Rejected {
    pub fn message(&self) -> String {
        match self {
            Rejected::LoneModifier(key) => format!(
                "{} on its own would start a dictation every time you press it. \
                 Hold it together with another key, or pick an F-key.",
                key.label()
            ),
            Rejected::TypesText {
                key,
                with_shift: false,
            } => format!(
                "{} types text, and Hark never swallows keys — this would start a \
                 dictation in the middle of whatever you were writing. Hold Ctrl, Alt \
                 or Win with it, or pick an F-key.",
                key.label()
            ),
            Rejected::TypesText {
                key,
                with_shift: true,
            } => format!(
                "Shift + {} is just what you get when you type it, so you would start a \
                 dictation every time. Shift doesn't count here — you hold it while you \
                 write. Hold Ctrl, Alt or Win instead, or pick an F-key.",
                key.label()
            ),
            Rejected::MovesTheCaret {
                key,
                with_shift: false,
            } => format!(
                "{} moves the cursor. You would start a dictation every time you moved \
                 through your own text. Hold Ctrl, Alt or Win with it, or pick an F-key.",
                key.label()
            ),
            Rejected::MovesTheCaret {
                key,
                with_shift: true,
            } => format!(
                "Shift + {} selects text. Shift doesn't count here — you hold it while \
                 you write. Hold Ctrl, Alt or Win instead, or pick an F-key.",
                key.label()
            ),
            Rejected::CollidesWithInjection => "Ctrl+V is how Hark pastes your \
                 transcript, so a shortcut built on it would fight its own typing. \
                 Pick another key."
                .to_string(),
        }
    }
}

/// Builds a chord from a stream of raw capture edges (the "record a shortcut"
/// UX).
///
/// The chord is the PEAK — the largest set held simultaneously — and it commits
/// only once every key has been let go. Committing on the *first* release, the
/// obvious reading of "press the combo and let go", is wrong on real hands: a
/// combination is never pressed or released in one instant, and a user reaching
/// for Ctrl+Shift whose Ctrl comes up a few milliseconds before Shift goes down
/// records a bare "Left Ctrl". They then save it and every Ctrl press in every
/// app starts a dictation. Waiting for all-released costs nothing and cannot
/// truncate a combination.
///
/// Release-to-commit at all, rather than the commit-on-first-non-modifier rule
/// most shortcut recorders use, because push-to-talk chords are usually
/// modifier-only (Discord, OBS and every other PTT app work the same way): a
/// recorder that waited for a "real" key could never record Ctrl+Win.
#[derive(Debug, Default)]
pub struct CaptureBuffer {
    /// Every key seen down since the last commit: the peak, in press order.
    held: Vec<PttKeyCode>,
    /// The subset still physically down. The chord commits when this empties.
    down: Vec<PttKeyCode>,
    saw_any: bool,
    rejected: Option<Rejected>,
}

impl CaptureBuffer {
    pub fn new() -> CaptureBuffer {
        CaptureBuffer::default()
    }

    /// Feed one raw edge. Returns `Some(chord)` once the user has pressed a
    /// combination and let go of all of it. Duplicates (key auto-repeat) and a
    /// 5th simultaneous key are ignored, so the result is always 1..=4 distinct
    /// keys. A combination that is not usable as a shortcut resets the buffer
    /// and leaves a reason in [`Self::rejected`] instead of committing.
    pub fn on_event(&mut self, key: PttKeyCode, down: bool) -> Option<PttChord> {
        if down {
            self.saw_any = true;
            self.rejected = None;
            if !self.held.contains(&key) && self.held.len() < 4 {
                self.held.push(key);
            }
            // Tracked even when `held` is full: a 5th key gated out of BOTH sets
            // would let the commit fire while it is still physically down, and
            // its later release would arrive as a stray that can manufacture a
            // lone-modifier refusal out of a combination the user pressed fine.
            // Auto-repeat re-sends the press, so it must appear here once.
            if !self.down.contains(&key) {
                self.down.push(key);
            }
            return None;
        }

        self.down.retain(|k| *k != key);
        // Still mid-combination, or a stray release of a key that was already
        // held when recording began (which must not commit anything).
        if !self.saw_any || !self.down.is_empty() || self.held.is_empty() {
            return None;
        }

        let chord = PttChord::from_keys(std::mem::take(&mut self.held));
        self.saw_any = false;
        if let Some(why) = chord.rejection() {
            self.rejected = Some(why);
            return None;
        }
        Some(chord)
    }

    /// Keys held so far, for a live "Left Ctrl + F12" display while recording.
    pub fn held(&self) -> &[PttKeyCode] {
        &self.held
    }

    /// Why the last completed combination was turned down, if it was.
    pub fn rejected(&self) -> Option<Rejected> {
        self.rejected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> PttChord {
        PttChord::parse(text).unwrap()
    }

    /// Same as `chord`, named apart where a local binding shadows it.
    fn chord_of(text: &str) -> PttChord {
        chord(text)
    }

    #[test]
    fn capture_commits_the_peak_once_everything_is_released() {
        let mut b = CaptureBuffer::new();
        assert_eq!(b.on_event(PttKeyCode::LCtrl, true), None);
        assert_eq!(b.on_event(PttKeyCode::LWin, true), None);
        assert_eq!(b.held(), &[PttKeyCode::LCtrl, PttKeyCode::LWin]);
        // The first release must NOT commit: hands do not let go in one instant.
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        assert_eq!(
            b.on_event(PttKeyCode::LWin, false),
            Some(chord("LCtrl+LWin"))
        );
    }

    /// The bug this exists for: Ctrl released a moment before Shift goes down
    /// used to record a bare "LCtrl", which the user then saved -- and every
    /// Ctrl press in every app started a dictation.
    #[test]
    fn a_staggered_press_still_records_the_whole_combination() {
        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::LCtrl, true);
        b.on_event(PttKeyCode::LShift, true);
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        assert_eq!(
            b.on_event(PttKeyCode::LShift, false),
            Some(chord("LCtrl+LShift"))
        );
    }

    #[test]
    fn a_lone_modifier_is_turned_down_with_a_reason() {
        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::LCtrl, true);
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        assert_eq!(
            b.rejected(),
            Some(Rejected::LoneModifier(PttKeyCode::LCtrl))
        );
        assert!(b.rejected().unwrap().message().contains("Left Ctrl"));
        // The buffer is clean afterwards, so the next attempt starts fresh.
        assert!(b.held().is_empty());
        b.on_event(PttKeyCode::LCtrl, true);
        b.on_event(PttKeyCode::F12, true);
        b.on_event(PttKeyCode::LCtrl, false);
        assert_eq!(b.on_event(PttKeyCode::F12, false), Some(chord("LCtrl+F12")));
    }

    #[test]
    fn capture_records_a_single_key_chord_when_it_is_not_a_modifier() {
        let mut b = CaptureBuffer::new();
        assert_eq!(b.on_event(PttKeyCode::F13, true), None);
        assert_eq!(b.on_event(PttKeyCode::F13, false), Some(chord("F13")));

        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::CapsLock, true);
        assert_eq!(
            b.on_event(PttKeyCode::CapsLock, false),
            Some(chord("CapsLock"))
        );
    }

    #[test]
    fn capture_dedupes_auto_repeat_and_caps_at_four() {
        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::LCtrl, true);
        // Windows repeats WM_KEYDOWN while held: no duplicate members, and the
        // repeat must not leave a phantom entry that blocks the commit.
        b.on_event(PttKeyCode::LCtrl, true);
        b.on_event(PttKeyCode::LShift, true);
        b.on_event(PttKeyCode::LAlt, true);
        b.on_event(PttKeyCode::LWin, true);
        // A fifth simultaneous key is ignored (chords support at most four).
        b.on_event(PttKeyCode::RCtrl, true);
        assert_eq!(
            b.held(),
            &[
                PttKeyCode::LCtrl,
                PttKeyCode::LShift,
                PttKeyCode::LAlt,
                PttKeyCode::LWin
            ]
        );
        for key in [
            PttKeyCode::LCtrl,
            PttKeyCode::LShift,
            PttKeyCode::LAlt,
            PttKeyCode::RCtrl,
        ] {
            assert_eq!(b.on_event(key, false), None);
        }
        assert_eq!(
            b.on_event(PttKeyCode::LWin, false),
            Some(chord("LCtrl+LShift+LAlt+LWin"))
        );
    }

    /// Feed an interleaved stream of hook edges and scanner edges, as the union
    /// really arrives, and return whatever the buffer commits.
    fn feed(edges: &[(PttKeyCode, bool)]) -> (Option<PttChord>, Option<Rejected>) {
        let mut b = CaptureBuffer::new();
        let mut got = None;
        for (key, down) in edges {
            if let Some(chord) = b.on_event(*key, *down) {
                got = Some(chord);
            }
        }
        (got, b.rejected())
    }

    /// THE observed bug. The hook delivered Shift's press and both releases but
    /// not Ctrl's press, so the buffer committed a bare "Left Shift" out of a
    /// two-key combination and refused it — the exact screenshot. A scanner tick
    /// in between sees Ctrl genuinely down and supplies the missing press.
    #[test]
    fn a_lost_down_edge_is_healed_by_the_scanner() {
        // What the hook alone delivered: LCtrl's press never arrived.
        let (chord, rejected) = feed(&[
            (PttKeyCode::LShift, true),
            (PttKeyCode::LCtrl, false),
            (PttKeyCode::LShift, false),
        ]);
        assert_eq!(chord, None);
        assert_eq!(rejected, Some(Rejected::LoneModifier(PttKeyCode::LShift)));

        // With the scanner supplying LCtrl's press, the whole chord survives.
        let (chord, rejected) = feed(&[
            (PttKeyCode::LShift, true),
            (PttKeyCode::LCtrl, true), // from HeldScan, not the hook
            (PttKeyCode::LCtrl, false),
            (PttKeyCode::LShift, false),
        ]);
        assert_eq!(chord, Some(chord_of("LShift+LCtrl")));
        assert_eq!(rejected, None);
    }

    /// A lost release used to wedge the recorder forever: `down` never emptied,
    /// so nothing committed, nothing timed out, and Cancel was the only way out.
    #[test]
    fn a_lost_up_edge_does_not_wedge_the_recorder() {
        let (chord, _) = feed(&[
            (PttKeyCode::LCtrl, true),
            (PttKeyCode::F12, true),
            (PttKeyCode::F12, false),
            // LCtrl's release never arrived from the hook; the scanner sees the
            // key is no longer down and emits it.
            (PttKeyCode::LCtrl, false),
        ]);
        assert_eq!(chord, Some(chord_of("LCtrl+F12")));
    }

    /// Both sources report the same key. Duplicates must be free — that is what
    /// lets the union skip a merge layer entirely.
    #[test]
    fn duplicate_edges_from_both_sources_commit_once() {
        let (chord, rejected) = feed(&[
            (PttKeyCode::LCtrl, true),
            (PttKeyCode::LCtrl, true), // scanner agrees
            (PttKeyCode::LWin, true),
            (PttKeyCode::LWin, true),
            (PttKeyCode::LCtrl, false),
            (PttKeyCode::LCtrl, false), // duplicate release: not tracked, no-op
            (PttKeyCode::LWin, false),
        ]);
        assert_eq!(chord, Some(chord_of("LCtrl+LWin")));
        assert_eq!(rejected, None);
    }

    /// A 5th simultaneous key used to be gated out of BOTH sets, so the commit
    /// fired while it was still physically held and its release came back as a
    /// stray — manufacturing a lone-modifier refusal with no lost edges at all.
    #[test]
    fn a_fifth_key_still_blocks_the_commit() {
        let mut b = CaptureBuffer::new();
        for key in [
            PttKeyCode::LCtrl,
            PttKeyCode::LShift,
            PttKeyCode::LAlt,
            PttKeyCode::LWin,
            PttKeyCode::RCtrl,
        ] {
            assert_eq!(b.on_event(key, true), None);
        }
        for key in [
            PttKeyCode::LCtrl,
            PttKeyCode::LShift,
            PttKeyCode::LAlt,
            PttKeyCode::LWin,
        ] {
            assert_eq!(
                b.on_event(key, false),
                None,
                "committed while {key} was held"
            );
        }
        assert_eq!(
            b.on_event(PttKeyCode::RCtrl, false),
            Some(chord_of("LCtrl+LShift+LAlt+LWin"))
        );
    }

    #[test]
    fn scan_emits_a_down_then_an_up_for_a_press() {
        let mut scan = HeldScan::new(|_| false);
        let mut seen = Vec::new();
        scan.tick(|k| k == PttKeyCode::F9, |e| seen.push(e));
        assert_eq!(
            seen,
            vec![CaptureEvent {
                key: PttKeyCode::F9,
                down: true
            }]
        );
        seen.clear();
        scan.tick(|_| false, |e| seen.push(e));
        assert_eq!(
            seen,
            vec![CaptureEvent {
                key: PttKeyCode::F9,
                down: false
            }]
        );
    }

    /// Baseline suppression, and that it is not permanent: the key the user was
    /// already holding is ignored until released, then behaves normally.
    #[test]
    fn scan_ignores_keys_already_held_when_recording_began() {
        let mut scan = HeldScan::new(|k| k == PttKeyCode::LCtrl);
        let mut seen = Vec::new();
        scan.tick(|k| k == PttKeyCode::LCtrl, |e| seen.push(e));
        assert!(seen.is_empty(), "a still-held stale key emitted {seen:?}");
        scan.tick(|_| false, |e| seen.push(e));
        assert!(seen.is_empty(), "the stale release leaked: {seen:?}");
        // Pressed again during the recording, it is an ordinary key.
        scan.tick(|k| k == PttKeyCode::LCtrl, |e| seen.push(e));
        assert_eq!(
            seen,
            vec![CaptureEvent {
                key: PttKeyCode::LCtrl,
                down: true
            }]
        );
    }

    #[test]
    fn scan_emits_nothing_when_nothing_changes() {
        let mut scan = HeldScan::new(|_| false);
        for _ in 0..5 {
            scan.tick(|_| false, |e| panic!("idle tick emitted {e:?}"));
        }
        let mut scan = HeldScan::new(|k| k == PttKeyCode::LWin);
        for _ in 0..5 {
            scan.tick(
                |k| k == PttKeyCode::LWin,
                |e| panic!("idle tick emitted {e:?}"),
            );
        }
    }

    #[test]
    fn all_keys_is_complete_and_unique() {
        assert_eq!(ALL_KEYS.len(), PttKeyCode::COUNT);
        // Ordinal order must match declaration order: HeldScan indexes its
        // bitset by position in ALL_KEYS and reads state by the key at that
        // position, so a mismatch would attribute one key's state to another.
        for (i, key) in ALL_KEYS.iter().enumerate() {
            assert_eq!(key.ordinal(), i, "{key} is out of order");
        }
        let mut tokens: Vec<_> = ALL_KEYS.iter().map(|k| k.token().to_lowercase()).collect();
        tokens.sort();
        let unique = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), unique, "two keys share a config token");
        // Every token must survive the round trip config.toml relies on.
        for key in ALL_KEYS {
            assert_eq!(
                crate::keycode::parse_key(key.token()),
                Some(key),
                "{key} does not round-trip"
            );
            assert!(
                !key.token().contains('+'),
                "{key} token breaks chord splitting"
            );
        }
        assert_eq!(ALL_KEYS.iter().filter(|k| k.is_modifier()).count(), 8);
        // Escape is never bindable: it cancels recording.
        assert_eq!(crate::keycode::parse_key("Esc"), None);
    }

    /// Baseline suppression must not leak a pre-held key into a shortcut: a
    /// lone Ctrl tap is still a lone Ctrl, not "Ctrl + the Shift you were
    /// already holding".
    #[test]
    fn a_key_held_before_recording_began_cannot_be_recorded() {
        let mut scan = HeldScan::new(|k| k == PttKeyCode::LShift);
        let mut b = CaptureBuffer::new();
        let mut committed = None;
        let mut feed = |e: CaptureEvent| {
            if let Some(c) = b.on_event(e.key, e.down) {
                committed = Some(c);
            }
        };
        // Ctrl tapped while Shift stays held from before.
        scan.tick(
            |k| k == PttKeyCode::LShift || k == PttKeyCode::LCtrl,
            &mut feed,
        );
        scan.tick(|k| k == PttKeyCode::LShift, &mut feed);
        assert_eq!(committed, None);
        assert_eq!(
            b.rejected(),
            Some(Rejected::LoneModifier(PttKeyCode::LCtrl))
        );
    }

    #[test]
    fn rejected_is_cleared_when_a_new_attempt_starts() {
        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::LCtrl, true);
        b.on_event(PttKeyCode::LCtrl, false);
        assert!(b.rejected().is_some());
        // The next press is a new attempt; the stale refusal must not describe it.
        b.on_event(PttKeyCode::F5, true);
        assert_eq!(b.rejected(), None);
    }

    #[test]
    fn capture_ignores_releases_of_keys_held_before_recording_began() {
        let mut b = CaptureBuffer::new();
        // Clicking "Change" with Ctrl still down: its release is not a chord.
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        assert_eq!(b.on_event(PttKeyCode::LShift, false), None);
        assert_eq!(b.rejected(), None);
        // Recording then proceeds normally.
        assert_eq!(b.on_event(PttKeyCode::F9, true), None);
        assert_eq!(b.on_event(PttKeyCode::F9, false), Some(chord("F9")));
    }
}
