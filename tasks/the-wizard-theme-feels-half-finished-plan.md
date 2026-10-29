# Plan: The wizard theme goes all the way

- **Spec:** [../intent/the-wizard-theme-feels-half-finished/spec.md](../intent/the-wizard-theme-feels-half-finished/spec.md)
- **Intent:** [../intent/the-wizard-theme-feels-half-finished/intent.md](../intent/the-wizard-theme-feels-half-finished/intent.md)
- **Written:** 2026-10-29
- **Owner:** Engineer

All paths are relative to `crates/hark-app/` unless noted. Every checkpoint is one commit. It
must pass `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test -p hark-app`
and `npm run check:claude` before the next one starts. Each commit gets a changelog section
and a version bump per `.claude/rules/commit-changelog.md`. Stage the changelog first, then
commit. The suggested bump is listed per checkpoint; ask if unsure between Minor and Patch.

## Before you start

1. **Check the gates.**
   - `rustup update stable` (rust.md: a stale toolchain hides new lints).
   - `git status`. Other sessions may hold uncommitted edits in `src/tray/*`,
     `src/ui/meetings/*` and `src/app.rs`. This plan touches none of them; keep it that way.
   - Never stage `AGENTS.md` or `agents.md`.
2. **Check whether F2 already landed.** The meeting-speakers work may have landed it first
   (`git log -S "enum Hue" -- crates/hark-app/src/theme.rs`). If so, skip F2 and add only
   Parchment's hue row in F3.
3. **Check Q1 in spec §8.** No font download happens without the product owner's yes. If Q1
   is unanswered, do F1-F3, C2-C4 and P1-P2. Leave F4 and C1 for later.
4. **Check the settings files.** The setup-tips spec (`intent/new-users-cannot-choose-a-transcription-setup/`)
   may also touch `ui/settings/*`. Check `git log` and that spec's plan before P2's Settings
   step.

## Foundation

### F1: Palette owns its status colors and its identity (Patch)

1. **Move status colors into `Palette`.** In `src/theme/palette.rs`, add `danger`, `success`
   and `warning` to `Palette`. Fill them with today's values: LIGHT and SOLARIZED_LIGHT get
   the `LIGHT_*` constants; DARK and SOLARIZED_DARK get `DANGER`, `SUCCESS` and `WARNING`.
   Add `pub(super) const ALL: [&Palette; 4]`.
2. **Make the lookup table-driven.** Rewrite `palette(v)` (`:117-127`) to iterate `ALL`,
   matching `window` against `v.panel_fill`. Keep the `dark_mode` fallback.
   - In `visuals()`, set `warn_fg_color` and `error_fg_color` from `p.warning` and `p.danger`.
3. **Read status colors from the palette.** In `src/theme.rs`, make `danger()`, `success()`
   and `warning()` read `palette(v)`. Keep the public `DANGER`, `SUCCESS` and `WARNING`
   constants for the tray and overlay. The private `LIGHT_*` constants become palette
   literals.
4. **Drive the tests from `ALL`.** In `src/theme/tests.rs`, replace the hard-coded
   `palettes()` array (`:21-31`) with one built from `palette::ALL`. Add T6.

- **Pattern:** `SOLARIZED_LIGHT` comments, which record why a value differs from upstream.
- **Gotcha: a vacuous test pass.** The old array could miss a palette and still pass. After
  this step, adding a palette without a test row cannot happen.

### F2: Categorical hues and the chip widget (Patch: tokens only, nothing visible yet)

1. **Hue tables.** Create `src/theme/hues.rs` with a `HueInk { ink, wash }` table per palette.
   Use the spec §3.3 starting values. Add `hues: [HueInk; 6]` to `Palette`.
2. **Hue API.** In `src/theme.rs`, add:
   - `pub enum Hue { Blue, Teal, Green, Gold, Rose, Violet }` and `Hue::ALL`;
   - `hue(v, Hue)` and `hue_wash(v, Hue)`;
   - `speaker_hue(slot)`, if the speakers spec has fixed the order. Otherwise leave a
     `// reserved for meeting speakers` doc line and no function.
   - **hark-app is a binary crate (no `lib.rs`), so an unused `pub` item still trips
     `dead_code`, and `-D warnings` fails CI.** Either land F2 with its first consumer (the
     tests alone do not count for non-test builds), or put `#[allow(dead_code)]` on the new
     items with a one-line reason, as `theme::icons` does (`theme.rs:28`). Remove it in P1.
