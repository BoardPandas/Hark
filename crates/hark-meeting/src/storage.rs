//! The circular audio storage cap (plan §4.9), decision half: which
//! recordings' audio must go so the total fits under the cap. Pure; the
//! deletion itself is in [`crate::storage_fs`], the only I/O this crate does.
//!
//! Rules: sizes come from the filesystem, not the database (a crash or a
//! manual delete would otherwise drift the total); whole recordings are
//! evicted, oldest end first; a recording still recording, or still being
//! transcribed or summarized, is never evicted, even when it alone exceeds
//! the cap. Only audio goes: transcripts and notes are never touched.

/// Bytes per megabyte of `audio_cap_mb` (binary: the UI shows GB = 1024 MB).
pub const MB: u64 = 1024 * 1024;

/// One meeting's audio on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAudio {
    pub id: String,
    /// Measured on disk (every file in the meeting's directory).
    pub bytes: u64,
    /// When the meeting ended; `None` while it is still recording, which
    /// also makes it ineligible for eviction.
    pub ended_ms: Option<u64>,
}

/// The outcome of [`plan_eviction`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionPlan {
    /// Meetings whose audio to delete, oldest first.
    pub evict: Vec<String>,
    /// Total audio bytes once `evict` is carried out.
    pub used_after: u64,
    /// Still over the cap afterwards: everything evictable is gone and the
    /// protected recordings alone exceed it. The UI says so once ("This
    /// meeting is larger than your storage cap"); recording carries on.
    pub over_cap: bool,
}

/// Which recordings' audio must go so the total is at most `cap_bytes`.
/// Never returns a `protected` id or a recording still in progress. Ties on
/// `ended_ms` break by id so the choice is stable across runs.
pub fn plan_eviction(
    recordings: &[StoredAudio],
    cap_bytes: u64,
    protected: &[&str],
) -> EvictionPlan {
    let mut used: u64 = recordings.iter().map(|r| r.bytes).sum();
    let mut candidates: Vec<(u64, &StoredAudio)> = recordings
        .iter()
        .filter(|r| !protected.contains(&r.id.as_str()))
        .filter_map(|r| r.ended_ms.map(|ended| (ended, r)))
        .collect();
    candidates.sort_by(|(a, ra), (b, rb)| a.cmp(b).then_with(|| ra.id.cmp(&rb.id)));

    let mut evict = Vec::new();
    for (_, r) in candidates {
        if used <= cap_bytes {
            break;
        }
        used -= r.bytes;
        evict.push(r.id.clone());
    }
    EvictionPlan {
        evict,
        used_after: used,
        over_cap: used > cap_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, mb: u64, ended_ms: Option<u64>) -> StoredAudio {
        StoredAudio {
            id: id.to_string(),
            bytes: mb * MB,
            ended_ms,
        }
    }

    #[test]
    fn under_the_cap_nothing_goes() {
        let recs = [rec("a", 100, Some(1)), rec("b", 100, Some(2))];
        let plan = plan_eviction(&recs, 200 * MB, &[]);
        assert!(plan.evict.is_empty());
        assert_eq!((plan.used_after, plan.over_cap), (200 * MB, false));
    }

    #[test]
    fn over_the_cap_the_oldest_go_first_until_it_fits() {
        let recs = [
            rec("newest", 300, Some(30)),
            rec("oldest", 100, Some(10)),
            rec("middle", 200, Some(20)),
        ];
        let plan = plan_eviction(&recs, 350 * MB, &[]);
        assert_eq!(plan.evict, ["oldest", "middle"]);
        assert_eq!((plan.used_after, plan.over_cap), (300 * MB, false));
    }

    #[test]
    fn a_protected_recording_is_never_evicted_even_alone_over_the_cap() {
        let recs = [
            rec("live", 900, None),
            rec("summarizing", 400, Some(50)),
            rec("old", 100, Some(10)),
        ];
        let plan = plan_eviction(&recs, 500 * MB, &["summarizing"]);
        assert_eq!(plan.evict, ["old"]);
        assert_eq!(plan.used_after, 1300 * MB);
        assert!(
            plan.over_cap,
            "the in-progress meeting alone exceeds the cap"
        );
    }

    #[test]
    fn cap_zero_evicts_every_finished_unprotected_recording() {
        let recs = [
            rec("a", 1, Some(1)),
            rec("b", 1, Some(2)),
            rec("live", 1, None),
        ];
        let plan = plan_eviction(&recs, 0, &[]);
        assert_eq!(plan.evict, ["a", "b"]);
        assert!(plan.over_cap);
    }

    #[test]
    fn ties_on_end_time_break_by_id() {
        let recs = [
            rec("b", 100, Some(5)),
            rec("a", 100, Some(5)),
            rec("c", 100, Some(5)),
        ];
        assert_eq!(plan_eviction(&recs, 150 * MB, &[]).evict, ["a", "b"]);
    }

    #[test]
    fn an_empty_store_needs_nothing() {
        let plan = plan_eviction(&[], 0, &[]);
        assert!(plan.evict.is_empty());
        assert_eq!((plan.used_after, plan.over_cap), (0, false));
    }

    #[test]
    fn exactly_at_the_cap_is_under_it() {
        let recs = [rec("a", 100, Some(1)), rec("b", 100, Some(2))];
        assert!(plan_eviction(&recs, 200 * MB, &[]).evict.is_empty());
        assert_eq!(plan_eviction(&recs, 200 * MB - 1, &[]).evict, ["a"]);
    }
}
