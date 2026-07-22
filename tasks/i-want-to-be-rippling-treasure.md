# Plan — Invocations (canned phrase → canned text)

> Status: planned 2026-07-22. Target version **0.19.0** (Minor: new feature).

## Context

Hark today turns speech into polished text. There is no way to say a short
phrase and have a long, *fixed* block of text appear — a text-expander /
snippet capability. The user's example: saying **"Access Granted"** should type
out a paragraph listing the tools a support tech has access to.

Nothing invocation-like exists in the repo today (verified: exhaustive grep for
invocation / snippet / macro / expansion / canned / trigger / phrase across
`tasks/`, `Docs/`, `README.md`, `CHANGELOG.md`, and all crates found no prior
art and no deliberate deferral). The design space is open.

**Outcome:** a fourth sidebar section — History, Dictionary, **Invocations**,
Stats — where the user maintains trigger-phrase → expansion pairs. When the
pipeline hears a trigger, the canned text is injected *exactly as authored*.

### Decisions locked with the user (2026-07-22)

| Decision | Choice |
|---|---|
| Trigger scope | **Per-invocation**: "whole dictation only" (default) or "anywhere in the sentence" |
| Match strictness | **Fuzzy/phonetic**, reusing Hark's existing guarded matcher |
| LLM cleanup voices | **Never touch canned text** — a fired invocation skips cleanup entirely |
| Ship in v1 | History badge · per-row Test button · multi-line expansions |
| Not in v1 | Sending trigger phrases to the STT provider as bias/keyterm hints |

---

## 1. Where it runs in the pipeline

Today, [worker.rs:218-219](crates/hark-pipeline/src/worker.rs:218):

```rust
let text = corrected_text(&worker.corrector, &transcript.text);
let cleaned = cleaned_text(worker.cleanup.as_ref(), &worker.corrector, text);
```

becomes:

```rust
let text = corrected_text(&worker.corrector, &transcript.text);
let expanded = expanded_text(&worker.expander, text);
// Canned text is authored, not spoken: it never reaches a cleanup model that
// would rewrite it, and never pays for the call.
let plan = worker.cleanup.as_ref().filter(|_| expanded.fired.is_none());
let cleaned = cleaned_text(plan, &worker.corrector, expanded.text);
```

`expanded_text` is a ~12-line sibling of the existing
[`corrected_text`](crates/hark-pipeline/src/worker.rs:342) — same shape, same
logging discipline (counts and millis only, **never** phrases or expansion text).

**Why after dictionary pass 1:** pass 1 canonicalizes the *spoken* words, which
can only help a trigger match (matching lowercases both sides, so casing changes
are harmless). The expansion is inserted afterwards and is therefore never
touched by the dictionary.

**Why `Option::filter` rather than "clean, then splice":** setting `plan = None`
makes [`cleaned_text`](crates/hark-pipeline/src/worker.rs:286) hit its
`passthrough` on the first line, which kills four failure modes at once:

1. `over_expanded` never sees the canned text. The guard's allowance is
   `max(input_words × ratio, input_words + 3)`
   ([voices.rs:160-170](crates/hark-voice/src/voices.rs:160), grace = 3.0 words).
   A 3-word utterance expanding to 60 words gets an allowance of **6** — even
   `max_expansion_ratio = 10.0` still rejects it, and the expansion would be
   *silently discarded* in favour of the literal words "access granted".
2. `skips_cleanup` never flips. A short, free, instant dictation must not turn
   into a billed LLM round trip on the release-to-inject path (Groq bills a 10 s
   minimum per request).
3. No HTTP call — no latency, no cost, no failure mode.
4. Dictionary pass 2 (inside `cleaned_text`'s success arm,
   [worker.rs:329](crates/hark-pipeline/src/worker.rs:329)) never runs over the
   expansion, so phonetic post-correction can't mangle URLs or product names in
   the user's own authored text.

