# Design Guardrails — Hark (native desktop / egui)

Rules for Hark's UI and interaction. This is a **native desktop app** (`eframe`/`egui` immediate-mode + `tray-icon`), **not** a web project — there is no component library, no CSS, no browser. Guardrails are adapted accordingly. The overriding constraint is **latency**: the hot path must never wait on the UI.

## 1. Threading & structure (the non-negotiable rule)

- **macOS requires all UI on the main thread.** The main thread owns the event loop (tray + egui window). The dictation pipeline (hotkey handling, audio, STT, HTTP, injection) runs on **worker threads only**. Getting this wrong causes hangs that surface only on Mac.
- The tray daemon owns the hot path; the settings window is opened on demand and must not exist in the hot path.
- Communicate between pipeline threads and the UI with channels; never block the UI thread on pipeline work, and never block a pipeline thread on the UI.
- **Component/file size:** keep any single UI module under ~300 lines of egui code; split panels (settings / history / stats) into separate widgets. Hard cap 500 lines per file (project rule).

## 2. Latency SLA (the product)

- Model is loaded once at startup and kept warm; run one throwaway warmup inference at launch.
- **Verbatim** and any skip-eligible short utterance must never touch the network.
- History/stats writes happen **after** injection, off the hot path — a DB insert must never delay text appearing.
- Show a lightweight processing indicator (tray state or small overlay) only for the non-Verbatim LLM round-trip; never a modal that blocks input.
- The history list must virtualize via egui `ScrollArea` so long histories never stall rendering.

## 3. Accessibility (desktop)

- **Keyboard-navigable** settings window: every control reachable and operable without a mouse; logical tab order; visible focus indicator.
- Respect OS **light/dark** theme and **reduced-motion**; do not hard-code a single palette.
- Legible default type scale; never convey status by color alone (pair color with an icon or label).
- The global push-to-talk key is user-configurable from day one; never assume a fixed key.

## 4. Interaction & consistency

- One consistent spacing scale and type scale across all panels (egui `Style`/`Spacing`, set once).
- Destructive actions (**clear all history**, **reset stats**) require an explicit confirm step.
- Per-entry history actions: **copy to clipboard** and **delete**, always visible on the row.
- Surface the **selected BYOK model** wherever cleanup output is shown, so a disappointing result has an obvious cause.
- Tray menu stays trivial: voice selection + open window. No heavy logic behind tray items.

## 5. Privacy UI patterns (replaces web "auth UI" section)

Hark has no accounts, sign-in, or auth flows. The equivalent trust surface is **local-first privacy**:

- Honestly disclose in-UI that **non-Verbatim voices send dictated text to the user's chosen provider**; Verbatim/transcription stay local.
- The BYOK key is entered once and stored in the **OS keychain** — never shown back in full, never written to `config.toml`.
- Expose **disable-capture**, **delete entry**, **clear all**, and the **retention cap** prominently; make clear that lifetime stats survive a history clear (separate reset control).

## 6. First-run & permissions (macOS especially)

- macOS **Accessibility** (for injection) and **microphone** access are gated behind OS prompts. Provide a clean first-run flow that explains why each is needed and links to System Settings if denied.
- Handle the denied/revoked state gracefully — never silently fail to inject or record.

## Performance budget

- Cold first decode is a bug: warm + warmup at launch (see §2).
- Keep idle CPU near zero — the ring buffer runs continuously but nothing UI-related runs while idle.
- Windows tray binary has no console; any console child process must set `CREATE_NO_WINDOW` to avoid a flashing window that steals focus (LL-G: `kb/rust/gui-subsystem-console-child-window.md`).
