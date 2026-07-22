# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.20.0] - 2026-07-22

### Added
- **Invocations: say a short phrase, get a block of text you wrote.** A new section in the sidebar, next to Dictionary. You pair a trigger phrase with the text it should produce — say "access granted" and Hark types the paragraph listing what a support tech has access to. The text is injected exactly as you wrote it, including line breaks, and never goes near a cleanup voice, so it cannot be reworded, shortened, or "improved" on the way to your cursor. Each invocation decides for itself when it may fire: **whole dictation only** (the default — the phrase has to be the entire thing you said, so it can never go off mid-sentence by accident) or **anywhere in the sentence**, where it is spliced into a longer dictation in place.
- **Triggers are matched by sound, not by spelling.** If your provider hears "access granite" for "access granted", the invocation still fires. Hark matches triggers the same way the Dictionary already matches terms, but demands a closer match before acting — a Dictionary mistake costs you one word, an invocation mistake pastes a whole paragraph. A trigger that keeps getting misheard can be fixed for free by adding the stubborn word to your Dictionary, since triggers are matched after Dictionary corrections are applied.
- **A dictation that fires an invocation skips the cleanup call.** It costs nothing, adds no delay, and cannot fail — which also means an "anywhere" trigger inside a longer sentence means that whole dictation goes uncleaned.
- **An invocation that can never fire says so on its own row.** A trigger shorter than two words, an invocation with no text to type, or a trigger a previous entry already claimed is marked in the list with the reason, instead of quietly doing nothing when you say it.

### Changed
- **Invocations count the words you spoke, not the words Hark pasted, in your stats.** "Time saved" values every word at typing speed, so a two-word trigger producing a 300-word block would otherwise have invented about seven and a half minutes of saved time out of nothing. Your existing figures are untouched; only invocation dictations count differently, and only from this version on.

## [0.19.1] - 2026-07-22

### Added
- **Groundwork for transcribing and tidying in a single request.** With a cleanup voice switched on, Hark currently makes two round trips before your words appear: one to transcribe, then a second to clean the result up, which cannot start until the first has finished. Google's Gemini can do both at once. This release adds the adapter for it, behind the scenes and not yet selectable in Settings, along with a harness that measures it against the current two-request path so the choice is made on real numbers rather than expectations. Nothing changes about how Hark behaves today.

## [0.19.0] - 2026-07-22

### Added
- **The cleanup provider has its own Test button.** "Test connection" only ever tried cleanup when your default voice was one that runs it, so anyone using Verbatim by default — or pointing cleanup at a second provider with its own key — had no way to check that the endpoint, model, and key actually worked short of dictating and hoping. Settings → Cleanup provider now has a Test cleanup button that sends one short request to whatever that section is currently set to and reports the model it reached and how long it took, or the reason it failed. A Verbatim default is tested with the Clean voice, since Verbatim makes no cleanup call at all, and the result says so.
- **An unsaved-changes bar above the status footer.** The Save button used to sit at the bottom of the settings form, which is exactly where a long form pushes it out of view — so a changed setting could sit unsaved with nothing to say so. Changing anything in Settings now raises a bar just above the status bar with Save and Discard, pinned in place no matter how far you have scrolled. It also carries the result of the save, so a failure names its cause instead of leaving a silent form.

### Fixed
- **The status bar names the engine that is actually transcribing.** With the on-device model set as your primary engine, the bottom-right corner still read `deepgram · nova-3` — a cloud provider that, in that mode, is never contacted. It now reads `on-device` and the model name. Set to backup instead, it names your cloud provider first and marks the local model as the fallback it is.

## [0.18.1] - 2026-07-21

### Fixed
- **The recording pill is visible again on multi-monitor setups.** If the floating indicator stopped appearing when you hold the shortcut — while dictation itself kept working perfectly — this is why. Hark worked out where to put the pill from the size of one monitor while the position was measured from the corner of your main one, and it assumed every screen ran at the same scaling. On a single screen that guess happens to be right. On two, especially at different scaling levels, it put the pill on the wrong monitor or in the empty space between them, where it was drawn every frame and never seen. Hark now asks Windows where your screens actually are, and places the pill on the one you are typing into, clear of the taskbar.

## [0.18.0] - 2026-07-21

### Added
- **Hark can now transcribe without the internet.** Under Settings → On-device model you can download a speech model onto your computer and have Hark use it. Nothing is sent anywhere: the audio never leaves the machine. Two ways to use it, and you pick: as a **backup**, where Hark tries your cloud provider first and quietly falls back to the local model when the provider is down, times out, or your key stops working — so a dropped connection costs you a second instead of your sentence; or as your **primary engine**, where Hark never contacts a provider at all and does not ask you for an API key. The model is Parakeet TDT 0.6B v3, it is about 670 MB, and it is not included in Hark — you download it on demand from the Settings page, with a progress bar, a cancel button, and a delete button to get the space back later. Cancelling keeps what you have already downloaded, so starting again picks up where you left off rather than beginning again.
- **History says which engine wrote each line.** An entry produced on-device is labelled `local`, and one produced because the cloud failed is labelled `local (fallback)`, so a transcript that reads differently from usual explains itself instead of being blamed on the wrong model.

### Changed
- **When a local backup is ready, Hark gives the cloud a shorter deadline.** Waiting the full 15 seconds for a failing provider and only then spending a couple of seconds transcribing locally would make a rescued dictation slower than no rescue at all. With a downloaded model standing by, cloud requests get 6 seconds before Hark falls back (`local_stt.fallback_after_ms`). Without one, nothing changes.
- **The first dictation using the local model says what it is doing.** Loading the model into memory takes a few seconds, once, and the status bar now names that instead of looking frozen. After that it stays loaded and every dictation is fast — at the cost of the memory it occupies, which is why there is a Delete button.

## [0.17.0] - 2026-07-21

### Fixed
- **Hark no longer goes deaf on quieter microphones.** If you had to lean into your mic for Hark to pick you up — while the same mic worked fine in Teams, Zoom, or Discord — this is why. Hark judged whether you had spoken by averaging the loudness of the whole recording, and every recording includes the padding Hark keeps before and after your words. That padding is silence, and averaging it in dragged the number down, so short utterances scored lower than long ones spoken at exactly the same volume. A quiet "yes" could be discarded while a quiet full sentence went through. Hark now asks whether the recording ever reached speaking level, which does not care how long you spoke or how long you paused before starting.
- **Speech is now measured against your room, not a fixed number.** Even with the above fixed, the cutoff was a fixed loudness that assumed a certain microphone. Hark now also accepts anything that stands clearly above the background noise of your own recording, so a quiet setup works without you touching anything.
- **Stereo and array microphones no longer lose half their volume.** Many laptop microphones present a second channel that is nearly silent. Hark used to average the channels together, which quietened your voice by about half on those machines and pushed borderline recordings under the cutoff.

### Added
- **A live input meter in Settings → Microphone.** It shows what Hark is actually hearing right now, with plain-language guidance — no input, too quiet, good, or too loud. If Hark cannot hear you, you can now see that immediately instead of guessing.
- **Windows' communications microphone is labelled in the picker.** Windows keeps two separate default microphones: one for general use and one for calls, and headset setup guides usually tell you to set the calling one. When those differ, Teams and Hark listen to different microphones. The device Teams and Zoom use is now marked in the list.
- **Quiet recordings get boosted before transcription.** Recordings that come in below the level transcription services expect are lifted automatically, stopping short of distortion and without amplifying a quiet room into hiss. Audio that was already at a good level is passed through untouched.

### Changed
- **"We didn't catch that" now says so.** Discarded dictations used to end in silence, indistinguishable from a broken app. When Hark hears nothing loud enough to be speech it now says so in the status bar and offers a jump to Settings. Tapping the shortcut by accident stays silent, as before, because that needs no reply.

## [0.16.0] - 2026-07-21

### Added
- **Only one Hark runs at a time.** Starting Hark while it is already running — the usual way being launch-at-login plus a manual launch or a double-click on the Start Menu shortcut — used to start a second copy, leaving two programs fighting over the same push-to-talk key, two tray icons, and two writers on one history database. The second copy now notices the first and exits quietly, leaving the running Hark alone. Signing in as a different user still gets its own Hark, and force-quitting Hark never blocks the next launch.

## [0.15.0] - 2026-07-21

### Added
- **A cleanup that rewrites too much is now thrown away.** Clean, Professional, and Casual could turn a short remark into a full paragraph. Any cleanup whose output runs past a length budget is discarded and your own uncleaned words are injected instead, so a chatty model can no longer put sentences in your mouth. The limit is `voice.max_expansion_ratio` (default 1.4x what you said; `0` turns the check off), adjustable under Settings → Behavior.

### Changed
- **Voice presets edit rather than rewrite.** Every built-in voice now states that it is editing a spoken transcript, not writing prose: a five-word dictation comes back about five words, with no invented sentences, greetings, sign-offs, or context you did not say. Professional and Casual adjust wording and formality only. The Custom voice is deliberately exempt from both the instruction and the length check, so a prompt like "turn this into an email" still works as written.

## [0.14.3] - 2026-07-17

### Fixed
- **Windows installer build.** The installer script used an invalid registry flag that aborted the Inno Setup compile, so no signed installer was produced. The launch-at-login registry entry now uses the correct flag and the installer builds again.

## [0.14.2] - 2026-07-17

### Added
- **Project documentation wiki.** A new `Docs/` folder holds a generated, evidence-based wiki: an overview, architecture, getting-started, configuration, and data-storage pages, one page per pipeline subsystem (audio capture, transcription, dictionary, voice cleanup, text injection, desktop UI, updates and autostart), a release-and-packaging runbook, and a glossary, all indexed from `Docs/README.md`. Every claim links to the exact source file and line range it describes. Developer-facing only, with no change to the app.

## [0.14.1] - 2026-07-17

### Changed
- **Phase-5 handoff documentation.** Added a Phase 5 (Polish/Ship) handoff recording that the Windows-side Ship work has landed (signed installer, launch-at-login, in-app updates, mic picker) and scoping the remaining macOS work (packaging/notarization, login item, first-run permission flow) plus a cross-platform single-instance guard. No app behavior changed.

## [0.14.0] - 2026-07-17

### Added
- **Windows installer.** Hark now ships as a proper installer (`Hark-<version>-windows-x64-setup.exe`), the headline download on each release, alongside the portable exe. It installs per user to `%LOCALAPPDATA%\Programs\Hark` with no admin prompt, adds a Start Menu shortcut (and an optional desktop shortcut), and registers a clean uninstaller. An uninstall leaves your settings and history in `%APPDATA%\hark` untouched.
- **Launch at login.** Hark can start automatically when you sign in to Windows, running hidden in the system tray with no window. It is on by default and controlled by a new **"Launch Hark at login"** toggle under **Settings → Behavior**. Turning it off removes the startup entry, and disabling Hark from Windows Task Manager's Startup tab is respected rather than overridden.
- **Microphone picker in Settings.** A new **Microphone** section lets you choose which input device Hark records from, with a **Rescan** button for mics plugged in after the page opened. A configured microphone that is currently unavailable stays selected (shown as "not connected") instead of silently resetting, and capture falls back to the system default until it returns.

