//! Who said each line of a meeting: the speaker-key vocabulary, and the
//! [`Roster`] that turns keys into labels and color slots (spec
//! `intent/meeting-speakers-are-hard-to-tell-apart/spec.md` §2.3).
//!
//! A key is derived from what is stored, never a new identity: channel 0 is
//! always `me`, channel 1 is `them` or `them:<diarized id>`, and a speaker the
//! user creates while moving a line is `added:<n>`. The store keeps keys as
//! text; this module owns the vocabulary, the same way `hark-pipeline` owns
//! `transcript_status`.
//!
//! Every reader (detail view, exports, notes) resolves labels through one
//! `Roster`, so no screen can keep showing a stale name.
//!
//! Pure: no I/O, no clocks. Names are content and are never logged; only
//! counts are.

use std::collections::HashMap;
use std::fmt;

/// The high bit Gemini's final pass sets on a window-encoded speaker id
/// (`hark_stt::meeting_gemini::WINDOW_SPEAKER_BIT`): `window << 10 | speaker`.
const WINDOW_SPEAKER_BIT: u32 = 0x8000_0000;

/// One speaker identity within a meeting. Stored as text (see [`fmt::Display`]
/// and [`SpeakerKey::parse`]). Not content, so `Debug` is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpeakerKey {
    /// Any channel-0 (microphone) line.
    Me,
    /// A channel-1 line with no diarized speaker (a live transcript).
    Them,
    /// Channel 1, diarized speaker `n`: a Deepgram id, or a Gemini
    /// window-encoded id with the high bit set.
    Diarized(u32),
    /// A speaker the user created while reassigning a line.
    Added(u32),
}

impl SpeakerKey {
    /// `None` for anything this build does not know (a newer Hark's key, or
    /// junk), which callers treat as "no override".
    pub fn parse(s: &str) -> Option<SpeakerKey> {
        match s {
            "me" => Some(SpeakerKey::Me),
            "them" => Some(SpeakerKey::Them),
            _ => {
                if let Some(n) = s.strip_prefix("them:") {
                    number(n).map(SpeakerKey::Diarized)
                } else {
                    number(s.strip_prefix("added:")?).map(SpeakerKey::Added)
                }
            }
        }
    }

    /// The key a line was transcribed with: channel 0 is always Me, whatever
    /// speaker it carries.
    pub fn transcribed(channel: u8, speaker: Option<u32>) -> SpeakerKey {
        match (channel, speaker) {
            (0, _) => SpeakerKey::Me,
            (_, None) => SpeakerKey::Them,
            (_, Some(n)) => SpeakerKey::Diarized(n),
        }
    }

    /// The key a line is shown with: its override when one parses, else the
    /// transcribed key.
    pub fn effective(channel: u8, speaker: Option<u32>, override_key: Option<&str>) -> SpeakerKey {
        override_key
            .and_then(SpeakerKey::parse)
            .unwrap_or_else(|| SpeakerKey::transcribed(channel, speaker))
    }
}

/// Plain decimal only: `+3`, `03` and `" 3"` are not keys this module wrote.
fn number(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return None;
    }
    s.parse().ok()
}

impl fmt::Display for SpeakerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpeakerKey::Me => f.write_str("me"),
            SpeakerKey::Them => f.write_str("them"),
            SpeakerKey::Diarized(n) => write!(f, "them:{n}"),
            SpeakerKey::Added(n) => write!(f, "added:{n}"),
        }
    }
}

/// The label a key has before anyone names it: "Me", "Them", "Speaker 2",
/// "Window 3 · Speaker 1" (Gemini), or "Other speaker 1".
pub fn default_label(key: SpeakerKey) -> String {
    match key {
        SpeakerKey::Me => "Me".to_string(),
        SpeakerKey::Them => "Them".to_string(),
        SpeakerKey::Diarized(id) if id & WINDOW_SPEAKER_BIT != 0 => {
            let window = (id & !WINDOW_SPEAKER_BIT) >> 10;
            let speaker = id & 0x3ff;
            format!("Window {} · Speaker {}", window + 1, speaker + 1)
        }
        SpeakerKey::Diarized(id) => format!("Speaker {}", u64::from(id) + 1),
        SpeakerKey::Added(n) => format!("Other speaker {}", u64::from(n) + 1),
    }
}

