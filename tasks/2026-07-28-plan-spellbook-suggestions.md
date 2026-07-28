# Plan — Spellbook learning: aliases, click-to-correct, and suggested terms

**Status:** planned, not started. Written 2026-07-28 against 0.26.0, expanded
the same day to fold in the alias data model and click-to-correct.
**Goal:** the Spellbook stops being a list the user maintains by hand. Hark
captures corrections where they happen, proposes terms it probably got wrong,
and can express corrections the phonetic matcher cannot reach on its own.

Three things that only work together:

1. **Aliases** — an entry can carry known-bad spellings, so corrections the
   fuzzy matcher can't reach become expressible.
2. **Click-to-correct** — clicking a word in History authors those aliases
   without anyone retyping the mistake.
3. **Suggestions** — Hark proposes entries on its own, from the same signals.

Aliases without click-to-correct is a form nobody will fill in. Click-to-correct
without aliases throws away half of what the click already knows.

---

## 0. The constraint that shapes everything

The motivating example — Hark hears "Al Drazi", the user fixes it to "Eldrazi" —
happens **in the user's editor, in another process, after injection**. Hark
never sees it. There is no supported, non-invasive way to observe an edit made
in Notepad, VS Code, or a browser textarea: reading it back would mean
accessibility-API scraping of arbitrary windows, which is fragile, permission-
hungry, and a privacy posture Hark should not adopt for a convenience feature.
**Rejected — do not revisit without a much stronger reason.**

The feature therefore cannot observe the real fix. It has to make correcting
*inside Hark* cheaper than correcting outside it, and infer the rest.

---

## 1. Data model: entries with aliases

### The shape

Today a Spellbook entry is a bare canonical string, and correction is purely
phonetic. The new shape:

```
Eldrazi
  also correct: "Al Drazi", "El Drossy"
```

An entry is `{ term, aliases }`. Simple mode shows only `term` and behaves
exactly as today. A per-entry **Advanced** checkbox reveals the alias field.

**Per-entry, never a global mode.** A global "advanced" toggle taxes the simple
path forever; a checkbox on the entry being edited costs nothing when unused.

### Why aliases are not redundant with the phonetic matcher

[`Corrector`](../crates/hark-spellbook/src/lib.rs) requires Double Metaphone
equality **plus** Jaro-Winkler ≥ 0.85. That net has a hard edge, and two common
failures fall outside it:

- **Badly split proper nouns.** The provider renders three syllables as
  unrelated words; no single-window phonetic comparison recovers it.
- **The provider emits a legitimate English word.** The matcher is deliberately
  conservative here, and should stay that way — a false positive on a real word
  is expensive.

An alias is the explicit override for exactly the cases the fuzzy net cannot or
should not reach. Complementary, not duplicative.

### Matching order

Aliases match **first** — exact, normalized, case-insensitive, multi-word — then
the phonetic pass sweeps the remainder. Explicit user intent beats inference.
Multi-word aliases need no new machinery: `window_matches` in
[`matcher.rs`](../crates/hark-spellbook/src/matcher.rs) already walks multi-word
windows. Output casing follows the canonical term, as it already does.

### The alias footgun

An alias of `there` fires on every legitimate "there" the user ever says. This
is the single most likely way the feature makes Hark worse. Mitigation, in
preference order:

1. Warn at entry time when an alias is a common English word (ship a small
   frequency list; no network).
2. Offer a scope selector — whole-utterance vs anywhere-in-sentence. The
   precedent already exists in
   [invocations](../crates/hark-config/src/invocations.rs) (`Scope`), so this is
   a known pattern rather than a new concept.

### Two matching systems means two explanations

With an exact pass and a fuzzy pass, "why did my text change?" gets harder to
answer. Cheap mitigation: record which entry fired and badge the History row.
Worth doing in the same release, not later — the debuggability cost lands the
moment aliases ship.

---

## 2. Capture surface: highlight-and-add in History

**This supersedes two earlier drafts of this section:** editable history rows
(worse — demands retyping the correction in prose, and leaves the corrected span
ambiguous) and per-word clickable widgets (worse — turns a paragraph into a row
of tiles, breaking wrapping, spacing, and ordinary drag-to-select).

The interaction is the one every desktop app already has: **highlight the text,
then act on it.**

### Interaction

The user reads "I cast the Al Drazi commander" in a History row, highlights
`Al Drazi`, and presses **Add**. That takes them to the Spellbook with the heard
text already filled in; they type `Eldrazi` and save.

- **Selection is the primitive, not the word.** Multi-word phrases work for
  free, which matters because the motivating example is two words and most
  mangled proper nouns split. No custom drag code, no new idiom to learn, and
  shift+arrow keyboard selection comes along with it.
