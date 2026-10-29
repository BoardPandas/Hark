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

(Filled in after the work ships. Route anything generalisable to LL-G with `/add-lesson`.)