### Changed
- **README brought in line with the BYOK-cloud pivot.** The intro, design principles, tech-stack table, architecture diagram, prerequisites, project structure, and privacy notes no longer describe the retired on-device (sherpa-onnx / Parakeet) transcription; they now document the `SttProvider` trait, the OpenAI-compatible and Deepgram adapters, and the actual crate layout, and the privacy section states plainly that audio is sent to your chosen provider under your own key.

## [0.13.7] - 2026-07-17

### Removed
- **Retired the internal `hark-cli` dev binary.** The push-to-talk app (`hark-app`) has fully replaced it end to end, so the standalone command-line harness has been removed from the workspace. Internal cleanup only, with no effect on the installed app.

## [0.13.6] - 2026-07-17

### Added
- **Choose which microphone Hark records from.** Capture can now target a specific input device instead of always using the Windows default: the selected device name is stored in the `[audio]` section of the config, and if that device is later unavailable (unplugged, powered off) capture falls back to the system default rather than stopping. The Settings picker that exposes this lands with the settings-UI update.
- **In-app updates.** Hark now checks GitHub for a newer release and can install it for you. A **Check for updates** button in Settings reports whether you're current and, when a new version exists, shows its release notes with a one-click **Download & install**. Hark also checks once automatically at startup (a toggle in Settings turns this off) and, if an update is waiting, shows a banner across the top with **Install** and, once downloaded, **Restart to finish**. On Windows the downloaded build's code signature is verified against the running app's publisher before it replaces anything, so only a genuine signed Hark is ever installed; on macOS the check links out to the release page.

## [0.13.5] - 2026-07-17

### Changed
- **Phase-4 wrap documentation.** CP6 (live hardware validation) and the signed-release pipeline are marked done in the phase-4 spec, and a CP7 handoff (retire hark-cli) was added for the next session. The three Azure Trusted Signing gotchas found while getting signed releases green were contributed to the shared LL-G knowledge base under a new `github-actions` category.

## [0.13.4] - 2026-07-17

### Fixed
- **Windows release signing now authenticates correctly.** The Azure Trusted Signing action (v2) reads the service principal from its own `azure-tenant-id` / `azure-client-id` / `azure-client-secret` inputs; the workflow had been supplying them as step environment variables, which the action never picked up, so every signed release failed with "environment variables are not fully configured." The credentials are now passed as action inputs.

## [0.13.3] - 2026-07-17

### Fixed
- **Release signing now fails fast with a clear message when a signing secret is empty.** Previously an empty Azure credential surfaced only as an opaque `SignerSign() failed` crash deep inside signtool. A preflight step now checks all six signing secrets before signing and, if any is empty, stops with a message naming the offending secret and pointing at the Doppler source. It prints only secret lengths, never values, so the log stays safe to share.

## [0.13.2] - 2026-07-17

### Fixed
- **Release workflow now references the actual Azure signing secret names.** The signing step read `AZURE_SIGNING_ENDPOINT` / `_ACCOUNT_NAME` / `_CERT_PROFILE_NAME`, but the Doppler-synced secrets are named `AZURE_TRUSTED_SIGNING_*`; the mismatch left the endpoint empty and failed signing. Aligned the workflow and `.github/RELEASING.md` to the real names.

## [0.13.1] - 2026-07-17

### Added
- **Signed Windows release automation.** Pushing a `vMAJOR.MINOR.PATCH` tag now builds `hark-app` on a Windows runner, signs the exe with Azure Trusted Signing (RFC3161-timestamped), verifies the signature, and publishes a GitHub release with the signed `Hark-<version>-windows-x64.exe` attached and auto-generated notes. The tag version is checked against `package.json` so a mistagged build fails before shipping. Setup (required secrets, Azure RBAC role, and the tag flow) is documented in [`.github/RELEASING.md`](.github/RELEASING.md).

## [0.13.0] - 2026-07-17

### Added
- **Hark now lives in the system tray (Phase 4 checkpoint 5).** A tray icon shows the pipeline state at a glance: an accent ring while listening, a red disc while recording, an accent disc while processing, and exclamation-marked discs for the failure states (amber when the API key needs attention, red when the last dictation failed, gray when the pipeline is stopped). The tooltip always says the same thing in words, including the hotkey chord while listening and the actual cause on a failure.
- **The tray menu carries the essentials:** a voice radio group (picking a voice persists it, applies it to the next dictation immediately, and keeps the Settings form in sync), Open Settings, and Quit. Double-clicking the tray icon brings the window back on whatever page it was.
- **The app launches hidden when dictation is ready.** With a key stored, Hark starts straight into the tray; the window only appears when something needs attention (first-run onboarding, a broken config, a stopped pipeline). Tray interactions are delivered promptly even while the window is hidden.

### Changed
- **Closing the window now hides it instead of quitting;** Hark keeps dictating from the tray, and Quit lives in the tray menu. If the tray cannot be created, the window stays visible and close means quit, so the app is never unreachable.

## [0.12.1] - 2026-07-16

### Added
- **Phase 4 CP5 session handoff (`tasks/2026-07-16-handoff-phase4-cp5.md`):** repo state after CP4, the load-bearing CP4 discoveries, the CP5 scope (tray icon and menu, state icons, close-to-hide, the hidden-window event-delivery design question with the recommended pump-pattern answer), and the deferred items carried to CP6, now including a real-hardware pass over the new History/Stats panels. Two durable CP4 lessons were contributed to the shared LL-G knowledge base: egui `ScrollArea::show_rows`'s uniform-row-height assumption breaking heterogeneous lists (MEDIUM), and the join-on-drop `JoinHandle` deadlock when a later-dropped field still holds a sender to the worker (HIGH).

## [0.12.0] - 2026-07-16

### Added
- **Dictation history is live (Phase 4 checkpoint 4).** Every successful dictation is now saved locally the moment after it injects: a dedicated storage thread receives the finished record from the pipeline, writes it to the history database, and applies the retention caps, all off both the dictation hot path and the UI thread, so neither latency nor responsiveness pays for it. If the database cannot be opened, dictation still works; History and Stats say why they are empty.
- **The History panel is real.** Dictations appear newest first under day headers (Today / Yesterday / date), searchable as you type across both the raw transcript and the final text, with a live entry count. Each row shows the final text with a caption (relative time, voice, model) and always-visible copy and delete buttons; copying affirms inline with a fading "Copied". Clicking a row expands the raw transcript, the timing breakdown (stt / cleanup / total ms, naming the cleanup model only when one actually ran), and the full timestamp; Esc or a second click collapses it. Ctrl+F jumps to search. "Clear all" sits behind a confirm that states the exact entry count and that lifetime stats are untouched. Long histories load in pages behind a "Show more" button.
- **The Stats panel is real.** Locked behind the first 10 dictations (a progress bar toward unlocking, never a dashboard of zeros), it then shows lifetime cards for dictations, words, speaking time, and average release-to-inject latency, plus an honest "time saved vs typing at 40 WPM" line and the date counting began. "Reset stats" has its own confirm and leaves history entries untouched. On a database from before this release the latency average shows "n/a" instead of a made-up zero until new dictations accumulate.
- **Retention settings now apply on save and at startup** (not just after the next dictation): every pipeline start re-applies the entry-count and age caps.
- The stats row now also accumulates total release-to-inject time (schema migration 002) so the average latency figure derives from real totals rather than an approximation that would omit encode and inject time.

## [0.11.1] - 2026-07-16

### Added
- **Phase 4 CP4 session handoff (`tasks/2026-07-16-handoff-phase4-cp4.md`):** repo state after CP3, the load-bearing CP3 discoveries, the CP4 scope (storage thread, history panel, stats panel), and the deferred items parked for CP6 real-hardware validation. Two durable CP3 lessons were contributed to the shared LL-G knowledge base: eframe startup `set_theme` silently clobbering the persisted theme preference (HIGH), and the egui `CollapsingHeader` controlled-open one-frame latch (MEDIUM).

## [0.11.0] - 2026-07-16

### Added
- **The full settings form (Phase 4 checkpoint 3).** Everything the app needs configured is now in the window, with progressive disclosure: the provider picker, key section, hotkey, and voice stay visible; model & endpoint overrides, the cleanup provider, behavior, and history & privacy live in collapsed sections. The key section pastes into a masked field and writes straight to the OS keychain (a stored key is never displayed back; Remove sits behind a confirm dialog whose safe default is Cancel), the hotkey field validates the chord as you type, the model and base URL fields show each provider's default as a hint (with the endpoint section auto-expanding for OpenAI-compatible endpoints, where a base URL is required), and the voice picker exposes the custom prompt when Custom is selected. The cleanup section states honestly what will run: "Inherited from STT" with the model, the reason cleanup cannot run (e.g. Deepgram has no chat product), or an explicit override with its own key section when the keychain account differs. History & privacy carries the capture toggle (content off, counters still tick), both retention caps, and a plain-language disclosure of exactly what leaves the device.
- **Test connection, inline and honest.** One click transcribes a bundled speech clip through the configured provider on a background thread and shows the transcript plus latency; when a cleanup call would run, a tiny chat call tests that too. Results stay on screen until the next test, failures name the cause in the provider's own words, and Groq gets an honest note that every request bills a 10 second minimum.
- **Save applies everything at once:** validate, persist the TOML atomically, restart the dictation pipeline, and say what happened, in that order. An invalid draft changes nothing on disk. Storing or removing a key for the active provider restarts the pipeline immediately, so a fixed key starts working the moment it lands.
- **Get Started onboarding.** With no key at startup, Settings opens with a three-step card (pick provider, paste key, test) that checks steps off as they complete and then swaps to "Hold [your chord] and speak into any text field", which is true when shown because a passing test during onboarding saves and starts the pipeline automatically. The card is dismissible and retires itself after the first successful dictation. No wizard, nothing modal.
- **The dictionary is editable:** a pinned add field (Enter adds and keeps focus for the next term), click-to-edit rows, per-row delete. Every change saves immediately and applies to the next dictation.
- **A Light / Dark / System theme choice** in Settings > Behavior; the preference survives relaunch.

## [0.10.2] - 2026-07-16

### Added
- **Dependency audits now run clean and mean it.** A full-workspace audit (2026-07-16) found zero vulnerabilities in Hark's direct dependencies and a current toolchain. The three RustSec advisories cargo-audit reports (two quick-xml DoS issues and the ttf-parser unmaintained notice) live only in the Linux/Wayland build path under eframe/winit, which is never compiled into the shipped Windows or macOS binaries, and the quick-xml fix is blocked upstream until wayland-scanner adopts 0.41. A new `.cargo/audit.toml` records each ignore with that justification and its re-evaluation trigger, so `cargo audit` exits clean and any future finding is a real signal. Also noted for the record: reqwest stays on 0.13.1 deliberately; 0.13.4 dropped the `webpki-roots` feature the static-trust-roots design depends on (lesson contributed to the shared LL-G knowledge base).

## [0.10.1] - 2026-07-16