Protection must be **control flow, not persuasion**. Do not add a "leave this
untouched" clause to a voice prompt: every built-in voice already carries
`LENGTH_DISCIPLINE_CLAUSE` ("never expand a short remark into a paragraph") and
would actively fight the expansion, and the existing protected-terms clause
([voices.rs `budgeted_terms`](crates/hark-voice/src/voices.rs)) is a soft,
budget-capped comma list, not a span-preservation primitive.

**Accepted cost:** an "anywhere"-scope trigger inside a longer sentence means
that whole dictation loses its cleanup pass — filler words around the expansion
survive uncleaned. Stated plainly in the Invocations page copy.

---

## 2. Matching — reuse the guarded matcher, don't rebuild it

New module **`crates/hark-dictionary/src/expander.rs`** exposing
`hark_dictionary::Expander`.

**Why here and not a new crate:** the phonetic guard is LL-G HIGH
(`rust/phonetic-code-equality-needs-confirm-guard`) — Double Metaphone alone
maps "matter" and "modero" to the same code. The ≥4-char / all-alphabetic gate,
the digit/short-word exact-only path, and the Jaro-Winkler confirmation already
live in [`matcher.rs`](crates/hark-dictionary/src/matcher.rs) and
[`tokenize.rs`](crates/hark-dictionary/src/tokenize.rs), which are `pub(crate)`.
Reimplementing them in a sibling crate duplicates a HIGH-severity guard;
exporting them widens the API for no gain. hark-dictionary *is* the
phrase-matching crate — it gets a second consumer. All files stay well under 500
lines (`lib.rs` 373, `matcher.rs` 132, `tokenize.rs` 146, `expander.rs` ~200).

### Reused as-is
- [`tokenize::tokenize`](crates/hark-dictionary/src/tokenize.rs:28) — word cores
  with byte spans; hyphens split; punctuation outside the span, so splicing
  preserves it.
- [`matcher::build_entries`](crates/hark-dictionary/src/matcher.rs:58) —
  precompute + longest-first sort so multi-word triggers win overlaps.
- [`matcher::encode`](crates/hark-dictionary/src/matcher.rs:25) /
  [`window_matches`](crates/hark-dictionary/src/matcher.rs:101).

### One change to `matcher.rs`
Thread the Jaro-Winkler threshold as a parameter:
`window_matches(entry, tokens, codes, min_jw: f64)` →
`word_matches(word, token, codes, min_jw)`.

- `Corrector` passes the existing `JW_CONFIRM_THRESHOLD` (0.85) — behaviour and
  all 30 existing dictionary tests unchanged.
- `Expander` passes `INVOCATION_JW_THRESHOLD` (**0.90**).

Rationale in a code comment: *a dictionary false positive corrupts one word; an
invocation false positive pastes a paragraph.* Same guard, tighter confirm.

### `Expander` semantics

```rust
pub enum Scope { Utterance, Anywhere }

pub struct Expansion {
    pub text: String,
    /// The trigger phrase that fired, for the history record. `None` = nothing fired.
    pub fired: Option<String>,
}

impl Expander {
    pub fn new(entries: &[(String, String, Scope)]) -> Expander;   // (phrase, expansion, scope)
    pub fn expand(&self, text: &str) -> Expansion;
    /// Powers the UI Test panel: the closest non-firing trigger and its score.
    pub fn closest(&self, text: &str) -> Option<(&str, f64)>;
}
```

`expand` tokenizes once, then:

1. **Utterance pass first.** For each `Scope::Utterance` entry, if
   `tokens.len() == entry.word_count()` **and** the window matches, the whole
   result *is* the expansion. Return immediately. (Punctuation and casing around
   the words are irrelevant — they sit outside token spans.)
2. **Anywhere pass.** Only `Scope::Anywhere` entries, longest-first, consuming
   matched token windows, splicing over byte spans — the same algorithm as
   [`Corrector::correct`](crates/hark-dictionary/src/lib.rs:44). Multiple
   different triggers may fire; `fired` reports the first.
3. Nothing matched → `Expansion { text, fired: None }`, and `Corrector`'s
   empty-set fast path is mirrored so a user with no invocations pays only one
   `String` move.

