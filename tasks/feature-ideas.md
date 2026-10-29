# Hark Feature Ideas

A living checklist of features we would like to add. Ideas land here first. An idea that gets
built moves through a plan in `tasks/` like any other work, and this file links to it.

## How to use this file

- Add a new idea with the next free ID, a row in the summary table, and a detail section.
- Change an idea's status in **both** the summary table and its detail section.
- Never delete an idea. Mark it **Sidelined** or **Denied** and write down why, so the same
  discussion does not have to happen twice.
- When work starts, link the plan file. When it ships, record the version it shipped in.

### Statuses

| Status | Meaning |
|---|---|
| Incomplete | Wanted, not started yet |
| In progress | Being planned or built (link the plan) |
| Complete | Shipped (note the version) |
| Sidelined | Good idea, parked for now (note why and what would bring it back) |
| Denied | Decided against (note why) |

## Summary

| ID | Idea | Area | Status |
|---|---|---|---|
| F-001 | Color-code speakers in meeting transcripts | Meetings | Incomplete |
| F-002 | Move a single transcript line to a different speaker | Meetings | Incomplete |
| F-003 | Accent color choice on top of each theme | Look and feel | Incomplete |
| F-004 | Parchment / Spellbook theme | Look and feel | Incomplete |
| F-005 | Font toggle: wizard pen or modern app font | Look and feel | Incomplete |
| F-006 | "Book" view for the Spellbook and Invocations | Spellbook | Incomplete |
| F-007 | Wizard level setting: Full Wizard, Half-Wizard, Non-Wizard | Whole app | Incomplete |

## Meetings

### F-001 Color-code speakers in meeting transcripts

- **Status:** Incomplete
- **Raised by:** Brandon and Chaz, 2026-10-29
- **Idea:** Give each speaker their own color in a meeting transcript so it is easier to see
  who said what at a glance.
- **Today:** Speakers can be renamed ("Speaker 2" to "Dana") from the meeting's speakers card
  (`crates/hark-app/src/ui/meetings/detail.rs`), but every speaker looks the same.
- **Notes:**
  - Colors must stay readable in every theme, and pair with F-003 if accents become a thing.
  - Color must not be the only cue: keep the speaker name on each line for color-blind users.

### F-002 Move a single transcript line to a different speaker

- **Status:** Incomplete
- **Raised by:** Brandon and Chaz, 2026-10-29 (part of F-001)
- **Idea:** When the AI gets a line wrong (it says Speaker 2 said it, but it was definitely
  Speaker 3), let the user reassign just that line.
- **Today:** Renaming works on a whole speaker, not on one line.
- **Open questions:**
  - Should the fix carry into exports (subtitles, excerpts, Word)? Probably yes.
  - What happens to manual fixes if the recording is rerun? Keep them, or warn first?

## Look and feel

### F-003 Accent color choice on top of each theme

- **Status:** Incomplete
- **Raised by:** Chaz, 2026-10-29
- **Idea:** The app feels too monochromatic in every theme. Add pops of color the way the TCG
  portal does it: the theme sets the overall surfaces, and the user picks an accent color.
- **Today:** Five appearances (System, Light, Dark, Solarized Light, Solarized Dark) in
  `crates/hark-app/src/theme/appearance.rs`, colors in `theme/palette.rs`. No accent choice.
- **Reference:** The TCG design system builds every accent shade (hover, soft tint, ring,
  border) from one hue, so one user choice recolors everything consistently. See
  `~/Repos/tcg/.claude/references/design-system/project/colors_and_type.css`.
  Its MTG colors could be the starting accent set:

  | Color | Hex |
  |---|---|
  | White | `#f4ecd3` |
  | Blue | `#4a8bce` |
  | Black | `#2a2424` |
  | Red | `#d76250` |
  | Green | `#5fa472` |

- **Notes:** White and Black are nearly neutral, so as accents they may vanish on light or dark
  themes. Each accent needs a contrast check against each theme before it ships.

### F-004 Parchment / Spellbook theme

- **Status:** Incomplete
- **Raised by:** Brandon, 2026-10-29
- **Idea:** A warm parchment theme that looks and feels like a spellbook.
- **Notes:** Natural default look for Full Wizard mode (F-007).

### F-005 Font toggle: wizard pen or modern app font

- **Status:** Incomplete
- **Raised by:** Brandon, 2026-10-29 (part of F-004)
- **Idea:** On the Parchment theme, a toggle between a "wizard pen" handwriting font and the
  normal modern app font.
- **Notes:**
  - The font has to be bundled with the app (`theme/fonts.rs`) and its license must allow that.
  - Handwriting fonts get hard to read at small sizes; it may only suit headings and entries.
  - Hidden entirely in Non-Wizard mode (F-007).

## Spellbook

### F-006 "Book" view for the Spellbook and Invocations

- **Status:** Incomplete
- **Raised by:** Brandon, 2026-10-29
- **Idea:** A view toggle that shows the Spellbook and Invocations as a real book: a center
  binding, facing pages, and page turning. About 3 to 4 spells, or 6 to 8 invocations, per page.
- **Notes:**
  - Lives beside today's view as a toggle, not a replacement.
  - Search, add, and edit still need to work in this view, plus keyboard page turns.
  - Hidden entirely in Non-Wizard mode (F-007).

## Whole app

### F-007 Wizard level setting: Full Wizard, Half-Wizard, Non-Wizard

- **Status:** Incomplete
- **Raised by:** Brandon, 2026-10-29
- **Idea:** A choice made at first launch (changeable later in Settings) that sets how magical
  the whole app feels.

  | Level | What it means |
  |---|---|
  | Full Wizard | A full rewrite of the layout around the spellbook: magical themes, look, and feel throughout |
  | Half-Wizard | Roughly what Hark is today: lightly magical names, options, and text |
  | Non-Wizard | Plain business wording. Spellbook becomes "Corrections", Invocations become "Phrases". No Book view (F-006), no wizard font (F-005); only standard views such as Grid and List |

- **Today:** First launch already has an onboarding flow
  (`crates/hark-app/src/ui/settings/onboarding.rs`), so the choice could be one more step there.
- **Notes:**
  - This is really two projects. The wording layer (Half-Wizard vs Non-Wizard) is fairly
    small. Full Wizard is a large redesign and should get its own plan.
  - Only the words on screen change. Settings files and storage keep their current names, so
    switching levels never loses data.
  - Non-Wizard needs a Grid view, which does not exist yet.
