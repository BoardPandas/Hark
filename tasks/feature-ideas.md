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
| F-001 | Color-code speakers in meeting transcripts | Meetings | Complete (0.66.0) |
| F-002 | Move a single transcript line to a different speaker | Meetings | Complete (0.66.0) |
| F-003 | Accent color choice on top of each theme | Look and feel | Incomplete |
| F-004 | Parchment / Spellbook theme | Look and feel | Complete (0.67.0) |
| F-005 | Font toggle: wizard pen or modern app font | Look and feel | Complete (0.67.0) |
| F-006 | "Book" view for the Spellbook and Invocations | Spellbook | Complete (0.67.0) |
| F-007 | Wizard level setting: Full Wizard, Half-Wizard, Non-Wizard | Whole app | Incomplete |

## Meetings

### F-001 Color-code speakers in meeting transcripts

- **Status:** Complete (0.66.0)
- **Raised by:** Brandon and Chaz, 2026-10-29
- **Idea:** Give each speaker their own color in a meeting transcript so it is easier to see
  who said what at a glance.
- **Shipped:** Every speaker, "Me" included, has a color and a name you can change. Lines carry a
  colored bar and a speaker button, notes show colored names (0.64.8), and Word exports color
  names. Hark also guesses names it hears in the call ("Hi, this is Sam"), and Settings >
  Meetings has a "Your name" field for Me.
- **Plan:** [meeting-speakers-are-hard-to-tell-apart-plan.md](meeting-speakers-are-hard-to-tell-apart-plan.md),
  spec in `intent/meeting-speakers-are-hard-to-tell-apart/`.
- **Left out on purpose (candidate follow-ups):** recognizing a voice across meetings, speaker
  colors in the live call pane (still Me/Them), and color in Markdown, plain text and SRT
  exports (names carry identity there).

### F-002 Move a single transcript line to a different speaker

- **Status:** Complete (0.66.0)
- **Raised by:** Brandon and Chaz, 2026-10-29 (part of F-001)
- **Idea:** When the AI gets a line wrong (it says Speaker 2 said it, but it was definitely
  Speaker 3), let the user reassign just that line.
- **Shipped:** Click a line's speaker button to move it to yourself, anyone else in the
  meeting, or a new speaker. A moved line shows who it was transcribed as, and "As transcribed"
  puts it back. Moves save right away and survive a restart. Notes are rewritten after moves
  (0.64.7).
- **Answered questions:**
  - Exports read speakers through the corrected roster (0.64.4).
  - A rerun of the recording resets names and guesses again. The existing notes are baked first
    so they keep the old names.
- **Left out on purpose (candidate follow-ups):** splitting one line between two speakers,
  moving a range of lines at once, and filtering the transcript by speaker.

## Look and feel

### F-003 Accent color choice on top of each theme

- **Status:** Incomplete (the "too monochromatic" half is addressed; the accent picker is not built)
- **Raised by:** Chaz, 2026-10-29
- **Idea:** The app feels too monochromatic in every theme. Add pops of color the way the TCG
  portal does it: the theme sets the overall surfaces, and the user picks an accent color.
- **What shipped instead (0.66.0, 0.68.0):** Each theme gained six checked colors (blue, teal,
  green, gold, rose, violet), and every page now has its own color: Home green, Insights blue,
  History gold, Meetings teal, Spellbook violet, Invocations rose. Provider chips, colored
  icons and colored Insights bars came with it. The accent is still fixed per theme; there is
  no user choice.
- **Still to decide:** Now that pages are colorful, is a user-picked accent still wanted? If
  yes, it changes only the theme's `accent` (buttons, focus ring, links, Settings).
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

- **Status:** Complete (0.67.0)
- **Raised by:** Brandon, 2026-10-29
- **Idea:** A warm parchment theme that looks and feels like a spellbook.
- **Shipped:** Parchment, a sixth theme: aged paper, dark-brown ink and lapis-blue accents. It
  is light only and stays light when the computer is in dark mode.
- **Plan:** [the-wizard-theme-feels-half-finished-plan.md](the-wizard-theme-feels-half-finished-plan.md),
  spec in `intent/the-wizard-theme-feels-half-finished/`.
- **Left out on purpose (candidate follow-ups):** a dark, candle-lit Parchment (decided
  2026-10-29), and theming the recording pill and tray icon.

### F-005 Font toggle: wizard pen or modern app font

- **Status:** Complete (0.67.0)
- **Raised by:** Brandon, 2026-10-29 (part of F-004)
- **Idea:** On the Parchment theme, a toggle between a "wizard pen" handwriting font and the
  normal modern app font.
- **Shipped:** A "Wizard lettering" checkbox in Settings > Appearance. Body text uses
  Alegreya, and titles and the Home headline use Cinzel Decorative (both bundled, SIL Open Font
  License). Key names and timings stay monospace.
- **Differs from the idea:** It works on **every** theme, not just Parchment. Parchment turns it
  on until you choose for yourself; after that your choice holds across themes.
- **Still open:** Hidden in Non-Wizard mode once F-007 exists.

## Spellbook

### F-006 "Book" view for the Spellbook and Invocations

- **Status:** Complete (0.67.0)
- **Raised by:** Brandon, 2026-10-29
- **Idea:** A view toggle that shows the Spellbook and Invocations as a real book: a center
  binding, facing pages, and page turning. About 3 to 4 spells, or 6 to 8 invocations, per page.
- **Shipped:** A List | Book switch on both pages (List stays the default, and the choice is
  remembered). Wide windows show two facing pages, narrow ones show one. Entries start with a
  large colored capital. Turn pages with Previous and Next or Page Up, Page Down, Home and End.
  Editing happens in the book, and a newly added entry opens to its page marked "New".
- **Left out on purpose (candidate follow-ups):** a page-turn animation, search inside the
  book, and printing or exporting the book.
- **Still open:** Hidden in Non-Wizard mode once F-007 exists.

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
  Parchment, Wizard lettering and the book views (F-004 to F-006) are now the building blocks
  Full Wizard would start from.
- **Notes:**
  - This is really two projects. The wording layer (Half-Wizard vs Non-Wizard) is fairly
    small. Full Wizard is a large redesign and should get its own plan.
  - Only the words on screen change. Settings files and storage keep their current names, so
    switching levels never loses data.
  - Non-Wizard needs a Grid view, which does not exist yet.