### Build-time hygiene (fail soft, never fail the load)
`Expander::new` **skips** and counts (never logs the text itself):
- a phrase tokenizing to **fewer than 2 words** — one-word triggers fire
  constantly and are the single biggest false-positive source;
- an empty expansion;
- a **duplicate** phrase (normalized lowercase token sequence) — **first wins**.
  LL-G `sqlite/upsert-by-name-collision`: decide duplicate semantics *now*, not
  after users have data. The phrase is the identity; no hidden ids, so the TOML
  stays hand-editable (Hark's house style).

The Invocations page shows a per-row warning for any entry that will not arm, so
a hand-edited config never fails silently.

---

## 3. Config (`hark-config`)

New module **`crates/hark-config/src/invocations.rs`** — `lib.rs` is already 803
lines, over the project's 500-line rule; do not grow it.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Scope { #[default] Utterance, Anywhere }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Invocation { pub phrase: String, pub expansion: String, pub scope: Scope }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Invocations { pub entries: Vec<Invocation> }
```

`Settings` gains `pub invocations: Invocations` **as the last field** (TOML
arrays-of-tables must follow scalar keys) plus its `Default` arm.

```toml
[invocations]

[[invocations.entries]]
phrase = "access granted"
scope = "utterance"          # or "anywhere"
expansion = "You have access to the Support Forge tools..."
```

**No `Settings::validate` rule and no `CONFIG_VERSION` bump.** Per BP
`safety/versioned-config-migration-backup`, the first version bump must ship the
backup-then-migrate flow; this change is purely additive under
`#[serde(default)]`, so old files load unchanged and new files are readable by
older builds (unknown keys are already tolerated,
[lib.rs:543](crates/hark-config/src/lib.rs:543)). Hard-rejecting a bad phrase in
`validate` would make a hand-edited config unloadable and strand the user with
no UI to fix it — hence the fail-soft build-time skip above.

`config/default-config.toml` gains a commented `[invocations]` section next to
`[dictionary]` ([line 42](config/default-config.toml:42)).

---

## 4. Pipeline wiring (`hark-pipeline`)

- `Worker` gains `pub expander: Expander`, built in
  [`run`](crates/hark-pipeline/src/lib.rs:327) beside the existing `corrector`
  (in-memory matcher is a **derived artifact**, rebuilt from TOML on every
  pipeline start — LL-G `architecture/transient-cache-without-drift`; TOML stays
  the single owner, nothing is cached in SQLite).
- `DictationRecord` ([events.rs:12](crates/hark-pipeline/src/events.rs:12)) gains
  `pub invocation: Option<String>`.
- `expanded_text` is the pure, unit-tested seam.

---

## 5. Storage + stats honesty (`hark-store`)

- New migration **`crates/hark-store/migrations/003_entries_invocation.sql`**:
  `ALTER TABLE entries ADD COLUMN invocation TEXT;` (nullable, no DEFAULT, no
  backfill). Append to `MIGRATIONS`
  ([lib.rs:24](crates/hark-store/src/lib.rs:24)); **never** edit or renumber 001/002.
- `NewDictation` and `Entry` gain `invocation: Option<String>`; update the INSERT
  ([lib.rs:171](crates/hark-store/src/lib.rs:171)), the SELECT column list, and
  the row mapper.
- **Stats fix.** [`record`](crates/hark-store/src/lib.rs:195) counts
  `word_count(&d.final_text)`, and
  [`format::time_saved_ms`](crates/hark-app/src/ui/format.rs:132) values every
  word at 1500 ms. A 2-word utterance producing a 300-word expansion would
  fabricate ~7.5 minutes of "time saved". Change to: when
  `d.invocation.is_some()`, count `word_count(&d.raw_text)` — the words actually
  spoken. One `if`, one test, one CHANGELOG sentence so the shift doesn't read
  as a regression.

---

## 6. UI (`hark-app`)

### Nav
- `theme::icons` gains `LIGHTNING: &str = "\u{E2DE}"` (alphabetical, between
  `KEY` and `MAGNIFYING_GLASS`). **Verified**: present in the vendored
  `assets/Phosphor.ttf` cmap at gid 389, and matching egui-phosphor v0.12.0's
  constant — the font is the full regular variant, so no subsetting step exists.
- `pages::Page::Invocations` + `label()` / `icon()` / `description()` arms,
  `Views.invocations`, and the dispatch arm.
- [shell.rs:179-183](crates/hark-app/src/ui/shell.rs:179) nav array becomes
  `[History, Dictionary, Invocations, Stats]`.
- `Views` construction at [app.rs:68](crates/hark-app/src/app.rs:68).

### Page — `crates/hark-app/src/ui/invocations/{mod.rs, editor.rs}`
Two files so each stays under ~300 lines (`ui/mod.rs` design guardrail).

**List** — plain `ScrollArea::vertical().show()`. **Not `show_rows()`**: rows are
non-uniform, and LL-G `rust/egui-show-rows-uniform-height` (MEDIUM) says the
`row_height × count` arithmetic desyncs the scrollbar and shifts rows under the
cursor. Each row: `⚡ phrase` (monospace) · scope chip ("Whole dictation" /
"Anywhere") · whitespace-flattened one-line expansion preview (reuse the
truncation shape of [`row::preview`](crates/hark-app/src/ui/history/row.rs:121))
· Edit · Delete. A non-arming entry gets an inline `WARNING` + reason.

**Editor** — a draft struct with an explicit **Save** / Cancel / Delete.
**Do not commit on `lost_focus`** the way
[dictionary.rs:104-109](crates/hark-app/src/ui/dictionary.rs:104) does: focus is
lost by clicking a scrollbar or alt-tabbing, and every commit runs
`save_to_disk` + `pipeline.start()` — a full hook/worker/capture restart
including a keychain read.
- Trigger: single-line `TextEdit` with inline validation ("needs at least two
  words", "already used by another invocation") that disables Save.
- Scope: two radio buttons with plain-language labels and a one-line consequence
  note ("Anywhere also means this dictation skips your cleanup voice").
- Expansion: `TextEdit::multiline().desired_rows(6)`, injected byte-for-byte.
- **Test panel**: "Type what you'd say" → `✓ Would fire` / `✗ Would not fire`,
  plus `Expander::closest()` for the near-miss hint. Rebuild the preview
  `Expander` **only when a `preview_dirty` flag is set**, never per frame.
- Any expandable group uses a **one-shot** open flag
  (`let force = self.force_open.then_some(true); self.force_open = false;`) —
  LL-G `rust/egui-collapsingheader-controlled-open-latch` (MEDIUM): passing
  `Some(state)` every frame makes clicks look broken.

**Empty state** — mirrors
[dictionary.rs:76-88](crates/hark-app/src/ui/dictionary.rs:76): 40 px weak
`LIGHTNING`, subheading, and a "You say → Hark types" two-column example.

**Persistence** — a `pages.rs` sibling of
[`fn dictionary`](crates/hark-app/src/ui/pages.rs:117), with all four
obligations in order:

```rust
if views.invocations.show(ui, &mut settings.invocations) {
    views.invocations.set_notice(settings::save_to_disk(settings).err());
    pipeline.start(settings, ui.ctx());
    views.settings.draft.invocations = settings.invocations.clone();
}
```

That last line is load-bearing: without it a later Settings **Save** writes a
stale draft and resurrects deleted invocations — silent data loss, no error.

### History badge
[`row.rs`](crates/hark-app/src/ui/history/row.rs:50) caption gains a
`⚡ Invocation` segment when `entry.invocation.is_some()`; the expanded details
name the trigger that fired.

---

## 7. File-by-file change list

**New**
- `crates/hark-dictionary/src/expander.rs`
- `crates/hark-config/src/invocations.rs`
- `crates/hark-store/migrations/003_entries_invocation.sql`
- `crates/hark-app/src/ui/invocations/{mod.rs, editor.rs}`
- `Docs/features/INVOCATIONS.md` (sibling of `Docs/features/DICTIONARY.md`)

**Modified**
- `crates/hark-dictionary/src/{lib.rs, matcher.rs}` — `mod expander; pub use`;
  `min_jw` parameter
- `crates/hark-config/src/lib.rs` — `mod`/`pub use`, `Settings` field (last) + `Default`
- `crates/hark-pipeline/src/{lib.rs, worker.rs, events.rs}`
- `crates/hark-store/src/lib.rs` + `tests/store.rs`
- `crates/hark-app/src/{app.rs, theme.rs, storage.rs, pipeline.rs}`,
  `src/ui/{mod.rs, pages.rs, shell.rs, history/row.rs}`
- `config/default-config.toml`, `CHANGELOG.md`, `README.md`
- `package.json` + root `Cargo.toml` → **0.19.0**, identical (`release.yml`
  fails the build if they disagree)

**Positional struct fixtures that will break** (they only compile under
`--all-targets`): [store.rs:7-8](crates/hark-store/tests/store.rs:7),
[pipeline.rs:245-246](crates/hark-app/src/pipeline.rs:245),
[storage.rs:177-178](crates/hark-app/src/storage.rs:177) and
[:202](crates/hark-app/src/storage.rs:202),
[row.rs:145-159](crates/hark-app/src/ui/history/row.rs:145),
[worker.rs:250](crates/hark-pipeline/src/worker.rs:250).

---

## 8. Phases

- **CP0 Foundation** — `expander.rs` + `matcher.rs` threshold parameter +
  `hark-config/invocations.rs`. Fully unit-tested, no UI, no pipeline wiring.
- **CP1 Core** — pipeline branch (`expanded_text`, the `Option::filter`),
  `DictationRecord.invocation`, migration 003, store fields, stats fix, all
  fixtures.
- **CP2 Core** — nav, page, editor, persistence, empty state.
- **CP3 Polish** — Test panel + `closest()`, history badge, default-config,
  README, CHANGELOG, Docs.

One commit per CP; each ends with fmt + clippy + tests + CHANGELOG + both
version files.

---

## 9. Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings   # --all-targets is mandatory
cargo test --workspace
```

LL-G `rust/cargo-check-skips-test-targets` (MEDIUM): bare `cargo check`/`clippy`
compile lib+bins only, so the six broken fixtures above would look clean.

**Tests to add** (pure seams, matching the repo's existing style):

`hark-dictionary/src/expander.rs`
- `whole_utterance_trigger_fires_and_returns_only_the_expansion`
- `trigger_inside_a_sentence_never_fires_in_utterance_scope` ← permanent guard
- `anywhere_scope_splices_inline_and_preserves_punctuation`
- `phonetic_near_miss_below_0_90_never_fires` ← permanent guard against a future
  session "unifying the matchers" back down to 0.85
- `access_granite_still_fires_for_access_granted` (the recall case that justifies fuzzy)
- `one_word_phrase_is_skipped_at_build_time`
- `duplicate_phrase_is_first_wins`
- `empty_invocation_set_is_identity`
- Existing `Corrector` tests must pass **unchanged** (proves the 0.85 path is untouched).

`hark-pipeline/src/worker.rs`
- `a_fired_invocation_never_calls_the_cleaner` — a `MockCleaner` with an **empty
  script** (any call panics) plus a hostile `Corrector`; not panicking *is* the
  assertion, and the same test proves dictionary pass 2 never touched the canned text.
- `no_invocation_leaves_the_cleanup_path_identical`

`hark-config` — `from_toml → to_toml → from_toml` round trip with a **multi-line**
expansion and both scope values; a pre-invocations config still loads.

`hark-store/tests/store.rs`
- `migration_003_adds_the_invocation_column` (model on the existing migration test)
- `an_invocation_dictation_counts_spoken_words_not_injected_words`

**Manual, on real hardware** (this machine is coding-only — build/test/lint here,
run the app on Windows/macOS):
1. Add `access granted` / scope = whole dictation / the paragraph expansion. Save.
2. Hold the chord, say "access granted", release → the paragraph appears verbatim,
   footer shows no cleanup, History row carries the `⚡ Invocation` badge.
3. Say "I confirmed access granted for the new tech" → **nothing fires** (utterance scope).
4. Flip that invocation to "anywhere", repeat step 3 → the paragraph splices inline.
5. Set a voice to Professional, repeat step 2 → text is still byte-identical.
6. Delete the invocation, re-open Settings, press Save → it stays deleted.

---

## 10. Lessons Learned / Gotchas

*(Pre-filled from the LL-G/BP sweep; append discoveries during implementation and
route anything new to LL-G via `/add-lesson`.)*

- **Protect canned text with control flow, never with a prompt clause.** Every
  built-in voice carries `LENGTH_DISCIPLINE_CLAUSE` telling the model *not* to
  expand a short remark into a paragraph — it would fight the expansion.
- **`over_expanded`'s grace floor dominates short inputs.** `max(words × ratio,
  words + 3)` means a 3-word utterance can never legitimately yield 60 words, at
  any ratio. Never let an expansion reach the guard.
- **Never log phrases or expansion text.** `Invocation` must derive `Debug`
  (because `Settings` does), so a stray `log::debug!("{settings:?}")` would dump
  every expansion to disk. Counts, char lengths, millis, and indices only.
- **LL-G `rust/phonetic-code-equality-needs-confirm-guard` (HIGH)** — reuse the
  guarded path; never reimplement Double Metaphone matching without the ≥4-char /
  all-alphabetic gate and the Jaro-Winkler confirm.
- **LL-G `rust/egui-show-rows-uniform-height` (MEDIUM)** — non-uniform rows need
  plain `ScrollArea::show()`.
- **LL-G `rust/egui-collapsingheader-controlled-open-latch` (MEDIUM)** — one-shot
  open flag, not `Some(state)` every frame.
- **LL-G `rust/cargo-check-skips-test-targets` (MEDIUM)** — always `--all-targets`.
- **LL-G `sqlite/upsert-by-name-collision` (MEDIUM)** — the phrase is the identity;
  duplicate rule is first-wins, decided and tested up front.
- **LL-G `architecture/transient-cache-without-drift` (MEDIUM)** — the in-memory
  `Expander` is derived, rebuilt on every pipeline start; TOML is the sole owner.
- **LL-G `rust/libsqlite3-sys-msrv-cfg-select` (HIGH)** — do not bump
  `rusqlite`/`libsqlite3-sys` while adding migration 003.
- **BP `safety/versioned-config-migration-backup`** — this change is additive
  under `#[serde(default)]`, so `CONFIG_VERSION` stays 1; the first bump must
  ship backup-then-migrate.
- **`Docs/` is generated** (`Docs/_toc.yaml` + AUTOGEN markers, via the
  `doc-sync` skill), not hand-written. Add the `INVOCATIONS.md` page to
  `_toc.yaml` and re-run `/doc-sync`. Note
  [DESKTOP_UI.md:31](Docs/features/DESKTOP_UI.md:31) hard-codes *"four egui pages
  (History, Dictionary, Stats, Settings)"* and goes stale the moment this lands.
- **Deferred on purpose:** placeholders/variables (`{date}`, `{cursor}`) in
  expansions. They break the "injected byte-for-byte as authored" invariant this
  design's safety argument rests on; if they ever land, the escaping rule must be
  decided before any user data exists.
- **Deferred on purpose:** sending trigger phrases as STT bias/keyterm hints.
  Biasing raises recall but also raises the odds the provider hallucinates a
  trigger out of similar-sounding audio — the wrong direction when a false fire
  pastes a paragraph. The zero-cost recall fix is documented instead: because
  matching runs *after* dictionary pass 1, adding a stubborn trigger word to the
  Dictionary repairs the transcript before the match.