### Added
- **Phase 4 CP3 session handoff (`tasks/2026-07-16-handoff-phase4-cp3.md`)** and CP2 implementation lessons recorded in the Phase 4 spec §6 (eframe 0.35 trait and panel changes as found in practice, the vendored Phosphor icon decision, per-theme Context setters, font zip layouts, the worker-to-UI repaint pump, honest dictation-record labeling, and the build machine's Application Control policy blocking release builds). The three durable Rust lessons (libsqlite3-sys hidden MSRV, eframe 0.34/0.35 App trait split, egui companion-crate lag) were contributed to the shared LL-G knowledge base.

## [0.10.0] - 2026-07-16

### Added
- **Hark is now a windowed desktop app (`hark-app`, Phase 4 checkpoint 2).** A new binary opens a real settings window: fixed sidebar (History / Dictionary / Stats navigation, Settings pinned at the bottom, version caption), a persistent status footer that always tells the truth about the pipeline (Listening for the configured chord / Recording / Processing / an error with its cause, plus an "Open Settings" jump when the problem is key-related), and honest placeholder pages with designed empty states where the CP3/CP4 editors and lists will land. At startup the app resolves the BYOK key from the OS keychain and starts the dictation pipeline; with no key it opens on Settings with the footer saying why, never a silent dead state. Closing the window quits (the tray arrives at CP5). Debug builds keep a console for logs; release builds are windowless.
- **A real visual identity, not an egui demo.** Embedded Inter (Regular/Medium/SemiBold) and JetBrains Mono fonts (SIL OFL 1.1, official releases), a hand-rolled light and dark theme following the OS preference, an indigo accent, soft shadows, hairline strokes, a 4 px spacing grid, and a fixed type scale, all defined in one `theme.rs` token module. Icons are vendored Phosphor glyphs (the egui-phosphor crate still targets egui 0.34, so the font plus its matching codepoints are embedded directly). Unit tests pin the palette hexes and verify WCAG AA contrast (4.5:1 body text, 3:1 indicators) in both themes.
- **The pipeline now reports what it is doing.** `hark_pipeline::run` takes an event channel and emits advisory events (Recording, Processing, Injected with the full dictation record, Failed with the stage and cause). Sends are non-blocking and best-effort: dictation never waits on, and can never be broken by, the UI. The dictation record carries the raw and final transcripts, voice/provider/model labels, and per-stage timings, and deliberately has no Debug formatting so transcript content cannot leak into logs. When a cleanup call is skipped, gated, or fails, the record labels the entry verbatim rather than blaming a model that never ran.

### Changed
- **`hark-cli` passes the new event channel** (and drops the receiver; the CLI has no UI). Behavior is otherwise unchanged; the crate retires at CP7.

## [0.9.5] - 2026-07-16

### Added
- **Config saves are proven safe to repeat:** a new test verifies that saving over an existing `config.toml` replaces it cleanly on Windows (the temp-file + rename path), which is exactly what the settings UI will do on every Save.
- **Phase 4 session handoff (`tasks/2026-07-16-handoff-phase4-cp2.md`)** and CP0/CP1 implementation lessons recorded in the Phase 4 spec (toolchain MSRV gotcha behind rusqlite's bundled SQLite, in-memory WAL behavior, TOML Option serialization, keyring test seams, Windows rename semantics).

## [0.9.4] - 2026-07-16

### Added
- **The keychain can now be written, not just read (Phase 4 checkpoint 1).** `hark-keychain` gains `store_key` (rejects empty or whitespace-only keys before the OS backend is ever touched, and trims pasted whitespace), `delete_key` (removing an absent key is success, so the UI Remove button is idempotent), and `key_status` (Stored / Missing / Backend detail; the stored value is read and immediately dropped, never crossing the function boundary). These are the exact slots the settings UI paste field will drive. The no-key-material-in-errors guarantee and the no-test-touches-the-real-keyring rule both hold: backend outcomes are mapped by pure, separately-tested functions.
- **Config gains its schema version stamp and the `[history]` section.** `version = 1` is now written to every saved file so a future breaking change can ship backup-then-migrate cleanly; files predating the stamp load as current-generation. `[history]` carries `capture` (false stores no dictation content while numeric counters still tick), `max_entries` (default 1,000) and `max_age_days` (default 90), both validated >= 1.
- **Settings can be saved, not just loaded.** `Settings::save` validates first, then writes via temp-file-and-rename so a crash mid-save can never leave a truncated config. Serialization omits unset optional fields, stamps the version, and quietly upgrades the legacy `bias_terms` key to `terms` on the next save. A `default_data_dir()` helper resolves the OS data directory for the history database (on Windows it coincides with the config directory).

## [0.9.3] - 2026-07-16

### Added
- **Local dictation storage lands (`hark-store`, Phase 4 checkpoint 0).** New SQLite-backed crate holding dictation history and lifetime stats: WAL journal with a writer/reader two-connection pattern, numbered immutable embedded migrations tracked by `PRAGMA user_version`, and retention pruning (entries older than the age cap and beyond the newest-entries cap are deleted; both caps become configurable in the settings UI). History rows keep the raw transcript and the final injected text plus voice/provider/model labels and per-stage timings. The lifetime stats row is keyed on a fixed id and survives history clears and pruning by construction; "Clear history" and "Reset stats" are independent operations. Privacy semantics are built in: with capture off no transcript content is written while the numeric counters still tick, and the text-bearing types deliberately have no Debug formatting so transcripts cannot leak into logs. Behavior tests cover schema and reopen, paging, case-insensitive literal search (LIKE wildcards neutralized), strict pruning boundaries, and stats independence.

### Changed
- **Building Hark now needs Rust 1.97+** (workspace `rust-version` moved from 1.94): rusqlite 0.40.1's bundled SQLite (3.53.2) build script uses `cfg_select!`, which was unstable before then. The build machine's stable toolchain was updated to 1.97.1 accordingly.

## [0.9.2] - 2026-07-16

### Added
- **Phase 4 planning handoff (`tasks/2026-07-16-handoff-phase4-planning.md`).** Sole starting context for the next session: repo state (v0.9.1 @ `febcb5c`, 236 tests), what Phase 3 deferred into Phase 4 (the BYOK key paste field writing the existing keychain slots, the still-unrun cleanup model spike, the CP5 interactive voice gate), the master plan's Phase 4 scope (rusqlite history/stats, egui settings window, first-run onboarding, tray), load-bearing constraints (UI on main thread, pipeline restart semantics, logging hygiene, the read-only gap in hark-keychain that key entry must close), and the open questions to put to the user.

## [0.9.1] - 2026-07-16

### Changed
- **Phase 3 spec updated with CP0-CP4 implementation lessons and final status.** Checkpoint commits recorded; the parallel voice-enum decision, the HARK_CLEANUP_KEY testing seam for the explicit-provider path, the no-Debug/expect_err note, and the base-URL pre-warm comparison are captured in the spec's Lessons Learned section. The CP0 live spike and CP5 interactive gate remain queued for Phase 4 behind the BYOK UI paste field.

## [0.9.0] - 2026-07-16

### Added
- **Voice cleanup is live in the dictation loop (Phase 3 checkpoint 4).** With a non-verbatim voice configured and a resolvable cleanup provider, every dictation now runs: dictionary pass 1, the word-count gate (short utterances skip the call and log why), one chat-completions rewrite in the chosen voice, dictionary pass 2 (repairs any term the model re-mangled), then injection. Cleanup is fail-open end to end: an unresolvable provider, missing key, or failed request logs one warning and injects the dictionary-corrected transcript unchanged; a dictation is never lost to the optional feature. Inheriting an OpenAI/Groq STT provider reuses the already-resolved STT key with no second keychain read; when the cleanup endpoint differs from the STT one, the worker pre-warms both at startup so the first cleaned dictation skips the cold TLS handshake. Logging stays counts/millis/config-labels only, never text or prompts.
- **`hark-cli --voice <name>`** overrides the configured default voice for one run (`--voice=name` also accepted). Invalid names exit with the list of valid voices; `--voice custom` without a configured `custom_prompt` is rejected at startup with the fix named.

## [0.8.8] - 2026-07-16

### Added
- **The live cleanup adapter (Phase 3 checkpoint 3).** `OpenAiCompatibleChat` implements `CleanupProvider`: one buffered JSON POST to `{base_url}/chat/completions` with Bearer auth, a 10-second per-request timeout (tighter than STT's 15 s, because cleanup failure has a graceful fallback), and no retry, keeping worst-case hot-path latency bounded. The prompt is assembled per request so the protected-terms clause tracks the dictionary terms actually present in the outgoing text. Construction rejects the Verbatim voice (which never calls) instead of panicking, and the adapter's config struct cannot leak the API key or the user's custom prompt through Debug formatting. The live path is a thin shell over the pure request/response layer already under test; network behavior is proven at the deferred live gate.

## [0.8.7] - 2026-07-16

### Added
- **Voices, prompt assembly, and the word-count gate (Phase 3 checkpoint 2).** `hark-voice` gains the `Voice` enum (verbatim / clean / professional / casual / custom) with a case-insensitive parse whose error message lists the valid names, ready for `hark-cli --voice`. Per-request system prompts follow the planned shape: the voice's instruction (custom uses the user's prompt verbatim), a "Leave these terms exactly as written" clause built only from dictionary terms actually present in the outgoing text (case-insensitive, multi-word safe, capped at a ~400-token budget with the same order-is-priority drop rule as STT biasing), and a closing return-only instruction. Verbatim never builds a prompt at all. The `skips_cleanup` gate implements "fewer than N words skips" with 0 disabling the gate and exactly-at-threshold not skipping. The cleanup spike now drives the library's real prompt assembly, so future measurements exercise exactly what the adapter will send.

## [0.8.6] - 2026-07-16

### Added
- **`[voice]` config schema and cleanup-provider resolution (Phase 3 checkpoint 1).** `config.toml` gains `[voice]` (`default` voice, `custom_prompt`, `skip_below_words` gate) and an optional `[voice.provider]` table (`kind`, `base_url`, `model`, `temperature`, `reasoning_effort`, `key_account`), all additive with serde defaults, so existing config files keep loading unchanged. Validation enforces: `custom` voice requires a non-empty prompt, Deepgram is rejected as a cleanup provider (no chat product), and `openai-compatible` requires an explicit base URL. A pure, fully unit-tested resolution function decides the cleanup provider at pipeline build: an explicit table wins; otherwise an OpenAI or Groq STT provider is inherited (same kind and base URL, per-kind chat defaults, and the already-resolved STT key is reused with no second keychain read); otherwise a non-verbatim voice degrades to Verbatim with a one-time startup warning, never a hard error, so the out-of-the-box Deepgram + Clean default keeps working.
- **Keychain resolver generalized for the cleanup role.** `resolve_key_for(env_var, account)` serves any role; `resolve_key` stays as the STT wrapper so existing call sites stand. New `HARK_CLEANUP_KEY` env override; missing-key errors name the role's own env variable and the exact account, including `key_account` overrides. The keychain account remains the provider label, shared between STT and cleanup by design, and the same slots the Phase 4 UI paste field will write.

## [0.8.5] - 2026-07-16

### Changed
- **BYOK key entry moves to the Phase 4 UI (user decision).** No manual key handling in terminals or env vars; API keys will be pasted into a settings field once the UI exists, writing the same keychain slots the resolver reads. The CP0 live model spike and the CP5 interactive gate defer to Phase 4 accordingly; the per-kind chat model defaults (gpt-5-nano at minimal reasoning effort, llama-3.1-8b-instant) ship provisionally from research until live re-verification. CP1-CP4 proceed now on pure functions and mocked tests. Risk stays bounded by the fail-open design: a wrong provider-quirk assumption costs a logged warning and an uncleaned inject, never a lost dictation.

## [0.8.4] - 2026-07-16

### Added
- **`hark-voice` crate: pure chat-completions layer + CP0 cleanup model spike (Phase 3 checkpoint 0).** The new crate mirrors hark-stt's discipline: pure, unit-tested request/response functions (trailing-slash-tolerant URL building; a buffered JSON request body that omits `temperature` and `reasoning_effort` entirely when unset, since the GPT-5 family rejects any non-default temperature with a 400; an output-token cap derived from input length with generous reasoning headroom, clamped to [512, 4096]; response parsing that treats empty or null content as a provider error and names the `finish_reason`, because "length" there means reasoning tokens ate the budget), plus a `CleanupError` taxonomy that can never carry key material and the `CleanupProvider` trait the pipeline will mock at CP4. `cargo run --example cleanup_spike -p hark-voice` (keys from env: `OPENAI_API_KEY`, `GROQ_API_KEY`) rewrites filler-laden fixture transcripts across the four candidate models per voice prompt, measures warm p50/p95, checks protected dictionary terms survive, and drills the plan's open verifications: GPT-5 temperature rejection, `reasoning_effort` acceptance on gpt-5-nano, the error envelope on forced 400/401, transport-error classification on buffered JSON bodies, and reasoning-token headroom. Running the spike with real keys and pinning the plan's default models is the CP0 exit gate.

## [0.8.3] - 2026-07-16

### Added
- **Phase 3 spec: voice layer + cleanup BYOK (`tasks/2026-07-16-phase3-voices.md`).** Planned in full: an optional one-shot chat-completions cleanup call rewrites the transcript in a chosen voice (Verbatim / Clean / Professional / Casual / Custom, Clean default) before injection, via a new `hark-voice` crate mirroring hark-stt's adapter discipline. Locked decisions: voice selection is config + a `hark-cli --voice` flag (tray stays in Phase 4); a word-count gate (default 5, configurable, 0 disables) lets short utterances skip cleanup; the dictionary pass runs both before and after cleanup; cleanup is fail-open at every layer with no retry, so a dictation is never lost to the optional feature; cleanup providers may be inherited from an openai/groq STT config or set explicitly, with the keychain account remaining the provider label. Current chat model candidates (gpt-5-nano, llama-3.1-8b-instant) were verified against provider docs on 2026-07-16, including the GPT-5 temperature lock; a CP0 spike pins final defaults empirically. Six checkpoints, CP5 an interactive real-hardware gate.

## [0.8.2] - 2026-07-16

### Changed
- **Phase 2 (Windows) definition of done is met: the CP6 interactive gate passed user validation.** Dictating with a real dictionary loaded, spoken terms arrived at the cursor with their canonical spellings, decoy sentences of ordinary speech were left untouched, and no latency change was perceptible. The 0.85 Jaro-Winkler threshold held as shipped with no tuning. The collision lesson behind the guard was contributed to LL-G (`kb/rust/phonetic-code-equality-needs-confirm-guard.md`, HIGH). macOS validation remains deferred until Mac hardware; nothing in Phase 2 is platform-specific.

## [0.8.1] - 2026-07-16

### Changed
- **Phase 2 spec updated with CP0-CP5 implementation lessons.** The concrete "matter"/"modero" Double Metaphone collision that makes the Jaro-Winkler guard load-bearing, confirmed rphonetic 3.0.6 behavior, the span-based punctuation approach, and the hyphen tokenization decision are recorded in the spec's Lessons Learned section; status moves to CP6-pending (interactive gate on real hardware).

## [0.8.0] - 2026-07-16

### Added
- **Dictionary correction is live in the dictation loop (Phase 2 checkpoint 5).** Every dictation now runs the phonetic post-correction pass between the provider's transcript and injection: spans that sound like a configured `[dictionary] terms` entry but are spelled wrong arrive at the cursor with the canonical spelling. The corrector is built once at pipeline startup (per-term phonetic codes precomputed off the hot path) and the pass logs counts and millis only ("dictionary: N replacements in X ms"), never transcript text or term content, preserving the Phase 1 logging discipline. Worker-level tests drive a fake provider's misspelled transcript through the retry-and-correct path and assert the exact text handed to injection; the injection I/O itself remains a real-hardware concern for the CP6 gate.

### Changed
- **Whisper-family prompt biasing now respects the 224-token truncation limit.** `prompt_from_bias_terms` includes terms in configured order until a ~200-token budget (4 chars per token heuristic) is spent and drops the rest, instead of joining an unbounded list into a prompt the model would silently truncate. Adapter construction logs "prompt bias: included M of N terms" (counts only). Deepgram's `keyterm` path is unchanged.

## [0.7.6] - 2026-07-16

### Added
- **Multi-word dictionary terms and overlap resolution (Phase 2 checkpoint 4).** Terms split on whitespace and hyphens into word windows: "hark-stt" matches transcript "hark stt" and "hark-stt" (the latter as an uncounted no-op), "nova-3" matches "nova 3" and "Nova-3" but never "nova three" (digit words stay exact-only), and misspelled multi-word phrases correct as a unit ("madero clowd" -> "Modero Cloud"). Matching runs longest term first (word count, then char length), and consumed tokens are skipped, so "Modero Cloud" wins over "Modero" instead of double-firing; separated words never fuse across intervening tokens. Punctuation adjacent to a replaced phrase survives; stray STT-inserted punctuation inside a matched phrase is absorbed with the misrecognition it belongs to.

## [0.7.5] - 2026-07-16

### Added
- **Single-word dictionary matching (Phase 2 checkpoint 3).** `Corrector::correct` now actually corrects: transcript words that sound like a dictionary term but are spelled wrong are replaced with the canonical spelling. Two matching paths, decided per term word at construction: exact-only (case-insensitive equality) for words with digits or of 3 chars or fewer, where Double Metaphone codes degenerate; phonetic (code equality on primary or alternate, either side) confirmed by a Jaro-Winkler score of at least 0.85 as the false-positive guard. Realistic cases proven in tests: "madero" -> "Modero", "vosburg" -> "Vossburg", accent-bridging "muller" -> "Müller", case-insensitive exact hits take canonical casing, already-canonical text is a no-op (not counted), and "matter" (which shares Modero's phonetic code) is left alone by the JW guard, as are common short words. Punctuation around matches survives; unencodable words degrade to exact-only rather than matching everything.

## [0.7.4] - 2026-07-16

### Added
- **Dictionary tokenizer (Phase 2 checkpoint 2).** Transcripts split into word cores with byte spans: leading/trailing punctuation is never part of a span (so replacement splicing preserves it with no reattachment step), interior hyphens split a chunk into separate tokens (so hyphen-split terms like "hark-stt" will match both spellings with one window size), interior apostrophes stay in the core ("don't"), original casing is preserved in the spans with lowercase copies for comparison, and unicode words carry correct multi-byte offsets. 11 tests covering punctuation adjacency, unicode, empty/all-punctuation input, whitespace runs, and hyphenated chunks.

## [0.7.3] - 2026-07-16

### Changed
- **Dictionary config key renamed from `bias_terms` to `terms` (Phase 2 checkpoint 1).** One list now names what it really is: the canonical terms that will drive phonetic post-correction first and provider biasing second. Existing config files keep working via a serde alias (a regression test pins that forever), the committed `config/default-config.toml` documents the new key, and the pipeline reads `terms` when building provider bias configuration.

## [0.7.2] - 2026-07-16

### Added
- **Phase 2 dictionary spec** (`tasks/2026-07-16-phase2-dictionary.md`): the plan for phonetic post-correction of transcripts against the user's canonical terms (names, jargon, product words). Post-correction is the primary mechanism (the spike measured provider biasing as weak); matching is Double Metaphone code equality confirmed by a Jaro-Winkler score, with exact-only handling for words phonetics cannot encode (digits, very short words). Six commit-sized checkpoints ending in an interactive gate on real hardware.
- **`hark-dictionary` crate scaffold (Phase 2 checkpoint 0).** New workspace crate with its final dependencies (`rphonetic` 3.0.6, `strsim` 0.11.1), the `Corrector` API surface (identity pass for now), and proof tests pinning the third-party behavior the matcher will rely on: rphonetic encodes empty/non-ASCII/digit/hyphen inputs without panicking, vowel-swap misspellings produce equal Double Metaphone codes, and strsim's `jaro_winkler` returns 1.0 for equal single-char strings (historical-bug regression guard) and clears the planned 0.85 threshold for the flagship "madero" -> "modero" case.

## [0.7.1] - 2026-07-16

### Changed
- **Phase 1 (Windows) definition of done is met: the CP6 interactive gate passed user validation.** Hold Left Ctrl + Left Win, speak, release injects the transcript at the cursor with no issues: pre-roll captured early words, the clipboard was restored after paste, and the synthesized paste did not re-trigger recording. Recorded in the spec's Lessons Learned along with the Doppler dev-run note (secrets are provider-named, so `HARK_STT_KEY` must be mapped explicitly rather than relying on `doppler run`). macOS parity (checkpoint 7) is deferred until Mac hardware is available.

## [0.7.0] - 2026-07-16

### Added
- **`hark-cli` dev binary (checkpoint 6): the Windows dictation loop is wired end to end.** `cargo run -p hark-cli` loads settings from the OS config dir (defaults when absent), resolves the BYOK key (env override first, then keychain; a missing key exits with the actionable fix, code 3, never a panic), starts the pipeline, and parks on Ctrl+C with a clean staged shutdown (hook -> worker -> capture). Startup was smoke-tested live on this box: capture came up at 48 kHz F32 on the dedicated COM thread, the `WH_KEYBOARD_LL` hook installed, and the Deepgram pre-warm completed in 218 ms. Log output is structurally free of key material, raw audio, and transcript text (lengths/counts/millis only, grep-verified). The interactive hold-speak-release gate awaits a human at the keyboard; the spec's Lessons Learned section now records this session's discoveries (rubato 4.0 API restructure, cpal 0.18 deltas, the chord decision and Start-menu behavior, the connect-class string contract).

## [0.6.0] - 2026-07-16

### Added
- **Pipeline orchestration (`hark-pipeline`, checkpoint 5).** The pure state machine (`Idle -> Recording -> Transcribing -> Injecting -> Idle`) is total: every state/event pair is defined, stray releases and duplicate presses are inert, presses arriving mid-dictation are ignored rather than queued, and any failure aborts cleanly back to `Idle` (never a panic). The retry predicate honors the spike verdict exactly: one retry, only for `Timeout` and connect-class `Http`; `Auth`, `RateLimited`, `Provider`, `BadAudio`, and non-connect transport failures (which may already have reached the provider) never retry. A localhost contract test pins the connect-class detection against the frozen `hark-stt` transport mapping so drift is caught at test time. The worker loop stamps chord edges against the audio clock at processing time (pre-roll absorbs hook-to-worker latency), assembles/gates/encodes/transcribes/injects, treats empty transcripts as inject-nothing, and pre-warms the shared HTTP client on the worker thread at startup (the spike measured 0.4-0.9 s cold cost). `run(settings, api_key)` maps settings onto the frozen `hark-stt` contract (Groq/OpenAI/custom all ride the OpenAI-compatible adapter), spawns capture + hook + worker threads, and returns a handle whose drop tears everything down in dependency order. Integration tests drive the full pre-STT path (ring -> window -> gate -> WAV) with the committed spike fixture, asserting sample counts end to end.

### Changed
- **`hark-audio` capture sizing and handoff.** `start()` now takes ring seconds instead of a sample count (the device rate is only known once the stream config resolves; capacity is computed against the live rate) and returns the ring `Consumer` by value so it can move into the pipeline worker. New `window::ring_seconds` helper with a test proving it always covers `ring_capacity` at any rate.

## [0.5.0] - 2026-07-16

### Added
- **Text injection (`hark-inject`, checkpoint 4).** The clipboard path runs the full stash -> set -> read-back verify -> synthesized Ctrl+V -> restore sequence, with every clipboard operation inside a bounded retry loop (the clipboard is a global object; another process can hold it) and tunable settle delays around the paste (no OS-guaranteed timing exists; the verify catches sets that did not take). Every clipboard-side failure falls back to char typing, which never touches the clipboard; only key-synthesis failure is terminal (typing rides the same machinery). Restore failure after a successful paste is a warning, not a failed dictation, and empty transcripts are a strict no-op. The text-only clipboard round-trip (images/RTF/HTML clobbered) is documented as the accepted v1 limitation in the new `crates/hark-inject/CLAUDE.md`, alongside the enigo 0.6.1 pin rationale (its injected-flag contract guards against PTT feedback loops and has regressed upstream before). Retry policy, fallback decisions, and strategy selection are pure and tested (8 tests); the clipboard/key I/O itself is run-on-real-HW, including the checkpoint-6 check that our own hook ignores our own synthesized paste.

## [0.4.0] - 2026-07-16

### Added
- **Push-to-talk source (`hark-hotkey`, checkpoint 3).** A pure chord state machine (`edges.rs`) turns raw per-key events into clean `Down`/`Up` edges for the configured chord: engage when the last member goes down, release when the first lets go, auto-repeat suppressed, keys outside the chord ignored, and injected events (our own future synthesized Ctrl+V, `LLKHF_INJECTED`) dropped so dictation can never re-trigger itself. Chord strings like `"LCtrl+LWin"` parse case-insensitively with helpful errors (modifiers, CapsLock, F1..F24; up to 4 keys). The Windows listener installs `WH_KEYBOARD_LL` on a dedicated thread whose entire body is the message pump (the hook's delivery lifeline), always calls `CallNextHookEx` (observe, never swallow), and shuts down cleanly via `WM_QUIT`. The `spawn_listener` boundary is the platform seam the macOS CGEventTap implementation will slot behind in checkpoint 7. 14 new tests (edge semantics + VK mapping); hook install itself remains run-on-real-HW.

## [0.3.0] - 2026-07-16

### Added
- **Audio capture core (`hark-audio`, checkpoint 2).** Four layers, three of them pure and fully unit-tested on any machine: a lock-free SPSC ring buffer whose samples are atomic bit patterns with an absolute sample counter (read-by-index across wraps, with "not yet produced", "already overwritten", and lapped-mid-copy all detected rather than silently torn); rubato 4.0 whole-clip resampling to 16 kHz mono via `process_all` (exact 3:1 from 48 kHz and the non-integer 44.1 kHz ratio both asserted to the sample, startup-delay trim verified by a head-signal test); and pre-roll/tail window math with the two-stage silence gate (hold-duration misfire check before any waiting, RMS check on the assembled clip) so silence never costs a network request. Over-cap holds keep the most recent audio. The cpal glue builds the stream on a dedicated COM-owning thread with an allocation-free callback (cpal #970 discipline) and requires `SampleFormat::F32` explicitly; live capture remains flagged run-on-real-HW. 35 new tests; `crates/hark-audio/CLAUDE.md` records the callback and COM rules.

## [0.2.0] - 2026-07-16

### Added
- **Settings loader (`hark-config`, checkpoint 1).** TOML settings with sane defaults for every key: provider presets (deepgram / openai / groq / openai-compatible, with per-kind default base URLs and models; Deepgram nova-3 is the app default per the spike verdict), the push-to-talk chord (`LCtrl+LWin`), audio timing knobs (300 ms pre-roll, 150 ms tail, 120 s max hold, silence-gate thresholds), injection strategy and clipboard timing, and the Phase 2 `bias_terms` placeholder. Unknown keys are tolerated for forward compatibility; a missing config file yields defaults; `openai-compatible` without an explicit `base_url` and blank PTT chords are rejected at load. The committed `config/default-config.toml` documents every default and where user config lives per OS.
- **BYOK key resolution (`hark-keychain`, checkpoint 1).** `resolve_key(provider)` checks the `HARK_STT_KEY` env override first (dev/CI path; blank values are treated as unset) and only then the OS keychain (service `hark`, account = provider label). Both-absent produces a clear actionable error, never a panic. No type in the crate carries key material, and a regression test formats every failure path with a sentinel key in the environment to prove nothing leaks into Debug/Display output. 14 new unit tests across both crates.

## [0.1.4] - 2026-07-16

### Added
- **Phase 1 pipeline scaffolding (checkpoint 0).** Seven new workspace crates with their final dependency blocks and empty sources: `hark-audio` (cpal 0.18.1 + rubato 4.0), `hark-hotkey` (windows-rs 0.62.2, Windows-only target dep), `hark-inject` (arboard 3.6.1 + enigo 0.6.1, both without default features), `hark-keychain` (keyring pinned `=4.1.5`, `v1` backend bundle), `hark-config` (serde + toml 1.x), `hark-pipeline` (glue crate depending on all of the above plus the frozen `hark-stt`), and the `hark-cli` dev binary (ctrlc + env_logger). Whole-workspace build, clippy `-D warnings`, fmt, and the existing 20 `hark-stt` tests all green; `hark-stt` itself is untouched.
- **Confirmed Phase 1 defaults from user review:** push-to-talk defaults to a Left Ctrl + Left Win chord (user-configurable), the post-release tail window is configurable with a 150 ms default, and the max-hold cap is 120 s (transcribe-what-we-have on exceed).
- **rubato 4.0 research baked into `hark-audio`'s manifest:** the 2026-07-09 v4 release replaced the old `FftFixedIn`/`SincFixedIn` types with consolidated `Fft`/`Async` resamplers; whole-clip resampling must go through `process_all()` (trims FFT startup delay, exact output counts) rather than a single oversized `process()` call.

## [0.1.3] - 2026-07-16

### Added
- **Phase 1 pipeline spec** (`tasks/2026-07-16-phase1-pipeline.md`): the plan for the full dictation loop now that the STT spike passed. Crate layout follows the master plan's decomposition (`hark-audio`, `hark-hotkey`, `hark-inject`, `hark-keychain`, `hark-config`, `hark-pipeline` library + thin `hark-cli` dev binary; tray/egui UI stays a later phase). Eight commit-sized checkpoints from workspace scaffolding through a Windows end-to-end run, with macOS parity (CGEventTap) explicitly marked as needing real Mac hardware. Load-bearing gotchas are baked in up front: WASAPI won't deliver 16 kHz (resample in-process), `WH_KEYBOARD_LL` needs its own message pump thread, our injected Ctrl+V must be filtered via `LLKHF_INJECTED` so the hook doesn't re-capture it, and arboard clipboard restore only round-trips text formats.

## [0.1.2] - 2026-07-16

### Changed
- **Phase 1 STT spike completed with live measurements — verdict: Deepgram nova-3 is the default provider.** With valid keys (now stored in Doppler project `hark`, config `prd`, injected via `doppler run`), the full harness ran against all three providers: Deepgram nova-3 p50 150 ms / p95 630 ms, OpenAI gpt-4o-mini-transcribe p50 789 / p95 1223, Groq whisper-large-v3-turbo p50 944 / p95 1527 (N=20 warm runs on a 10.3 s clip). Cold-client penalty is 0.4-0.9 s across providers, so the pipeline will pre-warm the shared HTTP client at launch. The Deepgram keyterm A/B showed no lift on the clean TTS clip (5/5 recognition in both arms) while Groq's prompt biasing failed to enforce the spelling of "Levenshtein", reinforcing phonetic post-correction as the primary dictionary path. A real Groq 429 was handled correctly (Retry-After parsed). Spike acceptance criteria are all green; results recorded in the spec's Lessons Learned section.

## [0.1.1] - 2026-07-15

### Changed
- **Spike spec §12 (Lessons Learned) filled in** with the implementation findings: the reqwest 0.13 TLS feature rename, the multipart-streaming error-masking gotcha (both routed to LL-G `kb/rust/`), measured failure-drill bounds (bad key ~65-130 ms, dead DNS <20 ms, non-routable host at the 3 s connect bound), negligible WAV-encode cost (~3.7 ms for a 10 s clip), and the Windows TTS fixture-generation recipe. Live p50/p95 latency, cold-vs-warm delta, and the Deepgram keyterm A/B remain open: the `OPENAI_API_KEY` in the dev environment is rejected by OpenAI (401 on a bare `/v1/models` probe too) and no Groq/Deepgram keys are set; re-run the harness once valid keys exist. `.claude/agent-memory/patterns.md` gains the buffered-multipart and pure-error-mapping patterns.

## [0.1.0] - 2026-07-15

### Added
- **Both Phase 1 cloud STT adapters** (spike checkpoints 1-4). `openai_compatible` posts hand-assembled multipart to `{base_url}/audio/transcriptions` with Bearer auth (one code path for OpenAI and Groq; bias terms ride the `prompt` field), and `deepgram` posts raw `audio/wav` to `/v1/listen` with `Token` auth, `smart_format`, and repeated `keyterm` params for dictionary biasing. Both sit behind `hark_stt::build()` and share one long-lived rustls HTTP client (3 s connect / 15 s total timeouts).
- **The spike harness** (`cargo run --example transcribe_spike`): per configured provider it prints the fixture transcript with edit-distance divergence, a cold-vs-warm latency table (N warm runs, p50/p95/min/max, separate WAV-encode timing), the Deepgram keyterm A/B, live failure drills (bad key, dead DNS, non-routable IP), and a default-provider + retry-policy verdict. Providers without keys are skipped with an explicit message; a print-time self-check guarantees no API key can appear in any report line.
- **Pure-logic test suite** (`tests/adapter_pure.rs`, 20 tests): multipart body assembly and boundary-collision avoidance, Deepgram keyterm URL encoding, HTTP-status to error-taxonomy mapping (401/403/429/500 with Retry-After and snippet truncation), latency percentile math, and WAV encode/parse round-trips validated against `hound`.

### Fixed
- **Connect/timeout errors no longer masquerade as body errors.** reqwest's own multipart streams the body through a channel, so connect failures surfaced as opaque "send failed because receiver is gone" with `is_connect()`/`is_timeout()` false, wrecking the retry taxonomy. The adapter now buffers a hand-built multipart body; DNS failure classifies as a connect-class `Http` error in ~4 ms and a non-routable host as `Timeout` at the 3 s connect bound.

## [0.0.3] - 2026-07-15

### Changed
- **`hark-stt` crate rebuilt for BYOK cloud (spike checkpoint 0).** The sherpa-onnx dependency and its native-lib auto-download are gone; the crate now compiles with pure-Rust dependencies only (`reqwest` 0.13 blocking + rustls, `serde`, `thiserror`). The public surface is the new cloud `SttProvider` trait, `ProviderConfig` (with a redacting `Debug` so API keys can never leak into logs), the `SttError` taxonomy (`Http`/`Auth`/`RateLimited`/`Timeout`/`BadAudio`/`Provider`), a WAV encode/parse helper for the 16 kHz mono PCM16 contract, and latency-percentile metrics. Note: reqwest 0.13 renamed the 0.12 TLS feature `rustls-tls-webpki-roots` to `rustls` + `webpki-roots`.

### Added
- **Committed spike fixture** (`crates/hark-stt/fixtures/spike_clip.wav` + `expected.txt`): a ~10 s 16 kHz mono English clip with known transcript, containing the dictionary-ish terms "Hark" and "Levenshtein" for the upcoming Deepgram keyterm A/B.

## [0.0.2] - 2026-07-15

### Changed
- **STT pivoted from on-device to BYOK cloud (multi-provider).** The v1 plan's local sherpa-onnx + Parakeet TDT 0.6B stack (~1.1 GB of model assets) is replaced by cloud transcription using the user's own API keys: an `SttProvider` trait with an OpenAI-compatible adapter (OpenAI `gpt-4o-transcribe`/`whisper-1`, Groq `whisper-large-v3-turbo`; one shared multipart contract) and a Deepgram adapter (nova-3, `keyterm` dictionary biasing). Transport is `reqwest` blocking on pipeline worker threads; audio uploads as 16 kHz mono WAV. History, stats, dictionary, and settings remain strictly local; a small opt-in local fallback model (~75 MB) is recorded as a later-phase option. `tasks/plan-repo.md` rewritten as v2.
- **Phase 1 STT spike spec rewritten** (`tasks/2026-07-15-phase1-stt-spike.md`): now proves the cloud path (both adapters, real p50/p95 latency on 2-15 s clips, a Deepgram `keyterm` A/B, and an error taxonomy with a retry-policy verdict) instead of ONNX decode + hotwords. Runnable entirely on the Windows dev box; no per-OS inference runtime remains.
- **Project rules updated for the pivot:** root `CLAUDE.md`, `.claude/rules/rust.md`, and `.claude/references/design-guardrails.md` now encode the cloud latency SLA (one long-lived HTTP client with keep-alive/TLS resumption, at most one retry on timeout), first-class offline/key-rejected/provider-error UI states, and the honest disclosure that every dictation sends audio to the user's chosen STT provider.

### Added
- **Cloud STT research** in agent memory (`.claude/agent-memory/explorer/hark_cloud_stt_providers.md`, `hark_cloud_stt_rust_stack.md`): provider comparison with pricing and gotchas (Groq's 10 s billing minimum, Deepgram's `keyterm` vs legacy `keywords` split, pre-1.0 `deepgram` crate) and the verified Rust dependency set (`reqwest` 0.13 blocking, `hound`, `flacenc` as upgrade path, `whisper-rs` + `tiny.en` as the future fallback candidate).
- **Workspace scaffolding from the v1 spike** committed as the base the v2 spike rewrites: root `Cargo.toml`, `rustfmt.toml`, and the `crates/hark-stt` skeleton. Note: the skeleton still declares the `sherpa-onnx` dependency, which auto-downloads a large native lib; spike checkpoint 0 removes it, so do not build the workspace before that lands.

### Removed
- **Local model assets and fetch script:** the ~1.1 GB Parakeet ONNX download and `scripts/fetch-model.sh` are gone, along with the now-stale `/models/` `.gitignore` entry. The eventual installer shrinks from ~1.5 GB to tens of MB.

## [0.0.1] - 2026-07-15

This repository is now the home of **Hark** — an offline, single-user, push-to-talk voice dictation desktop app for Windows and macOS (Rust). This release repurposes the Claude Code starter template into Hark's project scaffolding and plans the first build phase. Versioning **resets to `0.0.1`** for Hark as a new product; the `0.7.0` and earlier entries below are the starter template's history, retained for record.

### Added
- **Hark project plan** (`tasks/plan-repo.md`) and a rewritten `README.md`. plan-repo was adapted rather than run literally: the web-app infrastructure and stack-research machinery don't apply to an offline desktop app, so the already-decided stack (Rust, `cpal`, sherpa-onnx/Parakeet TDT, `egui`, `rusqlite`, `keyring`) is captured with current-as-of-2026-07-15 research corrections.
- **Phase 1 STT spike spec** (`tasks/2026-07-15-phase1-stt-spike.md`): a runnable spec to prove the `sherpa-onnx` crate loads Parakeet TDT 0.6B v2, decode latency, execution-provider availability on macOS/Windows, and an A/B measurement of the known hotword-biasing bug (sherpa-onnx #3267), ending in a go/no-go verdict for Phase 2.
- **Desktop design guardrails** (`.claude/references/design-guardrails.md`) for native egui UI: the main-thread rule, latency SLA, desktop accessibility, and a local-first privacy section in place of web auth-UI patterns.
- **New path-scoped rules** `rust.md` (Rust/desktop conventions + verified stack gotchas) and `tests.md` (Rust test conventions); a `.claude/bp-audit.md` audit trail.
- **Stack-risk research** captured in agent memory (`.claude/agent-memory/explorer/hark_stt_stack_risk.md`, `sherpa_onnx_rust_api.md`): `sherpa-rs` is deprecated (use the official `sherpa-onnx` crate v1.13.4+), and push-to-talk needs native key hooks (CGEventTap / `WH_KEYBOARD_LL`) rather than the `global-hotkey` crate.

### Changed
- **`.claude/` config retargeted from the web-app template to a Rust desktop app.** Root `CLAUDE.md` rewritten for Hark's stack and the UI-on-main-thread / pipeline-on-workers rule, dropping the Northflank/Cloudflare/Better Auth infrastructure rules (which do not apply). `llg-check.md` and `bp-check.md` path globs retargeted to Rust (`crates/**`, `**/*.rs`, `**/Cargo.toml`, `rustfmt.toml`, …), and `tools.md`'s stack section swapped web tooling for the Rust chain (cargo, clippy, nextest, cross-compile, notarization) while preserving the MCP servers section.
- **`package.json`** name/description updated from the bootstrap template to Hark.
- **`.claude/agent-memory/debugging.md`** seeded with the HIGH-severity LL-G Rust/SQLite gotchas relevant to Hark plus the sherpa-onnx #3267 finding.

## [0.7.0] - 2026-07-08

### Changed
- **plan-repo skill overhauled.** Research now runs in two explicit waves (language + frontend first, then the four prompts that depend on those picks), every subagent prompt embeds the literal resolved date instead of "today's date", candidate lists are marked as seeds that subagents must refresh against current search results, and a failed subagent no longer stalls the skill. The skill now consults LL-G and BP before researching (RULE 1 + RULE 3) so HIGH-severity gotchas can demote candidates and pre-seed the plan's Lessons Learned section. The SPA-vs-SSR serving mode is recorded as an explicit decision in the recommendation and saved plan. Re-running the skill with an existing `tasks/plan-repo.md` now asks whether to revise or archive instead of overwriting. The optional project-description argument is actually consumed. Frontmatter tightened: `disable-model-invocation: true` and the Agent tool restricted to `Agent(explorer)`.
- **Agent roster hardened.** Read-only review agents (reviewer, performance, security, ux-reviewer, architect) drop `permissionMode: plan` and instead gain `Write` scoped solely to saving reports/plans under `tasks/`, with an explicit "never modify source" instruction; review agents also get `maxTurns` budgets. The security agent moves to `effort: xhigh`, adds package-manager-aware audit commands (`pnpm`/`yarn` audit, `pip-audit`) plus `git log`/`ls-files`/`check-ignore` for history and tracked-secret checks, and its scan categories are rebuilt around the 2025 OWASP Top 10 with value-shaped secret patterns. The builder agent runs in an isolated git worktree (`isolation: worktree`); explorer and tester gain `memory: project` so they read agent-memory before acting. The performance agent adds a hot-path verification step and measurement-to-confirm guidance.
- **Skills refreshed to current Claude Code practices.** security-scan, update-practices, spec-developer, ux-review, init-repo, performance-review, test-scaffold, dependency-audit, and doc-sync were revised for accuracy and current tooling; init-repo gains `AskUserQuestion`. CLAUDE.md adds RULE 0 (Read-Only First), documents hook-script portability and the new template-sync files, and `instructions.md`/`agents.md` are updated to match.
- **Template sync now tracks state.** New `.claude/references/template-sync-ignore.md` lets a project record files it deliberately removed so `update-practices` will not re-create them, alongside a `template-sync-state.json` for last-synced commit and dead-URL strikes.

### Removed
- **`agy-execute-plan` skill removed** (SKILL.md and its evals), superseded by the standard plan/execute workflow.

### Fixed
- **Stripe is no longer assumed.** plan-repo previously wrote Stripe env vars into every README; payments now enter the plan only when the requirements interview says the project takes them (Stripe as the default provider).
- **Gotcha routing aligned with CLAUDE.md.** plan-repo and init-repo told sessions to route post-implementation discoveries to the local `.claude/agent-memory/debugging.md`; both now route to LL-G via `/add-lesson`, and the section name is standardized to "Lessons Learned / Gotchas".

## [0.6.0] - 2026-07-08

### Added
- **New `repo-review` skill.** A general code health review of the whole repository that complements the specialized scans: it checks repo hygiene (tracked junk, oversized files, config drift), correctness and error handling gaps, maintainability (dead code, duplication, naming, premature abstraction), and configuration consistency, then produces a severity-ranked report where every finding includes a specific fix. Instead of duplicating the deep skills, it does a light pass on security, performance, tests, dependencies, docs, and UX, and routes real signal to security-scan, performance-review, test-scaffold, dependency-audit, doc-sync, or ux-review as follow-ups. Bound to the reviewer agent and triggered with "repo review".

## [0.5.2] - 2026-07-05

### Added
- **Hooks & settings catalog expanded to Claude Code 2.1.201 (July 2026).** `hooks-and-settings.md` now documents hook structured output (`updatedToolOutput` on PostToolUse, `additionalContext` on Stop/SubagentStop, `reloadSkills`/`sessionTitle` on SessionStart), `Tool(param:value)` parameter matching (e.g. `Agent(model:opus)`), HTTP hook custom headers with env-var interpolation, a `PermissionRequest` prompt-hook auto-approval pattern, new settings (`defaultMode`, `fallbackModel`, `enforceAvailableModels`, `disableBundledSkills`, `requiresMinimumVersion`, `attribution.sessionUrl`, `autoMode.*`), the full six-tier settings precedence chain, the `ENABLE_PROMPT_CACHING_1H` cache lever, and the v2.1.196 security change that stops committed MCP servers from auto-spawning.
- **New frontmatter capabilities documented.** `user-invocable: false` for hidden background-knowledge skills, `Agent(agent_type)` tool-allowlist entries to restrict which subagents an agent can spawn, and nested `.claude/` directories as a first-class per-subfolder convention (closest wins, `<dir>:<name>` collision naming).
- **Tools reference refreshed for mid-2026.** Biome promoted to the BP-recommended default for new JS/TS projects, `oxlint` and `rolldown` added (Vite 8+ bundles via Rolldown), eslint repositioned for plugin-dependent codebases, and the Prisma entry updated for v7's pure TS/WASM client with native edge support.

### Changed
- **Docs caught up with the agent roster.** `instructions.md` now covers the `builder` and `tester` agents, the `agy-execute-plan` skill, `hooks-and-settings.md`, and per-agent memory folders that already existed in the repo but were missing from the folder map and reference sections.
- **CLAUDE.md notes that subagents now run in the background by default** and can nest up to 5 levels; stale "see init-repo skill" pointers now point at the hooks-and-settings catalog.

### Removed
- **Generic coding-standard bullets pruned from CLAUDE.md** (clear code, descriptive names, small functions) per the "remove what the model handles natively" rule, keeping the file under the 200-line cap.

## [0.5.1] - 2026-06-14

### Added
- **Builder agent memory: skill-propagation pattern** (`.claude/agent-memory/builder/feedback_skill_propagation.md`). Captures the verified, safe sequence for propagating template skills into downstream repos (read every target before writing, stage-then-chmod `kb-upsert.sh`, add the `.gitattributes` LF rule before committing the script, re-read `package.json` right before bumping because a hook may auto-bump it, never sync `infrastructure.md`, and always route exploration to the custom `explorer` agent). Distilled by the builder agents during the cross-repo propagation run.

## [0.5.0] - 2026-06-14

### Added
- **New `agy-execute-plan` skill.** Hands an existing Claude-written plan to the Antigravity CLI (`agy`) for autonomous end-to-end execution, then independently verifies the result against the plan's acceptance criteria using the test suite and the git diff (not AGY's self-reported log), fixes whatever AGY left incomplete or broke, and reports an honest blocked/partial/complete status. Encodes the verified `agy` v1.0.8 operating knowledge a fresh session would otherwise have to rediscover: run headless with an empty stdin and `--dangerously-skip-permissions` or it hangs forever, print-mode stdout is empty when redirected (judge by diff + tests), the Windows PATH-reload step, the `AGY_BLOCKED.md` halt signal, and the set of flags that actually exist in v1.0.8.

## [0.4.0] - 2026-06-14

### Added
- **Bundled `.claude/scripts/kb-upsert.sh`.** A portable create-or-update helper for the GitHub contents API that captures each file's blob SHA immediately before writing and base64-encodes without the GNU-only `base64 -w0` flag. The `add-lesson` and `add-practice` skills now call it instead of hand-running ~8 `gh api` calls each with manual SHA threading, removing a fragile, duplicated sequence and a macOS portability landmine.
- **New `.claude/references/hooks-and-settings.md`.** A single canonical catalog of every hook event, the five hook types, matcher syntax, and all `settings.json` options. `init-repo` and `update-practices` now point at it instead of each carrying their own copy, so the lists can no longer drift apart.

### Changed
- **Knowledge-base skills route exploration to the custom `explorer` agent.** `spec-developer`, `mermaid-diagram`, `plan-repo`, `init-repo`, and `update-practices` previously spun up the built-in `Explore` subagent, which loads every connected MCP tool schema and exceeds the context window (the exact failure CLAUDE.md warns against). They now use the scoped `explorer` agent and say why; `doc-sync`'s explorer references were made explicit too.
- **`add-lesson`, `add-practice`, and `apply-practice` frontmatter normalized.** Added pushy, trigger-phrase-rich descriptions and the `user-invocable`, `argument-hint`, and least-privilege `allowed-tools` fields the other skills already declare.
- **Consistent model routing.** Every standalone skill now pins `model:` (haiku for the mechanical KB writers, sonnet for analysis, opus for orchestration); agent-bound skills continue to inherit their agent's model.
- **`init-repo` slimmed from 491 to 396 lines** by moving the hook/settings reference tables into `hooks-and-settings.md`, bringing it back under the 500-line guideline.

### Fixed
- **Corrected a false claim** in `add-lesson`/`add-practice`/`apply-practice` that the GitHub MCP server "does not exist and will hang the skill." A GitHub MCP server can be connected; the guidance now explains the real reason to stay on `gh`/`WebFetch` (avoid loading MCP schemas mid-skill).
- **Removed the dead `Agent` tool** from `security-scan`, `performance-review`, and `ux-review` allowed-tools — each is bound to a read-only agent that lacks the `Agent` tool, so the entry was impossible and unused.
- **Dropped a phantom `Error` hook event** that `init-repo` referenced; the new reference uses the real `StopFailure` event.

## [0.3.1] - 2026-06-14

### Changed
- **Rewrote the agent-memory README** (`.claude/agent-memory/README.md`) to a more prescriptive version ported from another project. Adds a numbered Rules section covering append-only edits, the 200-line context-injection limit per memory file, and topic-based partitioning when files grow, plus clearer entry-format and activation guidance.

## [0.3.0] - 2026-06-14

### Added
- **New `builder` agent** (`.claude/agents/builder.md`). The template's first implementation-capable agent: a scoped Read/Glob/Grep/Edit/Write/Bash role (`sonnet`, effort `high`, `permissionMode: acceptEdits`, `memory: project`) that turns a plan or spec into working, tested code matching existing conventions. It fills the gap that enabling agent teams exposed: every prior agent was read-only, so the only way to spawn an implementing teammate was the built-in `general-purpose` type that CLAUDE.md bans for blowing the context window. Builders own a file set and coordinate via messaging rather than editing across boundaries, making parallel feature and cross-layer work possible without conflicts.
- **New `tester` agent** (`.claude/agents/tester.md`). A Read/Glob/Grep/Bash role (`sonnet`, effort `medium`) that detects the project's test runner rather than assuming one, runs the relevant suite, and reports pass/fail with actual failure output and a likely-cause classification. It verifies behavior and never edits source, pairing with `builder` to complete the cross-layer team loop (one teammate builds, one verifies).
- Both agents registered in `agents.md` (full entries) and the CLAUDE.md key-agents list.

## [0.2.1] - 2026-06-14

### Added
- **Agent teams enabled project-wide.** `.claude/settings.json` now sets `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS: "1"` in `env`, so every session cloned from this template can coordinate multiple Claude Code instances (shared task list, inter-agent messaging) without per-machine setup. Agent teams are experimental and require Claude Code v2.1.32 or later; the flag is read at session start, so a restart is needed for it to take effect. Subagents (the `Agent` tool) need no flag and remain on by default.

## [0.2.0] - 2026-06-09

### Added
- **Per-skill and per-agent `effort:` frontmatter.** All 15 skills and all 6 agents now pin an effort level matched to their workload: `low` for mechanical step-by-step skills (add-lesson, add-practice, mermaid-diagram), `medium` for analysis and guided-edit skills and the sonnet agents (reviewer, performance, explorer, ux-reviewer), and `high` for orchestration, planning, and high-stakes analysis (init-repo, plan-repo, update-practices, spec-developer, security-scan, architect, security). Lightweight skill invocations no longer inherit session-level effort they do not need.
- **`disable-model-invocation: true` on the `merge-worktrees` skill.** The skill force-deletes branches and worktrees, so it should never auto-trigger; it now runs only when explicitly invoked with `/merge-worktrees`.

### Changed
- **`doc-sync` allowed-tools now declares `Agent`** instead of the pre-rename `Task` tool, completing the cleanup that 0.1.2 applied to the other skills.
- **`agents.md` documents each agent's effort level** alongside its model.

## [0.1.2] - 2026-06-09

### Fixed
- **Commit hook scripts now self-filter on the actual command instead of trusting the `if` rule.** Discovered live after 0.1.1 made the hooks active: the `if: "Bash(git commit*)"` rule fires conservatively on commands containing opaque command substitutions (verified: a `gh api ... -f content="$(base64 ...)"` upload with no git commit in it was blocked by the changelog gate). All four hook scripts now read the hook input JSON and exit 0 unless the command actually contains a git commit invocation, treating `if` as an optimization rather than the guard. Both gotchas from this work were contributed to the LL-G knowledge base under the new `claude-code` technology.

## [0.1.1] - 2026-06-09

### Fixed
- **The git-commit hooks never fired.** All four commit hooks in `.claude/settings.json` used `"matcher": "Bash(git commit*)"`, but hook matchers only match tool names (verified against the official hooks reference and confirmed empirically: a `git commit` with nothing staged ran unblocked). Changed each group to `"matcher": "Bash"` with `"if": "Bash(git commit*)"` on every handler, the documented way to filter on tool arguments. The here-string guard, both changelog gates, and the post-commit knowledge-base prompt are now live.
- **The changelog staged-check no longer falsely blocks compound commands.** `check-changelog-staged.sh` runs before the command executes, so `git add CHANGELOG.md ... && git commit ...` was blocked because the changelog was not staged yet at hook time. The script now reads the hook input and allows any command that stages `CHANGELOG.md` itself.
- **`disable-model-invocation` was spelled with underscores and silently ignored.** The `mermaid-diagram` and `spec-developer` skills used `disable_model_invocation: true`, which Claude Code does not recognize, so both skills remained auto-invocable despite the manual-only intent. Corrected to the hyphenated key (both skills now disappear from the model-invocable list) and fixed the two `instructions.md` passages teaching the underscore spelling.
- **The pre-commit changelog reminder still described the retired 4-segment version scheme.** `pre-commit-changelog-reminder.sh` told Claude to bump `Major.Minor.Patch.Build` with "Build: every commit," contradicting the 3-segment SemVer rule adopted in 0.0.1. The hook text now matches `.claude/rules/commit-changelog.md`.
- **`.gitignore` no longer ignores `Cargo.lock`** (lockfiles should be committed), and the GOPATH-era `bin/` and `pkg/` entries plus the duplicate `build/` under Java/Kotlin were removed so projects' real `bin/`, `pkg/`, and `build/` directories are not silently excluded.

### Changed
- **`instructions.md` caught up with the template's current contents.** Added the `ux-review` and `merge-worktrees` skills, the `ux-reviewer` agent, and `references/ux-laws.md` to the folder structure and reference sections.
- **Skill `allowed-tools` lists now use the current `Agent` tool name.** `performance-review`, `security-scan`, `test-scaffold`, and `ux-review` still listed the pre-rename `Task` tool; all skills now consistently declare `Agent`.

## [0.1.0] - 2026-06-05

### Added
- **New `merge-worktrees` skill** (triggered by "merge worktrees"). Consolidates outstanding work into the repository's main branch and tears down the leftovers: it inventories every worktree and local branch, detects the real main branch (no `main` assumption), shows a plan and asks for confirmation, commits pending work in each worktree, merges every branch into main with `--no-ff`, pushes, then removes the worktrees and force-deletes the merged branches (locally and, with confirmation, on the remote). Merge conflicts and non-fast-forward pulls are hard stops, not auto-resolved, and nothing is deleted until the merge is committed and pushed. Registered in the CLAUDE.md skills index.

## [0.0.3] - 2026-06-05

### Removed
- **Deleted the completed `tasks/peaceful-whistling-dolphin.md` plan file.** The Learning Lessons / Gotchas (LL-G) system it described has been implemented, so the plan is no longer needed.

## [0.0.2] - 2026-06-03

### Fixed
- **`doc-sync` skill no longer assumes the docs folder is capital-D `Docs/`.** On Windows and macOS the filesystem is case-insensitive, so a pre-existing lowercase `docs/` folder satisfied the old hardcoded `Docs/` existence check while the skill kept writing to and citing `Docs/`, creating confusion (and a second, divergent folder on case-sensitive Linux/CI/git). The skill now resolves the docs root once, case-insensitively, preferring the casing git actually tracks (`git ls-files`), reuses that exact name for the whole run, and only defaults to `Docs/` when no docs folder exists.

## [0.0.1] - 2026-06-01

### Changed
- **Switched versioning from 4-segment `Major.Minor.Patch.Build` to 3-segment SemVer `Major.Minor.Patch` and reset the version to `0.0.1`.** Updated the scheme table and rules in `.claude/rules/commit-changelog.md` (the Build segment is removed; the Patch segment now also covers docs, refactors, config, and chores). Prior `1.x.x.x` entries below are retained as historical record.

### Fixed
- **Path-scoped rules were using Cursor's frontmatter dialect and silently not scoping.** `.claude/rules/llg-check.md`, `bp-check.md`, and `commit-changelog.md` used `globs:` and `alwaysApply:`, which are Cursor `.mdc` keys that Claude Code ignores (verified against the official memory docs). Because a rule with no recognized `paths:` field loads unconditionally, the LL-G and BP rules were loading on every file instead of only their intended code/config paths. Converted all three to the official `paths:` YAML-list frontmatter; `commit-changelog.md` now correctly loads unconditionally with no `paths` field.
- **The Stop and Notification bell hooks printed a literal `\a` instead of ringing.** `.claude/settings.json` used `echo '\a'`, which emits the two characters `\` and `a` in most shells. Switched both to `printf '\a'` so the terminal bell actually fires.
- **The shipped baseline `.claude/settings.json` had an empty `permissions.deny` list** despite the init-repo skill documenting a secrets deny-list as "always configure." Added the documented deny entries (`~/.ssh`, `~/.aws`, `~/.azure`, `~/.kube`, `~/.docker/config.json`, `~/.npmrc`, `~/.git-credentials`, `~/.config/gh`, and shell rc files) so every repo cloned from the template starts with secret-file protection.

### Added
- **`SessionStart` hook** wired in `.claude/settings.json` plus a new `.claude/scripts/session-start-kb-check.sh` that surfaces the RULE 1 (LL-G) / RULE 3 (BP) knowledge-base mandate once per session. Previously the KB check only fired on `EnterPlanMode`, so sessions that never entered plan mode got no nudge.
- **`.claude/settings.local.json.example`** showing common git-ignored personal overrides (`disableAllHooks`, `alwaysThinkingEnabled`, `language`), as the init-repo skill recommends.
- **Changelog gate escape hatches.** `check-changelog-staged.sh` and `pre-commit-changelog-reminder.sh` now exempt merge commits (when `MERGE_HEAD` exists) and honor `SKIP_CHANGELOG=1` for genuinely trivial commits, instead of hard-blocking every commit without a staged `CHANGELOG.md`.

### Changed
- **Documented hook event count corrected from 28 to 30.** Added `UserPromptExpansion` (fires when a slash command expands) and `PostToolBatch` (fires after a parallel tool batch resolves), both confirmed in the official hooks reference. Updated `CLAUDE.md`, the init-repo skill hook table, the update-practices skill list (and refreshed its version reference to v2.1.159), and `instructions.md`.
- **`instructions.md` hook descriptions corrected** to reflect the hooks the template actually ships (the git-commit PreToolUse chain, the EnterPlanMode KB check, the PostToolUse KB-contribute prompt, and the new SessionStart reminder) rather than the previous inaccurate "logs a notification" summary.

## [1.8.1.3] - 2026-06-01

### Changed
- Repointed the LL-G and BP knowledge-base references from the `wellforce-brandon` GitHub org to `BoardPandas` after both repos moved. Updated the RULE 1 / RULE 3 fetch URLs in `CLAUDE.md`, the `llg-check` and `bp-check` path-scoped rules, the `add-lesson`, `add-practice`, `apply-practice`, and `init-repo` skills (repo headers, raw URL bases, `gh api` paths, and WebFetch URLs), the `pre-plan-kb-check` and `post-commit-kb-contribute` hook scripts, and `instructions.md`

## [1.8.1.2] - 2026-05-29

### Added
- `MessageDisplay` hook event (introduced in Claude Code v2.1.152) documented in the init-repo skill hook table, the update-practices skill hook list, `CLAUDE.md`, and `instructions.md`. It fires as assistant message text is displayed, letting hooks transform or hide output (for example, redacting secrets). Hook event count moves from 27 to 28

### Changed
- Refreshed the Claude Code version reference in the update-practices skill from v2.1.144 to v2.1.156 (the latest at time of update, verified against the official changelog)

### Removed
- Dropped the phantom `code-review` skill from the documented skill inventory. No `.claude/skills/code-review/SKILL.md` ever existed; the name would shadow the built-in `/code-review` command that the repo already recommends, and the full-codebase-audit niche is covered by `security-scan`, `performance-review`, and `ux-review`. Removed its references from `CLAUDE.md`, `README.md`, `instructions.md` (file tree and skill section), and the update-practices skill checklist. Code review is still available via the `reviewer` agent and the built-in `/code-review` command

## [1.8.1.1] - 2026-05-29

### Changed
- Swapped locked infrastructure defaults in the plan-repo skill and supporting references: frontend hosting moves from Cloudflare Pages to Northflank containers (SPA static-served or SSR, decided per project from the chosen framework), email locks to Resend only (AWS SES dropped), and the CDN is now Cloudflare's orange-cloud proxy in front of the Northflank frontend, with Northflank's built-in Fastly CDN as a no-WAF fallback
- Added a "CDN Setup Notes (Locked)" section to `.claude/references/infrastructure.md` covering Full (Strict) TLS, the ACME-challenge vs Cloudflare-proxy ordering, SSR cache-rule requirements, and zero-cost edge-to-R2 egress
- Updated `.claude/references/tools.md` so `wrangler` is scoped to Cloudflare R2 and DNS/CDN (not Pages) and `northflank` covers frontend deploys as well as backend

## [1.8.1.0] - 2026-05-23

### Fixed
- The update-practices skill now diffs the actual text content of template skills/agents/rules instead of relying on file existence. A skill that was rewritten upstream (e.g. `add-lesson`) is now detected as `TEMPLATE-REWRITTEN` and its body is replaced wholesale with the canonical version (re-applying only genuinely project-specific bits), rather than the old merge-only strategy that silently kept the stale local copy

## [1.8.0.0] - 2026-05-19

### Added
- `reviewer` and `architect` agents now use `memory: project`, reading `.claude/agent-memory/` on startup so they review and plan against accumulated project patterns and decisions
- `worktree.bgIsolation` and `worktree.baseRef` settings documented in the init-repo skill, CLAUDE.md, and update-practices skill (new in Claude Code v2.1.144)
- Prompt-cache preservation guidance (lock the MCP/tool list and model at session start) in CLAUDE.md and instructions.md

### Changed
- Refreshed the hook event reference in the init-repo and update-practices skills to the full 27 events as of Claude Code v2.1.144 (was 18). Added the missing `StopFailure`, `PostCompact`, `PermissionDenied`, `TaskCreated`, `CwdChanged`, `FileChanged`, `Elicitation`, `ElicitationResult`, and `Setup` events
- Added the `mcp_tool` hook type and the conditional `if:` field to the init-repo skill's hook reference, matching the documentation already in CLAUDE.md and instructions.md
- Corrected the init-repo skill's "Recommended agent enhancements" guidance: `background` and `isolation: worktree` are no longer suggested for read-only analysis agents, and `memory: project` guidance now covers `reviewer` and `architect`

## [1.7.1.0] - 2026-05-19

### Fixed
- `add-lesson`, `add-practice`, and `apply-practice` skills no longer hang and time out. They depended on a GitHub MCP server (`mcp__github__*`) that is not configured; invoking those nonexistent tools caused an unresolved tool search to loop until the turn hit its time limit. All three now use the `gh` CLI instead -- `gh api` for reads and writes in add-lesson/add-practice, `WebFetch` on raw URLs for the read-only apply-practice

## [1.7.0.0] - 2026-05-19

### Added
- Pre-commit hook (`check-commit-herestring.sh`) that blocks `git commit` commands using PowerShell here-string syntax (`@'...'@`). In the Bash tool that is not a here-string, so the `@` characters leak into the commit message as a stray `@` line. The hook points to writing the message to a file and using `git commit -F` instead

## [1.6.0.1] - 2026-04-29

### Added
- Documented `mcp_tool` hook type and the conditional `if:` filter syntax for hooks (CLAUDE.md, instructions.md)
- Documented `xhigh` effort tier and `keep-coding-instructions` skill frontmatter field (CLAUDE.md, instructions.md)
- Agent-memory README guidance on explicit memory curation framing and topic partitioning when files grow
- `Cost / token efficiency` audit section in the update-practices skill (effort tuning, model routing, cache preservation, input-format swaps, subagent delegation)
- ProductCompass "stop hitting Claude Code limits" entry to the source URL registry

## [1.6.0.0] - 2026-04-16

### Changed
- Rewrote `doc-sync` skill into a TOC-driven documentation builder that produces a categorized `Docs/` wiki (core, api, features, operations, etc.), modeled on the supportforge platform docs layout but with stable PAGE_ID and AUTOGEN markers for safe incremental updates
- `doc-sync` now operates in three modes: `init` (full generation), `update` (incremental git-diff regeneration), and `audit` (legacy report-only)

### Added
- `Docs/_toc.yaml` schema as the single source of truth for pages, sections, source-file mappings, and diagram requirements
- Reference files: `page-template.md`, `citation-policy.md`, `mermaid-policy.md`, `toc-schema.md`, `doc-categories.md`, `incremental-update.md`, `readme-template.md`
- Page templates: `overview.md`, `architecture.md`, `api-reference.md`, `feature.md`, `database-schema.md`, `module.md`, `data-flow.md`, `runbook.md`, `getting-started.md`, `configuration.md`, `glossary.md`, `_toc.yaml.template`
- Evidence-based citation rules with line numbers and parenthesized inline format
- Mermaid diagram policy (graph TD only, quoted node labels, no shorthand activation) and a 3-attempt repair budget
- AUTOGEN marker contract for safe regeneration that preserves manual notes
- `Docs/_meta/GENERATION.md` and `Docs/_meta/SUMMARY.md` outputs for generation metadata and coverage reporting

## [1.5.0.0] - 2026-04-14

### Added
- UX Review skill (`/ux-review`) for reviewing UI code against Laws of UX and Gestalt principles
- UX Reviewer agent (`ux-reviewer`) with severity-ranked finding output format
- UX Laws reference doc (`.claude/references/ux-laws.md`) covering all 30 laws from lawsofux.com with code-level indicators

## [1.4.0.0] - 2026-03-25

### Added
- Bootstrap template sync step in update-practices skill (Step 2b) to pull new/updated files from upstream template repo
- Bootstrap Template source URLs for GitHub API tree and raw content access
- Template sync report section in update-practices output summary

## [1.3.0.0] - 2026-03-24

### Added
- Add Practice skill wired to `wellforce-brandon/BP` via GitHub API
- Apply Practice skill wired to `wellforce-brandon/BP` via GitHub API
- Pre-plan hook to check LL-G and BP knowledge bases before creating plans
- Post-commit hook to evaluate if work should be contributed back to LL-G or BP
- Pre-commit changelog reminder hook with condensed update instructions

### Changed
- Commit-changelog rule set to `alwaysApply: true` so version bump instructions are always in context
- Pre-commit changelog enforcement hook status message clarified
