# Handoff: Hark UI Redesign (Nocturne)

## Overview
A full redesign of the Hark desktop app's main window (History, Dictionary, Invocations, Stats, Settings), plus the update banner and the recording overlay pill, restyled on the **Nocturne** design language: a quiet, compact dark UI — near-neutral blue-grey ground, Inter at medium weight, 8px radii, a single blurple accent used as a line/glow (never a flood), and hairline rules that fade to transparent at their ends.

The headline structural change: **the left sidebar is replaced by a slim top navigation bar** (wordmark + page tabs left, Settings + version right). Everything else keeps the current app's feature set 1:1.

## About the Design Files
The files in this bundle are **design references created in HTML** — an interactive prototype showing intended look and behavior, not production code. The target codebase is the existing **Rust `eframe`/`egui` app** (`crates/hark-app`): recreate these designs there, primarily by retuning `src/theme.rs` (all tokens live there by design) and restructuring `src/ui/shell.rs` (sidebar → top bar). Do not port the HTML/CSS literally; map the values below onto egui `Visuals`, `Style`, and the existing page modules.

`Hark Redesign.dc.html` is the prototype (open in a browser). `nocturne-styles.css` is the design-system token sheet the prototype consumes — the authoritative source for every color/spacing/radius value.

## Fidelity
**High-fidelity.** Colors, typography, spacing, and copy are final. Recreate pixel-faithfully within egui's rendering model. All copy strings match the current app and should be kept verbatim.

## Design Tokens
From `nocturne-styles.css` (`:root`):

