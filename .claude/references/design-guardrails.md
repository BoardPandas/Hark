# Design Guardrails — Hark (native desktop / egui)

Rules for Hark's UI and interaction. This is a **native desktop app** (`eframe`/`egui` immediate-mode + `tray-icon`), **not** a web project — there is no component library, no CSS, no browser. Guardrails are adapted accordingly. The overriding constraint is **latency**: the hot path must never wait on the UI.

## 1. Threading & structure (the non-negotiable rule)

- **macOS requires all UI on the main thread.** The main thread owns the event loop (tray + egui window). The dictation pipeline (hotkey handling, audio, STT, HTTP, injection) runs on **worker threads only**. Getting this wrong causes hangs that surface only on Mac.
- The tray daemon owns the hot path; the settings window is opened on demand and must not exist in the hot path.
- Communicate between pipeline threads and the UI with channels; never block the UI thread on pipeline work, and never block a pipeline thread on the UI.
- **Component/file size:** keep any single UI module under ~300 lines of egui code; split panels (settings / history / stats) into separate widgets. Hard cap 500 lines per file (project rule).

## 2. Latency SLA (the product)

- Release-to-inject = WAV encode (keep under ~10 ms) + one HTTPS POST to the STT provider + phonetic post-correction + inject.
- One long-lived HTTP client for the whole process (keep-alive + TLS session resumption); consider pre-warming a connection at launch. Never build a client per press.
- At most one retry, on timeout/connect errors only; never retry 4xx; never stack retries on the hot path.
- **Every dictation now touches the network** (STT is cloud). Show the lightweight processing indicator (tray state or small overlay) for every in-flight dictation; never a modal that blocks input. Skip-eligible short utterances may still skip the *cleanup* call, never the STT call.
- History/stats writes happen **after** injection, off the hot path; a DB insert must never delay text appearing.
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
- Surface the **selected STT provider/model** and the **cleanup model** wherever output is shown, so a disappointing result has an obvious cause.
- Tray menu stays trivial: voice selection + open window. No heavy logic behind tray items.

## 5. Privacy UI patterns (replaces web "auth UI" section)

Hark has no accounts, sign-in, or auth flows. The equivalent trust surface is **honest BYOK privacy**:

- Honestly disclose in-UI that **every dictation sends audio to the user's chosen STT provider**, and that non-Verbatim voices additionally send the transcribed text to the cleanup provider. History, stats, and the dictionary never leave the machine.
- Provider API keys are entered once and stored in the **OS keychain**; never shown back in full, never written to `config.toml`.
- Offline and provider-error states are first-class UI: distinguishable tray states for "no network", "key rejected", and "provider error/timeout". Dictation fails fast and visibly, never silently.
- Expose **disable-capture**, **delete entry**, **clear all**, and the **retention cap** prominently; make clear that lifetime stats survive a history clear (separate reset control).

## 6. First-run & permissions (macOS especially)

- macOS **Accessibility** (for injection) and **microphone** access are gated behind OS prompts. Provide a clean first-run flow that explains why each is needed and links to System Settings if denied.
- Handle the denied/revoked state gracefully — never silently fail to inject or record.

## Performance budget

- A cold TLS handshake on the first dictation is a bug: reuse one client and consider a launch-time connection pre-warm (see §2).
- Keep idle CPU near zero — the ring buffer runs continuously but nothing UI-related runs while idle.
- Windows tray binary has no console; any console child process must set `CREATE_NO_WINDOW` to avoid a flashing window that steals focus (LL-G: `kb/rust/gui-subsystem-console-child-window.md`).
