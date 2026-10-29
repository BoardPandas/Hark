# Intent: The wizard theme should go all the way

- **Status:** Approved
- **Originator:** Chaz Vossburg
- **Captured:** 2026-10-29
- **Approved by:** Chaz Vossburg (product owner), 2026-10-29 ("Approve all 3")

## Problem

Hark calls its parts the Spellbook and Invocations, but the app mostly looks like a plain
settings tool. Nothing in the app feels like a spellbook. Solarized Light comes closest to a
parchment look, but it is not right. Every theme leans on neutral greys, with few pops of
color on any page.

## Proposed outcome

- A Spellbook view that reads like a book: you turn pages, and the spells are written out
  as if in a book. Invocations could get one too.
- A Parchment theme.
- A separate switch between full wizard lettering and a modern font on parchment colors.
- More pops of color on every page and at every level of the UI, in **all** themes, not
  only the wizard ones.

## Affected users and systems

- **Users:** the single Hark user, every time the window is open.
- **Systems:** themes and fonts, the Spellbook and Invocations pages, and every page for
  the color pass.

## Constraints

- Text must stay readable. The project's accessibility rules require contrast minimums and
  never conveying status by color alone.
- The existing themes and the System (follow the OS) option must keep working.

## Decisions (originator, 2026-10-29)

- **The book view is optional.** A toggle switches between it and the current list; it
  does not replace the list.
- **Invocations get a book view in this round too.**
- **Wizard lettering covers headings and body text.** It must stay readable at body size;
  the separate switch to a modern font is the escape hatch.
- **Spells and invocations can be edited in the book itself.**

## Out of scope

- A dark (candle-lit) Parchment variant (decided 2026-10-29).

## Open questions

None outstanding.
- Can the wizard lettering be used with other themes, or only with Parchment? Owner:
  product owner.

## Lessons Learned / Gotchas

Recorded 2026-10-29, when the work shipped. Candidates for LL-G are marked; none has been sent yet.

- **egui 0.36 can use a variable font for several weights.** `FontTweak::coords` is applied when shaping and when rasterizing, so one Alegreya file registered three times with `wght` pinned gives three real weights. The old "egui cannot interpolate weights" rule predates the skrifa rasterizer. (LL-G candidate.)
- **`has_glyph` cannot test a single font.** In a family of one face, that face is also the replacement face, so `has_glyph` says false for every character. Read the charmap from `Font::characters()` instead. (LL-G candidate.)
- **`egui_wants_keyboard_input()` is true for any focused widget**, not only text fields. Gating page keys on it disabled them after one click on Next; `text_edit_focused()` is the right test. (LL-G candidate.)
- **Cinzel Decorative maps neither the no-break space nor µ**, and its licence forbids patching it. The serif family falls back to Alegreya for those two, and a test pins the gap.
- **Pagination in an immediate-mode UI can measure what it paints by remembering painted heights.** The book uses each entry's real height once drawn and discards the frame when a guess was wrong, so open editors never overflow a page silently. (BP candidate.)
- **Hue answered "more color" without blurring state.** Six categorical hues per palette, each tested for contrast and OKLab distance, kept apart from status colors by rule and by never pairing a hue with a status glyph. (BP candidate.)