- Ground `--color-bg: #161826` · Surface `--color-surface: #232532` · Text `--color-text: #e9e9ed`
- Accent `--color-accent: #9184d9` (accent ramp 100→900: #f5f4ff, #e7e5fe, #d2cefd, #b5abfc, #968ae0, #796cbf, #5d5294, #423a6a, #2b2741)
- Neutral ramp 100→900: #f3f5fe, #e4e7f5, #cfd3e5, #b2b6ca, #9397ab, #75798c, #595d6c, #3f424d, #292b31
- Divider: text at 16% alpha (`color-mix(in srgb, #e9e9ed 16%, transparent)`)
- Semantic (harmonized in OKLCH, not the old flat reds): danger `oklch(0.68 0.15 15)`, success `oklch(0.72 0.12 155)`, warning `oklch(0.78 0.13 75)`
- Muted text: text at ~45–55% alpha (three steps used: 55% descriptions, 50% hints, 45% faint meta)
- Type: Inter 400 body / 500 headings (never bolder — hierarchy is size and space). Body 15px, page titles 24px, section heads 15px/500, hints 12.5px, meta 11.5–12px. Mono (transcripts, latency, terms, triggers): JetBrains Mono / ui-monospace, 13–13.5px.
- Radii: 4 / 8 / 14px (radius-md 8px is the default). Spacing scale (0.7× density): 2.8 / 5.6 / 8.4 / 11.2 / 16.8 / 22.4px.
- Shadows: sm = 1px #3f424d edge; md = 1px #595d6c edge + 0 6px 18px rgba(0,0,0,.55).
- **Buttons are outlined, never filled**: primary = 1px accent border, accent text, transparent bg; hover = accent at 12% fill; secondary = divider border; ghost = accent text, no border. Disabled = 45% opacity.
- Focus: 2px accent outline, offset 2px. Selection: accent at 30%.
- Signature rule treatment: separators are 1px gradients fading to transparent over the last 48px on each side — used under every list row (history, dictionary, invocations).

## Screens / Views

### Shell (all pages)
- **Top bar** (~40px, border-bottom 1px divider, padding 9px 20px): accent mic glyph (17px, Phosphor `microphone`) + "Hark" wordmark (Inter 500, 17px); then tabs History · Dictionary · Invocations · Stats (14px, padding 5px 11px). Active tab: accent text + 2px accent underline (inset shadow / bottom border). Inactive hover: text tint. Right side: outlined "Settings" button with gear icon (active state = accent border + accent text), then version caption `v0.20.0` (11.5px, 40% text).
- **Content**: single scroll area, column max-width 820px (Settings section narrows to 620px), padding 26px 36px. Page header = title (24px) + one-line description (14px, 55% text) — same descriptions as today.
- **Status footer** (bottom, 1px top divider, padding 6px 16px, 12.5px): left = state icon + label; right = provider line (45% text). States: Idle = mic icon (45% text) + "Listening for LCtrl+LWin"; Recording = pulsing 10px danger dot + "Recording"; Processing = spinning circle-notch (accent) + "Processing"; No key = key icon (warning) + "No STT key yet." + ghost "Open Settings" jump. Provider line logic unchanged from `footer.rs` (`on-device · parakeet-tdt-0.6b-v3-int8 · cleanup gemini-3.5-flash-lite` etc.).
- **Update banner** (below top bar when visible): bg accent-900 `#2b2741`, bottom border accent-800, 13px accent-200 text "Hark v0.21.0 is available.", ghost "Details" (jumps to Settings), outlined-primary "Install", dismiss ✕ at far right.
- **Confirm dialog** (Clear all / Reset stats / Remove key): centered modal on `neutral-900 @ 50%` backdrop; surface bg, radius 14px, shadow-lg; 18px title, 14px body at 85%, right-aligned actions: secondary "Cancel" + danger-outlined confirm (danger text, danger-at-45% border).

### History
- Toolbar: 270px search input with inline magnifier icon (search-as-you-type over raw + final text; Ctrl+F focuses), count label ("556 dictations" / "N matches", 13px 55%), right-aligned borderless danger "Clear all" (disabled at zero).
- Day headers: uppercase 11px, 0.08em tracking, 50% text ("Today", "Yesterday", weekday).
- Rows (padding 10px 0, fading 1px rule below): 14.5px two-line preview (≈160-char clip); caption 12px 48% "44 min ago · clean · parakeet-tdt-0.6b-v3-int8", prefixed "⚡ Invocation ·" (accent lightning icon) when an invocation fired. Right: fade-out "Copied" affirmation (success, ~1.2s), 29px icon buttons copy + trash (secondary outline).
- Expanded detail (click preview; Esc collapses; one at a time): surface panel radius 8px, padding 11px 14px — optional invocation line, "RAW TRANSCRIPT" micro-label (10.5px uppercase 45%), raw text in mono 13px, timing line mono 11.5px (`stt 412 ms · cleanup 366 ms (gemini-3.5-flash-lite) · total 958 ms`), full timestamp + provider (11.5px 45%).
- "Show more" secondary button, centered, pages by 8 (real app: 100).
- Empty states (icon 36–38px at 50% + title + hint, centered): no storage / zero dictations ("Dictations appear here. / Hold LCtrl+LWin and speak…") / zero matches.

### Dictionary
- Description lines (as today), then pinned add row: 280px input "Add a term" + outlined-primary "Add" (disabled empty; Enter adds and keeps focus; trims + dedupes).
- Rows: term in mono 13.5px, click-to-edit in place (Enter/blur commits — empty or duplicate reverts; Esc cancels), trash icon button right; fading rule below each row.
- Empty state: book-open icon, "No dictionary terms yet."

### Invocations
- Description paragraph (verbatim), outlined-primary "⚡ New invocation".
- Rows: accent lightning + `"phrase"` in mono 13.5px + scope label 11.5px 45% ("whole dictation" / "anywhere"); one-line 80-char expansion preview (13px 55%, click to edit); warning line (warning color, 12px) for dead entries: needs-two-words / no-text / shadowed-by-earlier-duplicate. Right: secondary "Edit" + trash icon.
- Empty state includes the "You say / Hark types" two-column example.
- **Editor owns the whole page** (max-width 640px): "When I say" (320px input, hint `access granted`, live problem line in danger: empty / one-word / duplicate trigger); "Fire when" radios (whole-dictation default; "anywhere" reveals the cleanup-skip note); "Type this" textarea ("Injected exactly as written, line breaks and all."); "Try it" probe with live ✓ "Would fire" (success) / ✕ "Would not fire" (muted). Actions: primary Save (disabled while a problem exists or expansion empty), secondary Cancel, danger-outlined Delete right-aligned (existing entries only). Commits only on Save.

### Stats
- Unlock gate below 10 dictations: chart icon, "N of 10 dictations to unlock stats", 220px accent progress bar — never a zeroed dashboard.
- Unlocked: 2×2 cards (surface, shadow-sm edge, padding 16px 18px, max 340px wide): value 26px Inter 500 + 12.5px 50% label — Dictations, Words, Speaking time; Avg release-to-inject uses mono 23px and shows "n/a" when no total-time data.
- "About 2 h 4 m saved vs typing at 40 WPM." + "Since July 17, 2026." (12.5px 45%).
- Danger-outlined "Reset stats" behind a confirm that promises history stays untouched.

### Settings (620px column; sections spaced 18px, heads 15px/500)
Order and behavior mirror `settings/*.rs`:
1. **Get started card** (onboarding only): surface card, three numbered steps that earn success checks (provider picked / key stored / test passed), "Skip for now" ghost link.
2. **Speech to text**: radios Deepgram / OpenAI / Groq / OpenAI-compatible (kind change clears model+URL; compatible force-opens Model & endpoint).
3. **Key section**: status line (✓ success "Key stored for deepgram" / key-icon warning "No key for X yet"), masked 280px paste input + "Store" (disabled empty; Enter stores; buffer cleared immediately) + "Remove" behind confirm; store/remove for the running provider takes effect immediately. Then "Test connection" with spinner and success line (`STT reached nova-3 · 412 ms`, ms in mono).
4. **Model & endpoint** (collapsed by default): Model + Base URL inputs with per-provider default hints; inline danger error when compatible has no URL.
5. **Push-to-talk**: 180px chord field + "Record". Recording swaps to a surface panel showing held keys live + "Release to set. Modifier keys (Ctrl, Shift, Alt, Win), CapsLock, and F1..F24 only." + Cancel; the pipeline's own hook pauses during capture. Hint: "Hold these keys together to dictate; release to inject."
6. **Microphone**: 280px picker (System default first; comms-default device labeled "— used by Teams/Zoom"; unplugged configured device stays listed "(not connected)") + "Rescan". Live level meter: 240px × 6px bar, sqrt-scaled width, color+note by band — silent (neutral, "No input — is this the right microphone?"), quiet (warning), good (success, "Good level."), hot (danger).
7. **On-device model**: description + radios Off / backup / primary. When used: surface card with mono model name `parakeet-tdt-0.6b-v3-int8` · 671 MB, state line (ready ✓ / partial ⚠ resume note / not-downloaded ◌ accent), controls (Download / Resume / Re-download outlined-primary; Delete; Cancel + accent progress bar with "N MB of 671 MB" while downloading; download starts immediately, not on Save), license caption. Warning when primary && not ready: "Dictation will not work until this finishes downloading…".
8. **Voice**: picker Verbatim/Clean/Professional/Casual/Custom; Custom reveals prompt textarea + danger error when empty.
9. **Cleanup provider** (collapsed): override checkbox; when on — radios OpenAI/Groq/OpenAI-compatible (no Deepgram), Chat model / Base URL / Keychain account inputs with default hints; same-account note vs. its own key section when the account differs. When off — inherited line ("Inherited from STT (X) · model" / Verbatim no-call / Deepgram degradation warning). "Test cleanup" with spinner + result; note when default voice is Verbatim.
10. **Behavior** (collapsed): "Skip cleanup below [n] words" (0–50), "Reject cleanup longer than [x] x what you said" (1.00–5.00), Theme radios System/Light/Dark (this design documents Dark; Light derives from the same ramps), "Launch Hark at login" checkbox — each with its hint line.
11. **History & privacy** (collapsed): capture checkbox + note, "Keep at most [n] entries, for [n] days", privacy paragraph (verbatim).
12. **Updates**: "You're on Hark v0.20.0", "Check for updates" + spinner "Checking GitHub…", result states (up-to-date ✓ / available accent + "Download & install" / ready ✓ + "Restart to finish" / failure in danger), auto-check checkbox.
- **Sticky save bar** pinned above the footer whenever dirty or a notice is pending (surface bg, 1px top divider): warning ⚠ "Unsaved changes" + primary Save + secondary Discard; after save, success ✓ "Saved. Pipeline restarted." + Dismiss. A failed save shows the cause in danger and outranks "Unsaved changes".

### Recording overlay pill
Borderless always-on-top capsule, bottom-center ~72px above screen bottom: near-black translucent bg (#101120 @ 92%), pill radius, shadow-md, 13px accent dot pulsing (expanding accent glow rings, ~1.6s ease-out loop; blend a mic-amplitude pulse with a faint idle breathing sine as today) + "Listening…" 13px in neutral-200. Never takes focus, mouse-passthrough, hidden from taskbar (existing `overlay.rs` behavior — restyle only).

## Interactions & Behavior
- Nav switches pages instantly; open invocation editor closes on nav.
- History: search resets paging; row click toggles expand (one max, Esc collapses unless a dialog owns Esc); copy shows fading "Copied"; deletes are immediate; Clear all behind confirm naming the count.
- Dictionary and Invocations persist immediately in the real app (pipeline restart); the invocation editor commits only on Save.
- All destructive actions (Clear all, Reset stats, Remove key) go through the confirm dialog; confirm buttons name the action ("Delete 12 entries", "Reset stats", "Remove key").
- State is never carried by color alone — every status pairs an icon/mark with copy (WCAG AA is pinned by tests in `theme.rs`; keep those tests passing with the new palette).

## State Management
Reuse the app's existing state seams unchanged: `PipelineStatus` drives footer/overlay/tray; pages keep their cached-query pattern (`cache_key` on generation/search/pages); Settings keeps draft-vs-saved with the sticky bar; key storage stays keychain-immediate. This is a reskin + shell restructure, not a data-flow change.

## Assets
- Icons: **Phosphor** (already vendored in the app via `theme::icons`) — microphone, gear, magnifying-glass, copy, trash, lightning, book-open, chart-bar, clock-counter-clockwise, key, warning, check, x, circle-notch, arrow-up.
- Fonts: Inter 400/500/600 + JetBrains Mono (already embedded in the app).
- No photographs or raster assets.

## Files
- `Hark Redesign.dc.html` — the interactive prototype (all five pages, banner, overlay, dialogs).
- `nocturne-styles.css` — the Nocturne token sheet + component classes (authoritative values).
- `nocturne-readme.md` — the design system's own usage guide (direction, ramps, do/don't).