- **Text stays text.** Wrapping, spacing, and ordinary copy are untouched, so
  the panel does not change for anyone who never uses the feature.
- **Snap outward to word boundaries**, using
  [`hark-spellbook`'s own tokenizer](../crates/hark-spellbook/src/tokenize.rs).
  Select `l Draz` and you get `Al Drazi`. This is not just forgiveness: it
  guarantees the captured span is tokenized identically to the span the matcher
  will later look for. A hand-rolled word split here would silently produce
  entries that never fire.
- **Paint the snapped range**, so the expansion is visible rather than
  surprising.
- **The Add button lives in the row's existing action cluster** (beside copy and
  delete), enabled when that row has a selection. Always in the same place, no
  placement edge cases at panel edges, and more discoverable than a chip that
  materializes under the cursor. A floating chip is a later refinement, not v1.
- **Both raw and final text are selectable.** Raw is the more useful source —
  see the cleanup caveat in §8.
- **Keep the user's place.** Jumping to the Spellbook loses the entry they were
  reading, which stings while working through a dictation with three mangled
  names. Either return to History after saving, or complete the add without
  leaving the page. Decide before building.
- **Undo as a toast.** "Added Eldrazi · Undo". Adding must be as cheap to
  reverse as to perform.
- **Frequency badge.** "You've said this 4 times", from history — answers "is
  this worth adding?" before the user decides.
- **Tooltips.** The feature is invisible until someone highlights something;
  a hint on the Add button and a one-time nudge under the history list are the
  cheapest fix.

### Why this solves the alias data-entry problem

Nobody wants to *type* "Al Drazi" into an alias field — that means reproducing
the mistake from memory, correctly. Highlighting the span in the **raw**
transcript gives Hark both sides for free: `heard` is the selection, `correct`
is the one thing the user types. The alias path stops being a form and becomes a
byproduct of pointing at the mistake, which is also why no separate popover
editor gets built: the real Spellbook editor is the destination.

### What egui gives us, and what it does not (verified against egui 0.35.0)

Read the source before writing any of this. The split is not where you would
guess:

- `Label::selectable(bool)` is **public** — selection, drag, shift+arrow, and
  the highlight visuals all work out of the box.
- `LabelSelectionState::has_selection()` is **public** — so "is the Add button
  enabled?" is answerable.
- **The selected string is not reachable.** `selected_text()` is a private free
  function, `text_to_copy` is a private field on a private struct, and it is
  only ever populated when `got_copy_event()` fires — egui does not even
  assemble the string unless the user pressed Ctrl+C.

So egui hands over the *interaction* but not the *data*. The route that works is
to own the selection against the galley, using two public epaint APIs:

- `Galley::cursor_from_pos(pos) -> CCursor` — pointer position to char index.
- `Galley::pos_from_cursor(cursor) -> Rect` — char index back to a rect, for
  painting the highlight.

That is precisely what egui does internally, so the approach is sound; it is
just not reusable as published. Track both ends of the drag against the galley,
slice by char range, snap to word boundaries, paint the range.

**Do not mirror egui's selection alongside our own.** Two sources of truth for
"what is selected" will agree in testing and diverge on keyboard selection,
elided text, and multi-row spans. Own it or use it, not both.

**Rejected: the clipboard round-trip.** Triggering a copy and reading the
clipboard back would work, and Hark already has stash/restore machinery in
`hark-inject`. It also clobbers the user's clipboard on a read-only action and
races the injection path. Not worth it.

**Worth considering separately:** upstreaming a public accessor for the current
label selection to egui. It is a small, obviously-useful addition, and it would
delete most of this section.

### Retroactive check

On add, report how many past entries this would have fixed, and offer to apply
it to the current one. Double duty: it is satisfying, and it is a live
correctness test — see §3.

---

## 3. The phonetic gate is a router, not a filter

The first draft of this plan treated the phonetic gate as a rejecter: drop any
suggestion the corrector could never apply. With aliases in the model, that is
the wrong verb. Given a pair `(heard, correct)`:

| Phonetic gate | Meaning | Action |
|---|---|---|
| Already corrected by the current Spellbook | The user's list is fine | **Drop** — proposing it is noise |
| Passes the gate | A plain canonical term suffices | Propose a **simple entry** |
| Fails the gate | Only an explicit mapping can fix it | Propose an entry **with an alias** |

This is the single best consequence of folding the two ideas together. The same
pure function that used to discard the hard cases now routes them to the feature
built to handle them, and nothing is silently dropped except genuine noise.

It also gives the retroactive check its meaning: a proposed *simple* entry that
would have changed zero past occurrences is mis-routed and should have been an
alias. That is a testable invariant, not a heuristic.