3. **Chip widget.** Add `pub fn chip(ui, hue, glyph: Option<&str>, text) -> Response`.
   - It is a `Frame` with fill `hue_wash`, corner radius `CHIP_RADIUS`, and padding
     `CHIP_PADDING` (new tokens).
   - Its text is `ink`. Any glyph goes through `icon_text`, never `RichText::new(icons::X)`
     (`crates/hark-app/CLAUDE.md`).
4. **Tests.** T3 (extended to inks), T4 and T5. Port the OKLab conversion from the spec
   session's check script. The math is in spec §3.3. The test needs no new crate.

### F3: Parchment (Minor)

1. **Palette.** In `palette.rs`, add `PARCHMENT` (spec §3.1) and its row in `hues.rs`, then
   add it to `ALL`.
2. **Appearance.** In `src/theme/appearance.rs`:
   - Add `Appearance::Parchment`, with label "Parchment", key `"parchment"` and preference
     `Light`.
   - Replace the `solarized` bool in `install` (`:81-96`) with
     `fn pair(self) -> (&'static Palette, &'static Palette)`. Parchment returns
     `(&PARCHMENT, &PARCHMENT)`, so a stray dark resolution still draws Parchment.
3. **Tests.** T1 and T2. `every_appearance_survives_eframe_memory_serialization_and_startup`
   covers Parchment once it is in `ALL`. Extend `switching_back_to_system_restores_both_neutral_palettes`
   to Parchment.

- **Gotcha: LL-G `eframe-set-theme-clobbers-persisted-preference`.** `install` may only call
  `set_theme` with the *resolved* appearance's preference. Never write a default first. The
  existing `apply_preserves_a_restored_theme_preference` test must stay green.
- **Gotcha: older builds.** A downgraded Hark reading `"parchment"` falls back to the egui
  preference (Light). `legacy_and_unknown_palette_preferences_keep_the_existing_choice`
  already pins that.

### F4: Fonts and the spike (Patch; blocked on Q1)

