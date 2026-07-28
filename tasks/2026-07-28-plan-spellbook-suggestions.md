# Plan — Suggested Spellbook terms (context-aware corrections)

**Status:** planned, not started. Written 2026-07-28 against 0.26.0.
**Goal:** Hark notices words it probably got wrong and offers to add the right
spelling to the Spellbook, instead of the user maintaining that list by hand.

---

## 0. The constraint that shapes everything

The motivating example — Hark hears "Al Drazi", the user fixes it to "Eldrazi" —
happens **in the user's editor, in another process, after injection**. Hark
never sees it. There is no supported, non-invasive way to observe an edit made
in Notepad, VS Code, or a browser textarea: reading it back would mean
accessibility-API scraping of arbitrary windows, which is fragile, permission-
hungry, and a privacy posture Hark should not adopt for a convenience feature.
**Rejected — do not revisit without a much stronger reason.**

So the feature cannot literally observe "what the user had to go fix". It must
do one of two things instead:

1. **Create a place inside Hark where the correction happens**, and learn from
   that (high precision, needs a user action).
2. **Infer likely misrecognitions from the transcripts Hark already stores**
   (zero user effort, lower precision, needs a review step).

The plan ships both, feeding one review queue. Neither ever edits the Spellbook
on its own — every term reaches the list through an explicit Accept.

---

## 1. Signal sources

### Source A — Editable history (explicit, high precision)

History rows currently have copy + delete ([`ui/history/row.rs`](../crates/hark-app/src/ui/history/row.rs)).
Add inline edit, matching the Spellbook editor's interaction (pinned edit
buffer, Enter commits, Esc cancels).

When the user commits an edit, diff the stored `final_text` against the edited
text word-by-word. Each 1:1 substitution is a candidate `(heard, corrected)`
pair. Insertions and deletions are discarded — they are rephrasings, not
misrecognitions.

This is the honest analogue of the motivating example: the user still corrects
the text, but somewhere Hark can see. It also has standalone value (fixing a
history entry before re-copying it), which is why it is worth building even if
Source B is never enabled.

### Source B — LLM mining of recent transcripts (inferred, zero-effort)

Batch the last N transcripts and ask the user's **existing cleanup provider**
(same BYOK key, same adapter as `hark-voice`) one question: which spans look
like mangled proper nouns, product names, or jargon, and what is the likely
intended spelling?

Cheap, off the hot path, and it catches the "Al Drazi" case without the user
doing anything — a model that has seen the surrounding sentence can tell that
"Al Drazi" is a mangled proper noun in a way no local heuristic can.

**This is the first feature that would send stored history off-device.** Today
cleanup sends only the transcript currently in flight; history is local-only and
the UI says so. Source B must therefore be **off by default, opt-in, with copy
that states plainly what leaves the machine.** Do not fold it into an existing
toggle.

### Source C — Re-dictation proximity (rejected for v1)

"User re-dictates a similar utterance within N seconds" is a real failure signal
but a weak one: it fires on genuine rephrasing, on hotkey slips, and on the user
simply saying something twice, and it never tells you the *correct* spelling —
only that something went wrong. Not worth the false-positive budget. Revisit
only if A and B both underperform.

---

## 2. The filter that makes this feature work

**A suggestion is only worth showing if adding it to the Spellbook would
actually change the outcome.** Two tests, both pure and both reusing
[`hark-spellbook`](../crates/hark-spellbook/src/matcher.rs):

1. **Would the current Spellbook already fix it?** Run the existing `Corrector`
   over `heard`. If it already produces `corrected`, drop the suggestion — the
   user's list is fine and the noise is unwarranted.
2. **Could a Spellbook entry ever fix it?** The corrector matches on Double
   Metaphone equality confirmed by Jaro-Winkler ≥ 0.85. If `heard` and
   `corrected` do not clear that bar, adding `corrected` to the Spellbook will
   never fire on that misrecognition, and the suggestion is a lie. Drop it.

Test 2 is the load-bearing one and the thing most likely to be skipped by a
naive implementation. Without it the list fills with plausible-looking terms
that silently never do anything — worse than an empty list, because the user
concludes the whole feature is broken. It is also trivially unit-testable
against the real matcher, so there is no excuse for shipping it untested.
`("Al Drazi", "Eldrazi")` should be an explicit fixture in both directions:
it must pass test 2, and it must fail test 1 on an empty Spellbook.

---

## 3. Storage

New migration `004_spellbook_suggestions.sql`. **Never renumber 001–003**
(BP FOUNDATIONAL, already noted in [`hark-store`](../crates/hark-store/src/lib.rs)).