Fixtures, in `hark-spellbook`, pure:
- `("Al Drazi", "Eldrazi")` — must route correctly and consistently, and must
  **not** be already-corrected against an empty Spellbook.
- A same-sound pair that passes the gate → simple entry.
- A real-English-word pair (`"there" / "their"`) → alias route **plus** the
  common-word warning from §1.

---

## 4. Suggestions (unattended)

### Source: LLM mining of recent transcripts

Batch recent transcripts and ask the user's **existing cleanup provider** (same
BYOK key, same [`hark-voice`](../crates/hark-voice/src/lib.rs) adapter) which
spans look like mangled proper nouns, product names, or jargon, and what the
intended spelling likely is. A model that has seen the surrounding sentence can
identify "Al Drazi" as a mangled proper noun in a way no local heuristic can.

Results route through §3 exactly like a click-authored pair, so mining produces
simple entries and alias entries without a separate code path.

**This is the first feature that would send stored history off-device.** Today
cleanup transmits only the transcript currently in flight; history is local-only
and the settings copy
([`ui/settings/form.rs`](../crates/hark-app/src/ui/settings/form.rs)) says so.
Mining must be **off by default, opt-in, with copy that states plainly what
leaves the machine.** Do not fold it into an existing toggle — shipping it
on-by-default would make existing UI text false.

### Rejected for v1: re-dictation proximity

"User re-dictates a similar utterance within N seconds" fires on genuine
rephrasing, hotkey slips, and simple repetition, and never reveals the *correct*
spelling — only that something went wrong. Not worth the false-positive budget.

### Review queue

A **Suggested** section at the top of the Spellbook page, visible only when
something is pending (no empty-state clutter on the common path). Each row shows
`heard → term`, the times seen, and Accept / Dismiss. Accept takes the same
persist-and-restart path manual edits already use
([`ui/pages.rs`](../crates/hark-app/src/ui/pages.rs), `spellbook()`), so there is
exactly one code path by which a term enters the Spellbook.

**Never auto-add.** A wrong auto-added term corrupts every future dictation
containing that sound, silently.

---

## 5. Storage and config migration

### Suggestions table

New migration `004_spellbook_suggestions.sql`. **Never renumber 001–003**
(BP FOUNDATIONAL, already noted in [`hark-store`](../crates/hark-store/src/lib.rs)).

```sql
CREATE TABLE spellbook_suggestions (
  id          INTEGER PRIMARY KEY,
  term        TEXT NOT NULL,          -- proposed canonical spelling
  heard       TEXT NOT NULL,          -- what the provider returned
  kind        TEXT NOT NULL,          -- 'simple' | 'alias'  (routed per §3)
  source      TEXT NOT NULL,          -- 'click' | 'mined'
  seen_count  INTEGER NOT NULL DEFAULT 1,
  first_ts_ms INTEGER NOT NULL,
  last_ts_ms  INTEGER NOT NULL,
  status      TEXT NOT NULL DEFAULT 'pending'  -- 'pending'|'accepted'|'dismissed'
);
CREATE UNIQUE INDEX idx_suggestions_term ON spellbook_suggestions(term, heard);
```

`dismissed` persists **forever**, and dedup runs against *all* rows, not just
pending ones. Deduping against the accepted set alone is the classic convergence
bug: every dismissed suggestion returns on the next mining run, forever.

`seen_count` drives ranking — a word Hark has fumbled five times is a better
suggestion than one it fumbled once.

### Config schema — and the migration that has never run

`[spellbook]` goes from a flat `terms = [...]` array to entries with aliases:

```toml
[[spellbook.entries]]
term = "Eldrazi"
aliases = ["Al Drazi"]
```

Old flat arrays must keep loading. That is achievable with a custom
deserializer accepting either shape, the same backward-compat discipline the
0.26.0 rename used.

**But this is the second schema change to this section in one release cycle,
`CONFIG_VERSION` is still 1, and the documented backup-then-migrate flow
(BP `versioned-config-migration-backup`, cited in
[`hark-config`](../crates/hark-config/src/lib.rs)) has never actually executed.**
This is the right change to exercise it on: back up as `config.toml.v1.bak`,
map fields explicitly, bump `CONFIG_VERSION`, persist. Far better that the flow
runs for the first time on a change we planned than on one we didn't.

---

## 6. Phases

Ordered by risk, not by layer. The selection work is the only part of this plan
that is not a known quantity, and the config migration is the only part that is
irreversible — so the unknown resolves first and the irreversible waits.