/// One entry in the reassign menu: everyone sharing a name is one person.
/// Carries a name: no `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct Person {
    /// The earliest key in slot order; the one a line moves to.
    pub key: SpeakerKey,
    /// Every key shown as this person, `key` first.
    pub keys: Vec<SpeakerKey>,
    pub label: String,
    pub slot: usize,
}

struct Entry {
    key: SpeakerKey,
    label: String,
    /// Before merging by name.
    own_slot: usize,
    /// Lines currently shown as this key.
    lines: usize,
}

/// Labels and color slots for one meeting. Build once per view refresh,
/// export or notes render; everything else reads through it.
pub struct Roster {
    /// Slot order: Me first, then first transcribed appearance, then added
    /// keys in key order.
    entries: Vec<Entry>,
    /// Merged slot per key (the earliest slot among keys sharing a label).
    slots: HashMap<SpeakerKey, usize>,
}

impl Roster {
    /// `segments` are `(channel, speaker, speaker_override)` in transcript
    /// order; `names` are stored `(speaker_key, display_name)` rows. Rows and
    /// overrides that do not parse are skipped and counted.
    pub fn new<'a>(
        segments: impl IntoIterator<Item = (u8, Option<u32>, Option<&'a str>)>,
        names: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Roster {
        let mut transcribed: Vec<SpeakerKey> = vec![SpeakerKey::Me];
        let mut extra: Vec<SpeakerKey> = Vec::new();
        let mut lines: HashMap<SpeakerKey, usize> = HashMap::new();
        let mut unknown = 0usize;
        for (channel, speaker, override_key) in segments {
            let own = SpeakerKey::transcribed(channel, speaker);
            if !transcribed.contains(&own) {
                transcribed.push(own);
            }
            let parsed = override_key.and_then(SpeakerKey::parse);
            if override_key.is_some() && parsed.is_none() {
                unknown += 1;
            }
            let effective = parsed.unwrap_or(own);
            if let SpeakerKey::Added(_) = effective {
                if !extra.contains(&effective) {
                    extra.push(effective);
                }
            }
            *lines.entry(effective).or_default() += 1;
        }

        let mut named: HashMap<SpeakerKey, String> = HashMap::new();
        for (key, name) in names {
            match SpeakerKey::parse(key) {
                Some(key) if !name.trim().is_empty() => {
                    if let SpeakerKey::Added(_) = key {
                        if !extra.contains(&key) {
                            extra.push(key);
                        }
                    }
                    named.insert(key, name.trim().to_string());
                }
                _ => unknown += 1,
            }
        }
        if unknown > 0 {
            log::warn!("meeting roster: {unknown} unknown speaker keys ignored");
        }
        // Diarized or Them overrides with no transcribed line of their own
        // (a newer build's data) still get a slot, after the added keys.
        let mut orphans: Vec<SpeakerKey> = lines
            .keys()
            .filter(|k| !transcribed.contains(k) && !extra.contains(k))
            .copied()
            .collect();
        extra.sort_unstable();
        orphans.sort_unstable();

        let entries: Vec<Entry> = transcribed
            .into_iter()
            .chain(extra)
            .chain(orphans)
            .enumerate()
            .map(|(own_slot, key)| Entry {
                key,
                label: named.remove(&key).unwrap_or_else(|| default_label(key)),
                own_slot,
                lines: lines.get(&key).copied().unwrap_or(0),
            })
            .collect();

        let mut first_slot: HashMap<String, usize> = HashMap::new();
        let mut slots = HashMap::new();
        for e in &entries {
            // Me is always slot 0, and no one else ever takes it.
            let slot = if e.key == SpeakerKey::Me {
                0
            } else {
                *first_slot.entry(merge_name(&e.label)).or_insert(e.own_slot)
            };
            if e.key == SpeakerKey::Me {
                first_slot.entry(merge_name(&e.label)).or_insert(0);
            }
            slots.insert(e.key, slot);
        }
        Roster { entries, slots }
    }

    /// The name row if there is one, else the default label.
    pub fn label(&self, key: SpeakerKey) -> String {
        self.entry(key)
            .map_or_else(|| default_label(key), |e| e.label.clone())
    }

    /// The color slot: 0 for Me, the earliest merged slot otherwise. A key
    /// this roster never saw gets the next unused slot, never Me's.
    pub fn slot(&self, key: SpeakerKey) -> usize {
        match key {
            SpeakerKey::Me => 0,
            _ => self.slots.get(&key).copied().unwrap_or(self.entries.len()),
        }
    }

    /// The menu: Me, then one entry per person in slot order. Two keys with
    /// the same label (ignoring case and surrounding spaces) are one person.
    pub fn people(&self) -> Vec<Person> {
        let mut people: Vec<Person> = Vec::new();
        for e in &self.entries {
            let slot = self.slot(e.key);
            match people.iter_mut().find(|p| p.slot == slot) {
                Some(p) => p.keys.push(e.key),
                None => people.push(Person {
                    key: e.key,
                    keys: vec![e.key],
                    label: e.label.clone(),
                    slot,
                }),
            }
        }
        people.sort_by_key(|p| p.slot);
        people
    }

    /// Keys with at least one line shown as them, in slot order: the
    /// speakers card, one chip per key so a merged key can be renamed apart.
    pub fn speaking(&self) -> Vec<SpeakerKey> {
        self.entries
            .iter()
            .filter(|e| e.lines > 0)
            .map(|e| e.key)
            .collect()
    }

    /// Another key already shown with `name`, for the "same person" hint.
    pub fn named(&self, name: &str, except: SpeakerKey) -> Option<SpeakerKey> {
        let wanted = merge_name(name);
        self.entries
            .iter()
            .find(|e| e.key != except && merge_name(&e.label) == wanted)
            .map(|e| e.key)
    }

    /// The next free `added:<n>`.
    pub fn next_added(&self) -> SpeakerKey {
        let next = self
            .entries
            .iter()
            .filter_map(|e| match e.key {
                SpeakerKey::Added(n) => Some(n.saturating_add(1)),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        SpeakerKey::Added(next)
    }

    /// `(key text, default label, display label)` for every key, the table
    /// notes are rendered with (`hark_voice::MeetingNotes::render`).
    pub fn notes_names(&self) -> Vec<(String, String, String)> {
        self.entries
            .iter()
            .map(|e| (e.key.to_string(), default_label(e.key), e.label.clone()))
            .collect()
    }

    fn entry(&self, key: SpeakerKey) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }
}

/// Merge-by-name compares case-insensitively after trimming, and nothing
/// else: "Sam" and "Samuel" stay two people (spec A7).
fn merge_name(name: &str) -> String {
    name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEMINI: u32 = WINDOW_SPEAKER_BIT | (2 << 10) | 1;

    #[test]
    fn keys_round_trip_through_their_stored_text() {
        for key in [
            SpeakerKey::Me,
            SpeakerKey::Them,
            SpeakerKey::Diarized(0),
            SpeakerKey::Diarized(7),
            SpeakerKey::Diarized(GEMINI),
            SpeakerKey::Diarized(u32::MAX),
            SpeakerKey::Added(0),
            SpeakerKey::Added(12),
        ] {
            assert_eq!(SpeakerKey::parse(&key.to_string()), Some(key));
        }
        assert_eq!(SpeakerKey::Diarized(GEMINI).to_string(), "them:2147485697");
        assert_eq!(SpeakerKey::Added(3).to_string(), "added:3");
    }

    #[test]
    fn unknown_key_text_parses_to_none() {
        for junk in [
            "",
            "Me",
            "ME",
            "me ",
            "them:",
            "them:x",
            "them:-1",
            "them:+1",
            "them:01",
            "them:4294967296",
            "added:",
            "added:1.5",
            "guest:1",
            "speaker:2",
        ] {
            assert_eq!(SpeakerKey::parse(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn transcribed_keys_follow_channel_then_speaker() {
        assert_eq!(SpeakerKey::transcribed(0, None), SpeakerKey::Me);
        assert_eq!(SpeakerKey::transcribed(0, Some(3)), SpeakerKey::Me);
        assert_eq!(SpeakerKey::transcribed(1, None), SpeakerKey::Them);
        assert_eq!(SpeakerKey::transcribed(1, Some(0)), SpeakerKey::Diarized(0));
        assert_eq!(
            SpeakerKey::transcribed(1, Some(GEMINI)),
            SpeakerKey::Diarized(GEMINI)
        );
    }

    #[test]
    fn an_unparseable_override_is_no_override() {
        assert_eq!(
            SpeakerKey::effective(1, Some(2), Some("guest:9")),
            SpeakerKey::Diarized(2)
        );
        assert_eq!(
            SpeakerKey::effective(1, Some(2), Some("me")),
            SpeakerKey::Me
        );
        assert_eq!(SpeakerKey::effective(0, None, None), SpeakerKey::Me);
    }

    #[test]
    fn default_labels_match_the_old_export_labels() {
        assert_eq!(default_label(SpeakerKey::Me), "Me");
        assert_eq!(default_label(SpeakerKey::Them), "Them");
        assert_eq!(default_label(SpeakerKey::Diarized(0)), "Speaker 1");
        assert_eq!(
            default_label(SpeakerKey::Diarized(GEMINI)),
            "Window 3 · Speaker 2"
        );
        assert_eq!(default_label(SpeakerKey::Added(0)), "Other speaker 1");
        assert_eq!(
            default_label(SpeakerKey::Diarized(u32::MAX >> 1)),
            "Speaker 2147483648"
        );
    }

    fn roster(segs: &[(u8, Option<u32>, Option<&str>)], names: &[(&str, &str)]) -> Roster {
        Roster::new(segs.iter().copied(), names.iter().copied())
    }

    #[test]
    fn a_name_row_wins_over_the_default_label() {
        let r = roster(
            &[(0, None, None), (1, Some(0), None), (1, Some(1), None)],
            &[("them:1", " Dana "), ("me", "Chaz")],
        );
        assert_eq!(r.label(SpeakerKey::Diarized(1)), "Dana");
        assert_eq!(r.label(SpeakerKey::Diarized(0)), "Speaker 1");
        assert_eq!(r.label(SpeakerKey::Me), "Chaz");
        // Never seen: still a default label, never a panic.
        assert_eq!(r.label(SpeakerKey::Diarized(9)), "Speaker 10");
    }

    #[test]
    fn me_is_slot_zero_even_when_others_speak_first() {
        let r = roster(
            &[(1, Some(4), None), (1, Some(2), None), (0, None, None)],
            &[],
        );
        assert_eq!(r.slot(SpeakerKey::Me), 0);
        assert_eq!(r.slot(SpeakerKey::Diarized(4)), 1);
        assert_eq!(r.slot(SpeakerKey::Diarized(2)), 2);
    }

    #[test]
    fn slots_follow_transcribed_order_and_never_move_with_an_override() {
        let before = roster(
            &[(1, Some(0), None), (1, Some(1), None), (1, Some(2), None)],
            &[],
        );
        // Every line of speaker 0 moved to speaker 2: nobody repaints.
        let after = roster(
            &[
                (1, Some(0), Some("them:2")),
                (1, Some(1), None),
                (1, Some(2), None),
            ],
            &[],
        );
        for k in [0, 1, 2] {
            let key = SpeakerKey::Diarized(k);
            assert_eq!(before.slot(key), after.slot(key));
        }
        assert_eq!(
            [0, 1, 2].map(|k| after.slot(SpeakerKey::Diarized(k))),
            [1, 2, 3]
        );
        // The derived slot is not constant across keys.
        assert_ne!(
            after.slot(SpeakerKey::Diarized(0)),
            after.slot(SpeakerKey::Diarized(1))
        );
    }

    #[test]
    fn added_keys_take_slots_after_every_transcribed_key_in_key_order() {
        let r = roster(
            &[
                (0, None, Some("added:1")),
                (1, Some(0), None),
                (0, None, Some("added:0")),
                (1, Some(1), None),
            ],
            &[],
        );
        assert_eq!(r.slot(SpeakerKey::Diarized(0)), 1);
        assert_eq!(r.slot(SpeakerKey::Diarized(1)), 2);
        assert_eq!(r.slot(SpeakerKey::Added(0)), 3);
        assert_eq!(r.slot(SpeakerKey::Added(1)), 4);
        assert_eq!(r.next_added(), SpeakerKey::Added(2));
        assert_eq!(
            roster(&[(0, None, None)], &[]).next_added(),
            SpeakerKey::Added(0)
        );
    }

    #[test]
    fn keys_with_the_same_name_merge_ignoring_case_and_spaces_only() {
        let r = roster(
            &[
                (1, Some(0), None),
                (1, Some(1), None),
                (1, Some(2), None),
                (1, Some(3), None),
            ],
            &[
                ("them:0", "Sam"),
                ("them:2", "  sAM "),
                ("them:3", "Samuel"),
            ],
        );
        assert_eq!(
            r.slot(SpeakerKey::Diarized(2)),
            r.slot(SpeakerKey::Diarized(0))
        );
        assert_eq!(r.slot(SpeakerKey::Diarized(0)), 1);
        // Unmerged keys keep their own slots: no compaction, no repaint.
        assert_eq!(r.slot(SpeakerKey::Diarized(1)), 2);
        assert_eq!(r.slot(SpeakerKey::Diarized(3)), 4);
        let people = r.people();
        let labels: Vec<&str> = people.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["Me", "Sam", "Speaker 2", "Samuel"]);
        assert_eq!(
            people[1].keys,
            [SpeakerKey::Diarized(0), SpeakerKey::Diarized(2)]
        );
        assert_eq!(
            r.named("sam", SpeakerKey::Diarized(1)),
            Some(SpeakerKey::Diarized(0))
        );
        assert_eq!(
            r.named("Sam", SpeakerKey::Diarized(0)),
            Some(SpeakerKey::Diarized(2))
        );
        assert_eq!(r.named("Samu", SpeakerKey::Diarized(0)), None);
    }

    #[test]
    fn a_merged_key_takes_the_earliest_slot_and_splits_again_on_rename() {
        let segs = [(1, Some(0), None), (1, Some(1), None), (1, Some(2), None)];
        let merged = roster(&segs, &[("them:2", "Dana"), ("them:1", "Dana")]);
        assert_eq!(merged.slot(SpeakerKey::Diarized(2)), 2);
        assert_eq!(merged.slot(SpeakerKey::Diarized(1)), 2);
        assert_eq!(merged.people().len(), 3);
        let apart = roster(&segs, &[("them:2", "Dana"), ("them:1", "Dee")]);
        assert_eq!(apart.slot(SpeakerKey::Diarized(2)), 3);
        assert_eq!(apart.people().len(), 4);
    }

    #[test]
    fn naming_someone_like_me_never_takes_slot_zero() {
        let r = roster(
            &[(0, None, None), (1, Some(0), None)],
            &[("me", "Chaz"), ("them:0", "chaz")],
        );
        assert_eq!(r.slot(SpeakerKey::Me), 0);
        assert_eq!(r.slot(SpeakerKey::Diarized(0)), 0);
        // Merged with Me in the menu, as one person.
        assert_eq!(r.people().len(), 1);
        let only_them = roster(&[(1, Some(0), None)], &[("them:0", "Me")]);
        assert_eq!(only_them.slot(SpeakerKey::Diarized(0)), 0);
    }

    #[test]
    fn speaking_lists_keys_with_effective_lines_in_slot_order() {
        let r = roster(
            &[
                (1, Some(0), None),
                (0, None, None),
                (1, Some(1), Some("me")),
                (1, Some(2), Some("added:0")),
            ],
            &[("them:5", "Nobody"), ("added:3", "Pat")],
        );
        assert_eq!(
            r.speaking(),
            [
                SpeakerKey::Me,
                SpeakerKey::Diarized(0),
                SpeakerKey::Added(0)
            ]
        );
        // Named added keys with no lines still exist for the menu.
        assert!(r.people().iter().any(|p| p.key == SpeakerKey::Added(3)));
        // A name row for a key that never spoke and is not added is ignored.
        assert!(!r.people().iter().any(|p| p.key == SpeakerKey::Diarized(5)));
    }

    #[test]
    fn me_is_listed_only_once_it_has_lines() {
        let r = roster(&[(1, Some(0), None)], &[]);
        assert_eq!(r.speaking(), [SpeakerKey::Diarized(0)]);
        assert_eq!(r.people()[0].key, SpeakerKey::Me);
    }

    #[test]
    fn notes_names_pair_each_key_with_its_default_and_display_label() {
        let r = roster(
            &[(0, None, None), (1, Some(1), None)],
            &[("them:1", "Dana"), ("bogus", "X")],
        );
        assert_eq!(
            r.notes_names(),
            [
                ("me".to_string(), "Me".to_string(), "Me".to_string()),
                (
                    "them:1".to_string(),
                    "Speaker 2".to_string(),
                    "Dana".to_string()
                ),
            ]
        );
    }
}