```sql
CREATE TABLE spellbook_suggestions (
  id         INTEGER PRIMARY KEY,
  term       TEXT NOT NULL,           -- the proposed canonical spelling
  heard      TEXT NOT NULL,           -- what the provider returned
  source     TEXT NOT NULL,           -- 'edit' | 'mined'
  seen_count INTEGER NOT NULL DEFAULT 1,
  first_ts_ms INTEGER NOT NULL,
  last_ts_ms  INTEGER NOT NULL,
  status     TEXT NOT NULL DEFAULT 'pending'  -- 'pending' | 'accepted' | 'dismissed'
);
CREATE UNIQUE INDEX idx_suggestions_term ON spellbook_suggestions(term, heard);
```

`status` must persist `dismissed` **forever**, and the dedup check must run
against *all* rows, not just pending ones. Dedup against the accepted set alone
is the classic convergence bug: every dismissed suggestion returns on the next
mining run and the user dismisses it again, permanently.

`seen_count` drives ranking — a word Hark has fumbled five times is a better
suggestion than one it fumbled once.

---

## 4. UI

A **Suggested** section at the top of the Spellbook page, visible only when
there is at least one pending suggestion (no empty-state clutter on the common
path). Each row: `heard → term`, how many times it was seen, Accept and Dismiss.

Accept appends to `settings.spellbook.terms` and takes the existing
persist-and-restart path that manual edits already use
([`ui/pages.rs`](../crates/hark-app/src/ui/pages.rs), `spellbook()`), so there is
one code path for "a term entered the Spellbook", not two.

Honest states, per the repo's no-blank-region rule:
- History capture off → say the feature needs history capture, and why.
- Mining on but no cleanup provider resolved → say that, don't show silence.

---

## 5. Phases

**Foundation**
- Migration 004 + `hark-store` read/write/dedup. Tests: dedup across all
  statuses, ranking by `seen_count`.
- The §2 filter as a pure function in `hark-spellbook`, with the
  `Al Drazi → Eldrazi` fixtures.

**Core**
- Source A: inline history edit + word-diff → candidate pairs.
- Suggested section in the Spellbook page, Accept/Dismiss wired to the existing
  persist-and-restart path.

**Polish**
- Source B: mining call on a worker thread, opt-in setting, explicit privacy
  copy. Reuse the `hark-voice` adapter; do not add a second HTTP client.
- Ranking, honest empty/gated states.

**Ship**
- Verify on real Windows/macOS: the edit affordance, and that mining never
  perturbs release-to-inject latency.

---

## 6. Constraints this must not violate

- **Nothing on the hot path.** Mining is scheduled or manual, on a worker
  thread. Diffing runs on a user edit, not during dictation.
- **No auto-add, ever.** A wrong auto-added term corrupts every future
  dictation containing that sound, silently. Accept is always explicit.
- **Content stays out of logs.** Suggestions are transcript-derived;
  `DictationRecord` has no `Debug` impl on purpose and the same discipline
  applies here. Log counts, never terms.
- **`capture = false` is a real configuration.** Both sources are inert without
  stored text; degrade visibly, never silently.
- **One HTTP client.** Mining reuses the shared long-lived client.

---

## 7. Lessons Learned / Gotchas

Pre-implementation; fold discoveries back here and route the durable ones to
LL-G via `/add-lesson`.

- **The premise needs correcting before design starts.** "Learn from what the
  user edited" sounds implementable until you notice the edit happens in another
  process. Anyone picking this up will lose a day to accessibility APIs if the
  §0 rejection is not read first.
- **Suggesting a term the corrector cannot apply is worse than suggesting
  nothing.** The §2 test-2 gate is the difference between a feature and a
  decorative list. Expect it to be the thing that gets dropped under time
  pressure.
- **Dedup against dismissed, not just accepted**, or mining re-proposes rejected
  terms every run. Same convergence trap as loop-until-dry without a `seen` set.
- **Mining changes Hark's privacy story**, not just its feature set. History
  being local-only is currently stated plainly in the settings copy
  ([`ui/settings/form.rs`](../crates/hark-app/src/ui/settings/form.rs)); shipping
  an on-by-default miner would make that text false.
- **Word-diff on cleaned text attributes the wrong error.** `final_text` has
  already been through cleanup, which rewrites wording; a diff against it can
  blame the STT provider for a voice's rephrasing. Diff against `raw_text` when
  the entry has no cleanup model, and consider suppressing Source A entirely for
  entries where a non-Verbatim voice ran.
- **Invocation rows must be excluded.** `entries.invocation IS NOT NULL` means
  the text was pasted from canned text, not transcribed; mining it would suggest
  terms for words nobody spoke.