**Slice 0 — selection spike (throwaway, gates everything else)**
- Get a real char range out of a real History row via `cursor_from_pos`, snap it
  to `hark-spellbook` token boundaries, paint the snapped highlight.
- Timebox it. If owning selection against the galley turns out to fight egui's
  own selection painting, that is worth knowing before any schema moves.

**Slice 1 — highlight-and-add, simple terms only**
- Add button in the row action cluster, handoff to the Spellbook with the term
  prefilled, undo toast, tooltips.
- **No schema change, no migration, no aliases.** A complete, shippable feature
  on its own: everything captured this way is a plain canonical term, exactly
  what today's `terms` array already holds.
- Ship it and use it. Real pairs from real use are what tell you whether the §3
  router splits traffic the way this plan predicts.

**Slice 2 — aliases**
- Alias data model in `hark-config` (either-shape deserializer) + the
  backup-then-migrate flow and `CONFIG_VERSION` bump, now that the interaction
  it exists to serve is proven.
- Alias matching in `hark-spellbook`, ahead of the phonetic pass. Tests:
  multi-word aliases, casing, alias-beats-phonetic precedence.
- The §3 router as a pure function, with all three fixtures.
- Spellbook entry editor: per-entry Advanced checkbox, alias list, common-word
  warning. Handoff from History now prefills the alias field.

**Slice 3 — suggestions**
- Migration 004 + suggestion read/write/dedup.

- Suggested section, ranking, Accept/Dismiss.
- LLM mining behind its opt-in, with the privacy copy.
- Retroactive check and "which entry fired" badge in History.
- Honest gated states: capture off, no cleanup provider resolved.

**Ship**
- Verify on real Windows/macOS: drag selection and snapping feel (not testable
  on this machine), and that mining never perturbs release-to-inject.

---

## 7. Constraints this must not violate

- **Nothing on the hot path.** Mining is scheduled or manual, on a worker
  thread. Alias matching *is* on the hot path — it must stay pure string work,
  measured against the existing budget (well under 10 ms for a 100-word
  utterance).
- **No auto-add, ever.**
- **Content stays out of logs.** Suggestions, aliases, and clicked spans are
  transcript-derived; `DictationRecord` has no `Debug` impl on purpose and the
  same discipline applies. Log counts, never terms.
- **`capture = false` is a real configuration.** Click-to-correct and mining are
  both inert without stored text; degrade visibly, never silently.
- **One HTTP client.** Mining reuses the shared long-lived client.
- **Invocation rows are excluded.** `entries.invocation IS NOT NULL` means the
  text was pasted from canned text, not transcribed; offering to correct words
  nobody spoke is nonsense.

---

## 8. Lessons Learned / Gotchas

Pre-implementation; fold discoveries back here and route the durable ones to
LL-G via `/add-lesson`.

- **The premise needs correcting before design starts.** "Learn from what the
  user edited" sounds implementable until you notice the edit happens in another
  process. Anyone picking this up will lose a day to accessibility APIs if the
  §0 rejection is not read first.
- **Aliases and highlight-and-add are one feature, not two.** Shipping the alias
  field alone produces a form that requires retyping a mistake from memory —
  it will go unused and read as bloat. If only one lands first, it must be the
  History interaction.
- **"The framework already does this" is worth ten minutes in the source.**
  egui really does implement label selection — drag, keyboard, highlight — and
  it really does not let you read the result: the selected string is assembled
  only on a copy event, into a private field. Both halves of that were
  surprising, and only one of them was guessable from the docs. The public
  `Galley::cursor_from_pos` / `pos_from_cursor` pair is the supported way to do
  it yourself.
- **Do not run two selection models side by side.** Deriving our own char range
  from pointer input while egui paints its own selection looks equivalent and
  diverges on keyboard selection, elided text, and multi-row spans.
- **Route, don't reject.** The earlier draft discarded pairs the phonetic
  matcher couldn't reach; those are precisely the pairs aliases exist for. A
  filter that silently drops the hardest cases looks identical to a broken
  feature.
- **Dedup against dismissed, not just accepted**, or mining re-proposes rejected
  terms every run.
- **An alias for a common English word is the main way this makes Hark worse.**
  Warn, or scope it. Both, ideally.
- **Two matching passes cost debuggability.** Ship the "which entry fired" badge
  with the feature, not after the first confused bug report.
- **Diffing cleaned text attributes the wrong error.** `final_text` has been
  through cleanup, which rewrites wording; treating that as an STT mistake
  blames the provider for a voice's rephrasing. Prefer `raw_text`, and suppress
  inference entirely where a non-Verbatim voice ran.
- **The config migration flow is untested in the field.** Whatever ships here is
  its first real execution. Test the backup path explicitly, including a
  read-only config directory.
