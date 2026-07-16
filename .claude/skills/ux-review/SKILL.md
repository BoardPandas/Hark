---
name: ux-review
effort: medium
description: Review UI/UX code against Laws of UX and Gestalt principles. Produces severity-ranked findings with specific improvement recommendations. Use on frontend repos or targeted components.
user-invocable: true
argument-hint: [optional: file, directory, or component name to scope the review]
agent: ux-reviewer
allowed-tools:
  - Read
  - Glob
  - Grep
---

# UX Review

You have been asked to perform a UX review of the codebase. This is a static, code-level review. Anything that depends on rendered output (computed target sizes, actual response latency, real color contrast) cannot be verified from code alone; flag those items for visual verification rather than asserting them as violations. Follow these steps.

## Step 1: Load Knowledge Base

1. Read `.claude/references/ux-laws.md` to load the full set of 30 UX laws and their code-level indicators. You will reference these throughout the review.
2. If `.claude/references/design-guardrails.md` exists, read it as well. Evaluate findings against the project's own guardrails in addition to the generic laws, and cite the specific guardrail when one is violated. Guardrail violations rank at least WARNING.

## Step 2: Determine Scope

1. If the user specified a file, directory, or component name, scope the review to that target.
2. If no scope was specified, identify all UI-related files in the codebase.
3. Detect the frontend framework (React, Vue, Svelte, Angular, plain HTML, etc.) and note conventions.
4. Exclude ignored and generated paths: everything in `.gitignore`, plus `node_modules`, `dist`, `build`, `out`, `.next`, `.nuxt`, `coverage`, storybook output, and minified or generated CSS/JS.
5. If no UI files remain after exclusions, stop here. Report that the repository has no reviewable UI code and suggest repo-review instead. Do not review backend or tooling code against UX laws.

Use these patterns to find UI files:

```
**/*.tsx, **/*.jsx        (React components)
**/*.vue                  (Vue components)
**/*.svelte               (Svelte components)
**/*.astro                (Astro components)
**/*.html                 (HTML templates)
**/*.css, **/*.scss       (Stylesheets)
**/pages/**, **/views/**  (Page-level components)
**/components/**          (Shared components)
**/layouts/**             (Layout components)
```

### Budget for large codebases

If the scope exceeds roughly 40 files, do not read everything:

- Prioritize pages, layouts, and shared components; these shape the experience most.
- Sample repeated patterns: read one representative card, row, or tile component rather than every instance.
- List every skipped area in the Scope section of the report. Coverage must never silently truncate.

## Step 3: Quick Scan with Grep

Before reading files in depth, run these searches to surface leads. Treat hits as leads to read in context, not as findings by themselves.

| Search for | Law | What it suggests |
|---|---|---|
| `<div onClick`, `<span onClick` | Fitts's Law, Law of Similarity | Non-semantic interactive elements; likely poor hit areas, no keyboard support |
| Identity/address inputs without `autocomplete=` | Parkinson's Law | User retypes data the browser could fill |
| `placeholder=` on inputs with no associated label | Law of Similarity, Working Memory | Placeholder used as label; disappears on focus |
| `window.confirm(`, `alert(` | Tesler's Law, Flow | Raw browser dialogs interrupting the task |
| `fetch(`, mutation hooks with no nearby loading/pending state | Doherty Threshold | Async action without feedback |
| Tiny size utilities on interactive elements (`h-5`, `w-5`, `p-0`) | Fitts's Law | Touch targets likely under minimum size |
| `delete`, `remove`, `destroy` handlers with no undo or confirm | Paradox of the Active User | Destructive action without recovery |

## Step 4: Analyze Navigation and Information Architecture

Review the top-level navigation and page structure:

1. **Hick's Law:** Count top-level navigation items. Flag if more than 7 primary options exist without grouping.
2. **Serial Position Effect:** Check whether the most important items are at the beginning and end of navigation lists.
3. **Miller's Law / Chunking:** Look for long flat lists without grouping, sectioning, or hierarchy.
4. **Jakob's Law:** Compare navigation patterns to industry conventions. Flag non-standard patterns (e.g., hamburger menus on desktop, search bars in footers, unconventional icon usage).
5. **Mental Model:** Check terminology for consistency with industry norms and with itself. The same concept should carry the same name everywhere in the UI.

## Step 5: Analyze Forms and User Input

Find all forms, inputs, and data entry patterns:

1. **Cognitive Load:** Count fields per form. Flag forms with 7+ fields on a single screen without sections.
2. **Chunking:** Check whether long forms are broken into logical groups or steps.
3. **Goal-Gradient Effect:** For multi-step forms, check for progress indicators.
4. **Postel's Law:** Review input validation. Flag overly strict validation that rejects reasonable input formats. Check whether error messages explain expected format.
5. **Parkinson's Law:** Identify unnecessary fields, missing autofill attributes, or missing pre-populated defaults.
6. **Flow:** Check for interruptions mid-form (modals, redirects, forced decisions unrelated to the task).
7. **Choice Overload:** Flag dropdowns or selects with 20+ options and no search or filtering, and filter panels that expose every option simultaneously.

## Step 6: Analyze Interactive Elements

Review buttons, links, and interactive controls:

1. **Fitts's Law:** Check touch/click target sizes where they are explicit in the code. Flag targets under 44px on mobile or 24px on desktop. Check spacing between adjacent interactive elements. If sizes are computed or inherited, route to Needs Visual Verification.
2. **Von Restorff Effect:** Verify that primary CTAs are visually distinct from secondary actions. Flag pages where multiple elements compete for attention equally.
3. **Law of Similarity:** Ensure clickable and non-clickable elements are visually distinct. Links should look like links. Buttons should look like buttons.
4. **Doherty Threshold:** Check for loading states on interactive elements (buttons, form submits). Flag actions that fire without immediate visual feedback.

## Step 7: Analyze Layout and Visual Grouping

Review the spatial organization of UI elements:

1. **Law of Proximity:** Check spacing between related elements. Flag labels far from their inputs, related actions spread apart, or unrelated items packed closely.
2. **Law of Common Region:** Check for visual containers (cards, sections, borders) grouping related content. Flag related content without boundary separation.
3. **Law of Uniform Connectedness:** In stepper/wizard/timeline components, check for connecting visual elements.
4. **Law of Pragnanz:** Flag overly complex visual patterns, charts with too many data series, or layouts with competing visual rhythms.
5. **Cognitive Load:** Evaluate overall density. Flag screens with no visual breathing room or too many competing calls-to-action.

## Step 8: Analyze Feedback and State Communication

Review how the system communicates state to users:

1. **Doherty Threshold:** Check that all async operations (API calls, form submits, file uploads) show loading/progress states.
2. **Peak-End Rule:** Review success states, completion screens, and error pages. Flag bland or missing success feedback. Flag error pages without recovery options.
3. **Zeigarnik Effect:** For multi-step processes, check for progress tracking. Flag flows where users cannot see how far they are or how much remains.
4. **Tesler's Law:** Check whether system complexity is exposed to users. Flag raw error codes, technical jargon, or configuration screens without sensible defaults.

## Step 9: Analyze Flows and Recovery

Review multi-screen flows and how users recover from mistakes:

1. **Working Memory:** Flag flows that require users to remember values from a previous screen: reference codes shown once and needed later, cart or order details that disappear at checkout, settings that must be recalled rather than shown.
2. **Paradox of the Active User:** Check that destructive actions have undo or a meaningful confirmation, and that guidance lives inline (tooltips, helper text, empty states) rather than only in documentation or help pages.
3. **Cognitive Bias:** Verify every error state has a recovery path. Negative moments register more strongly than positive ones; a dead-end error outweighs several pleasant screens.

## Step 10: Analyze Accessibility and Inclusivity

Review for accessibility concerns that intersect with UX laws:

1. **Von Restorff Effect:** Flag emphasis that relies solely on color (color-blind users cannot perceive it).
2. **Fitts's Law:** Check that all interactive elements have adequate hit areas for users with motor impairments.
3. **Law of Similarity:** Verify interactive elements have non-color indicators (underlines for links, borders for buttons).
4. **Selective Attention:** Check that important state changes use multiple channels (visual + text, not just color shifts).

## Step 11: Analyze Mobile and Responsive Design (if applicable)

If the project has responsive styles or mobile views:

1. **Fitts's Law:** Verify touch targets are at least 44x44px with adequate spacing.
2. **Hick's Law:** Check that mobile navigation reduces visible options appropriately.
3. **Cognitive Load:** Flag desktop-density layouts served on mobile without adaptation.
4. **Serial Position Effect:** Check mobile tab bars for optimal ordering of key items.

## Step 12: Produce Report

Format the report as follows:

```
# UX Review Report

## Summary
- Components/files reviewed: <count>
- Critical: <count>
- Warning: <count>
- Suggestion: <count>
- Primary UX laws violated: <list top 3-5 most common violations>

## Scope
<what was reviewed, the frontend stack detected, and any areas skipped under the
large-codebase budget>

## Critical Findings

[CRITICAL] file:line: Description
  UX Law: <law name>
  Impact: <what users experience>
  Recommendation: <specific code-level fix>

## Warnings

[WARNING] file:line: Description
  UX Law: <law name>
  Impact: <what users experience>
  Recommendation: <specific code-level fix>

## Suggestions

[SUGGESTION] file:line: Description
  UX Law: <law name>
  Impact: <what users experience>
  Recommendation: <specific code-level fix>

## Needs Visual Verification
<findings that depend on rendered output: computed target sizes, actual latency,
color contrast. State what to check and how to check it, rather than asserting
the violation from code alone>

## Healthy Areas
<brief note on what is in good UX shape, so the report is calibrated, not just
negative>

## Recommended Follow-ups
- <skill or audit>: <one-line reason based on what was found>

## Top Recommendations

Summarize the 3-5 highest-impact changes that would most improve the overall UX,
referencing findings above. Prioritize by user impact, not by count.
```

Route deep problems outside this skill's scope to the right follow-up instead of going deep here:

| Signal found | Recommend |
|---|---|
| Deep accessibility gaps (ARIA, keyboard traps, contrast failures) | dedicated accessibility audit |
| Missing loading states caused by genuinely slow endpoints | performance-review |
| Dead components, duplication, oversized UI files | repo-review |
| Untested UI logic or interaction handlers | test-scaffold |

Prioritize findings by real user impact. A single critical flow broken by poor cognitive load management matters more than ten minor spacing inconsistencies.