1. **Download and record the fonts.** After a yes on Q1, download the spec §8 Q1 files from
   `google/fonts` `main` into `assets/`. Record the google/fonts commit hash, the download
   date and the license in `assets/README.md`, matching the existing rows. Add
   `LICENSE-Alegreya.txt` and `LICENSE-CinzelDecorative.txt` (the `OFL.txt` contents).
   **Do not subset or modify the files** (spec §8 A3: Cinzel's Reserved Font Name).
2. **Spike the variable font.** Write a throwaway test, kept only if useful. It registers
   `Alegreya[wght].ttf` three times with `FontTweak { coords: wght=400/500/600, .. }`, lays
   out the same string in each, and asserts the widths differ (Medium is wider than Regular).
   - If they are equal, `coords` is not applied. Switch to the Spectral fallback (static
     files) and record why in the README.
   - Check that the `VariationCoords` construction compiles against egui 0.36. The docs.rs
     page for `FontTweak::coords` does not show its shape.
3. **Add the coverage test (T9).** Build a probe family that holds only the face under test,
   then assert `has_glyph` for every character in the spec AC7 coverage string, for the body
   face and the heading face.

- **Gotcha: LL-G `egui-companion-crate-lag`.** Vendor the files; do not add a font crate.
- **Gotcha: weight files.** The README says static per-weight files are deliberate
  (emilk/egui#1862). That predates egui 0.34's skrifa rasterizer and `coords`. Update the
  README paragraph to say which route was taken and why.

## Core

### C1: The lettering switch (Minor; needs F4)

1. **`src/theme/lettering.rs` (new).**
   - `Lettering { Modern, Wizard }` with `ALL` and `label()`.
   - Store it with `insert_persisted` under `Id::new("hark-lettering-v1")`.
   - `lettering(ctx)` returns the saved value, or the derived one (Wizard if the appearance is
     Parchment). Unknown keys fall back to derived.
   - `set_lettering(ctx, l)` persists the choice and reinstalls the fonts.
   - `lettering_toggle(ui)` is a checkbox labeled "Wizard lettering".
2. **`src/theme/fonts.rs`.**
   - `font_definitions(lettering)`. Register all font data once, unconditionally, so a switch
     is only a family-list change.
   - Rename the family keys to the roles `"Medium"`, `"SemiBold"` and `"Serif"`. `medium()`,
     `semibold()` and `serif()` keep their names.
   - Wizard families are `[Alegreya*, Phosphor, Inter, …defaults]`, plus `Serif` as
     `[CinzelDecorative, Phosphor, …]`. `Icon` is unchanged.
3. **`src/theme.rs`.**
   - `apply()` reads `lettering(ctx)` *after* `appearance(ctx)`, then calls
     `set_fonts(font_definitions(l))` and `text_styles(l)`.
   - `text_styles(lettering)` returns today's map for Modern and spec §4.1 for Wizard. Under
     Wizard, Heading becomes `FontId(TITLE_SIZE, serif())`.
4. **`src/theme/appearance.rs`.** `set_appearance` recomputes the effective lettering. It
   reinstalls fonts only when the lettering changed, because `set_fonts` rebuilds the atlas.
5. **`src/ui/settings/preferences.rs`.** In `appearance_section` (`:58-69`), add the lettering
   toggle and one weak help line.
6. **`src/ui/shell.rs`.** In the overflow test (`:204-250`), loop over `Lettering::ALL` inside
   the `Appearance::ALL` loop (T10).
7. **Tests.** T7, T8 and T10. Update `type_scale_matches_the_spec` to take a lettering and
   `every_family_resolves_and_leads_with_the_intended_font` to cover both.

- **Gotcha: a test that only proves the API exists.** A lettering test that sets and reads
  the value in one `Context` proves nothing about restart. Use the `MemoryStorage`
  serialization round trip from `appearance.rs:122-157`.
- **Gotcha: re-check AC6 after the rename.** Phosphor is second in every text family, but
  Wizard puts Inter third. Inter's private-use glyphs only matter if an icon renders in a
  text family, and `icon_text` forbids that. Re-run AC6 after the rename.

### C2: The shared book (Minor once a page uses it; commit with C3 if preferred)

1. **`src/ui/book/paginate.rs` (new, pure).**
   - `paginate(heights: &[f32], capacity: f32, gap: f32) -> Vec<Range<usize>>`.
   - `spread_of(anchor, pages, per_spread) -> usize`.
   - `clamp_anchor(anchor, len) -> usize`.
   - No `Ui`. T12 runs here.
2. **`src/ui/book.rs` (new).**
   - `BookView { List, Book }`, persisted with `insert_persisted` under a caller-given `Id`.
   - `view_toggle(ui, id, &mut BookView)`, built from two `theme::nav_button`s with
     `icon_label_job`.
   - `Book { anchor }` and `Book::show(ui, entries_len, measure, paint)`. It lays out the
     spread (one or two pages by `BOOK_SPREAD_MIN_WIDTH`), the running head, the folio and
     the gutter, plus the page-turn row with `focus_ring` and the keyboard handling.
3. **Keyboard.**
   - Consume PageUp, PageDown, Home and End with `i.consume_key(Modifiers::NONE, ..)` when
     `!ctx.egui_wants_keyboard_input()`.
   - Do this **before** any page `ScrollArea` is built, or the inner area may take the key.
   - Leave the arrow keys to egui focus navigation.
4. **Tokens in `src/theme.rs`.** `BOOK_PAGE_MIN_WIDTH`, `BOOK_SPREAD_MIN_WIDTH`,
   `BOOK_GUTTER`, `BOOK_PAGE_PADDING`, `BOOK_MIN_HEIGHT`, `BOOK_ENTRY_GAP`, `DROP_CAP_SIZE`,
   `book_gutter(v)` and an ornament helper beside `paint_fading_rule`.
5. **Icons.** Add `LIST_BULLETS`, `CARET_LEFT`, `CARET_RIGHT` and `PENCIL_SIMPLE` to
   `theme::icons`. Copy the codepoints from egui-phosphor 0.12.0
   `src/variants/regular.rs` (docs.rs source view). Never guess them.

- **Gotcha: LL-G `egui-scrollarea-shared-id-salt`.** The list `ScrollArea` and every book
  page `ScrollArea` share the page's parent `Ui`. Salt each one (`"spellbook-list"`,
  `"book-page-<n>"`), or toggling views opens the book at the list's scroll offset.
- **Gotcha: LL-G `egui-show-rows-uniform-height`.** Entries vary in height. Never use
  `show_rows` for the book or for an over-tall page.
- **Gotcha: measure what you paint.** Measuring and painting must use the same galleys.
  `measure` returns the laid-out galleys and `paint` draws those exact galleys. A second
  layout with different wrap widths makes pages overflow by a line, with no error.

### C3: The Spellbook book (Minor)

1. **`src/ui/spellbook/mod.rs`.**
   - Lift the term edit and commit block out of `entry_list` (`:259-292`) into
     `pub(super) fn edit_term(&mut self, ui, entries, index) -> bool`.
   - `alias_editor` (`:335-380`) is already per-index. Make it `pub(super)`.
   - Add the toolbar row with `view_toggle(.., Id::new("hark-spellbook-view-v1"), ..)`.
   - Keep the add row, the Undo row and the notice above both views.
2. **`src/ui/spellbook/book.rs` (new).**
   - `measure` and `paint` for an entry: drop cap in `hue(Violet)`, then the term in the serif
     role, then "Also heard as" with the monospace aliases, then the Edit and Delete row.
   - Edit sets the same `self.edit` and `edit_needs_focus` the list uses, and calls
     `edit_term` and `alias_editor` in place. While `self.edit` or `self.expanded` is `Some`,
     pin `anchor` to that index.
3. **History handoff.** After `prime_add` and a successful add, set `anchor` to the new entry's
   index (T15). Show the "New" chip on the entry whose term equals `last_added`.

- **Gotcha: LL-G `egui-collapsingheader-controlled-open-latch`.** If the alias editor is
  wrapped in a `CollapsingHeader` in the book, force it open for one frame only.
- **Gotcha: delete the guards together.** A pending delete is applied after the loop
  (`:326-331`). The book must defer deletes the same way, or indices shift mid-frame.

### C4: The Invocations book (Minor)

1. **`src/ui/invocations/mod.rs`.**
   - Extract the `Outcome` match in `show_editor` (`:182-211`) into
     `fn apply_outcome(&mut self, entries, Outcome) -> bool`, which both views call.
   - Add the toolbar row with the view toggle (`hark-invocations-view-v1`).
2. **`src/ui/invocations/book.rs` (new).**
   - `measure` and `paint` per entry: drop cap in `hue(Rose)`, the phrase, a scope chip, the
     aliases, the full expansion (line breaks kept), the `skip_reason` warning and the action
     row.
   - When `draft.is_some()` and the view is Book, call `Draft::show` inside the spread's full
     width, in place of the two pages, with the page-turn row disabled.
   - On `Saved` or `Deleted`, call `apply_outcome`, then set `anchor` to the saved index.

- **Gotcha: commit timing.** `Draft` commits only on an explicit Save, because each commit
  restarts the pipeline (`editor.rs:3-8`). The book must not add a lost-focus commit.

## Polish

### P1: App chrome color (Minor)

1. **`src/ui/pages.rs`.** Add `Page::hue() -> Option<theme::Hue>` (Settings returns `None`,
   meaning accent). In the header block (`:110-117`), put the page icon, as `icon_text` in
   the hue, before `ui.heading`. Add a `HEADER_RULE_WIDTH` × 2 px rule under the title.
2. **`src/ui/navigation.rs`.** Tint the nav icon by `Page::hue()`. `icon_label_job` uses
   `PLACEHOLDER` color, so build a two-color job, or tint only the icon section. Keep the
   label in text color.

### P2: Page-level color (Minor)

Work in this order, one page at a time, each with a quick look at the overflow test:

1. `ui/home.rs`: status icon by state.
2. `ui/stats/panels.rs` and `charts.rs`: tile icons and series hues.
3. `ui/history/row.rs`: provider chip and Invocation chip. The provider label is already in
   `entry.stt_provider`.
4. `ui/spellbook/mod.rs` and `ui/invocations/mod.rs`: list chips.
5. `ui/settings/sections.rs`, `keys.rs` and `local.rs`: section icon hue table and provider
   chips.

- **Rule: every hue needs words.** Each hued element must already have, or gain, text. Hue
  never sits beside a CHECK, WARNING or X glyph.
- **Rule: no ad-hoc colors.** No `Color32` literal enters `ui/`. Grep for
  `Color32::from_rgb` under `src/ui` before committing.

### P3: On-hardware review (no commit unless fixes)

Run H1-H6 on Windows first (platform priority), then macOS. Linux only needs to compile and
pass tests.

## Ship

- **Changelog.** One `## [x.y.z]` section per commit (see the bump per checkpoint). Write user
  wording, such as "New Parchment theme", "Wizard lettering switch" and "Book view for the
  Spellbook and Invocations".
- **Docs.** Update `crates/hark-app/CLAUDE.md`:
  - It says egui 0.35; the workspace pins 0.36.
  - Add a "Lettering" note: role-named families, never subset the bundled fonts, and Wizard
    lettering is an independent switch.
- **Intent and spec.** Update the intent's Lessons Learned and the spec's Status line.

## Test plan

All tests are headless, using `Context::default()` and `run_ui`, like the existing tests. No
new test crate.

| ID | Where | What it proves |
|---|---|---|
| T1 | `theme/appearance.rs` tests | Every appearance, Parchment included, survives eframe memory serialization. Parchment installs Parchment in both slots, and its preference is Light |
| T2 | `theme/appearance.rs` tests | System after Parchment or Solarized restores LIGHT and DARK |
| T3 | `theme/tests.rs` | For every `palette::ALL` entry, text, weak, status, accent and all six inks are at least 4.5:1 on window, panel, surface, sidebar and tint |
| T4 | `theme/tests.rs` | Ink on wash is at least 4.5:1; on_accent on accent is at least 4.5:1; the focus ring is at least 3:1 and 2 px wide |
| T5 | `theme/tests.rs` | Within each palette, the six inks are pairwise at least 0.06 apart in OKLab |
| T6 | `theme/tests.rs` | `palette(&visuals(p))` returns `p`; window colors are unique; every `Appearance::pair()` member is in `ALL` |
| T7 | `theme/lettering.rs` tests | Derived default (Parchment gives Wizard, others Modern); an explicit choice survives the serialization round trip and an appearance change; an unknown key gives the derived value; `set_appearance` to Parchment with no saved choice swaps the families |
| T8 | `theme/tests.rs` | Both letterings: every text family leads with its face and Phosphor is second; `Icon` leads with Phosphor; the Modern lists equal today's |
| T9 | `theme/tests.rs` | The coverage string resolves in the Wizard body and heading faces alone (probe families) |
| T10 | `ui/shell.rs` | No text overflow at 480, 860 and 1020 for every (appearance, lettering) pair |
| T11 | `ui/book.rs` tests | View choice defaults to List, persists per page `Id` across serialization, and the two pages are independent |
| T12 | `ui/book/paginate.rs` tests | Empty input gives one empty page. A fitting set gives one page. Overflow splits. A tall entry sits alone. Every index appears exactly once, in order, over 200 seeded random height sets (inline LCG, no new crate). `spread_of` is correct for 1 and 2 pages per spread. `clamp_anchor` handles delete-to-empty |
| T13 | `ui/book.rs` tests | A PageDown event advances `anchor` to the next spread; with a focused `TextEdit` it does not. Home and End reach the ends |
| T14 | `spellbook/edit.rs`, `invocations/mod.rs` tests | Book and list share `edit_term` and `apply_outcome`; pure tests on those prove both views produce the same `entries` |
| T15 | `ui/spellbook` tests | After `prime_add` and add in book view, `anchor` equals the new entry's index |
| T16 | `ui/pages.rs` tests | The six hued pages have distinct hues; `chip` output contains its text in the shapes |

**On-hardware checklist (H).** Run it on Windows at 100%, 125% and 150% scaling, then on macOS.

- **H1. Parchment.** It looks like parchment, not Solarized. The focus ring is visible on
  every surface.
- **H2. Wizard lettering.** Body text is readable at Body size, and headings in Cinzel
  Decorative read clearly. Flip to Modern on Parchment, and back.
- **H3. Book view.** It works both as a two-page spread and as a single page. Test
  PageUp, PageDown, Home and End, and Tab through the controls. The focus ring is visible.
- **H4. Editing in the book.** Edit a spell in place and save an invocation from the spread.
  Restart, and the edits persist.
- **H5. Color in other themes.** Check the color pass in Dark and Solarized Dark. Each chip is
  readable, and no status relies on color alone.
- **H6. History handoff.** Select text in History, send it to the Spellbook, and the book opens
  to the new term.

## Rollback

- **Revert order.** Each checkpoint is one commit. Revert in reverse order (P → C → F). F1 and
  F2 change no visuals, so they can stay even if everything else is reverted.
- **No data migration.** Nothing touches `config.toml` or the database.
- **Persisted keys.** The added egui-memory keys (`hark-lettering-v1`,
  `hark-*-view-v1`) are ignored by builds that do not know them. A saved `"parchment"`
  appearance falls back to Light on an older build, which is already pinned by a test.
- **Font removal.** Removing the fonts means reverting C1 and F4 together. F4 alone leaves
  `include_bytes!` paths that no longer resolve.
- **Speakers dependency.** If the speakers work has consumed `Hue`, do not revert F2. Revert
  the consumers instead.

## Lessons Learned / Gotchas

Gotchas already known, from LL-G and this exploration:

- **Startup must not clobber the saved theme.** LL-G `eframe-set-theme-clobbers-persisted-preference`
  applies to lettering too. Never write a default lettering at startup; derive it instead.
- **Sibling scroll areas need their own `id_salt`.** List and book `ScrollArea`s share a
  parent `Ui` (LL-G `egui-scrollarea-shared-id-salt`).
- **`show_rows` assumes uniform rows.** It breaks on variable-height entries (LL-G
  `egui-show-rows-uniform-height`).
- **`CollapsingHeader` with `open(Some)` every frame locks it.** Force it open for one frame
  only (LL-G `egui-collapsingheader-controlled-open-latch`).
- **Inter's private-use glyphs shadow Phosphor.** Inter's PUA collisions with Phosphor are
  why every icon goes through `icon_text`; the rule applies in both letterings.
- **Subsetting breaks the license.** A Reserved Font Name (Cinzel) means a subset font cannot
  ship under its name. Ship the files unmodified.
- **The palette lookup can drop a theme silently.** `palette()` reverse-looks-up by window
  color. Two palettes sharing a window color would make one theme silently render as the
  other; T6 pins it.

To fill in during and after implementation:

- [ ] Gotchas encountered → LL-G via `/add-lesson` (not sent yet; listed in the intent's
  Lessons Learned): variable-font `coords` work in egui 0.36; `has_glyph` is always false in a
  one-face family; `egui_wants_keyboard_input()` blocks page keys after a button click.
- [ ] Reusable patterns → BP via `/add-practice` (not sent yet): per-theme categorical hue sets
  with contrast and distinctness tests; anchor-based pagination that remembers painted heights.
- [ ] Repo-only notes → `.claude/agent-memory/patterns.md`.
- [x] Failed approaches: none. The variable-font spike passed (widths 268.8, 270.8 and 272.1 pt
  at wght 400/500/600), so the Spectral fallback was not needed.
- [x] Deviations, with reasons: page keys use `text_edit_focused()`, because
  `egui_wants_keyboard_input()` is true for any focused widget; Cinzel's two unmapped characters
  fall back to Alegreya; drop caps are drawn as capitals because phrases are usually lower-case;
  the shell overflow test kept its existing window sizes and runs every (appearance, lettering)
  pair; the provider chip sits on its own row above a key's status line, so a hue never shares
  a row with a status glyph.
