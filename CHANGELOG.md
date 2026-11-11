# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.69.6] - 2026-11-11

### Added
- Full Google Antigravity (`agy` / T3 Code Antigravity harness) workspace support alongside Claude Code via `scripts/sync-antigravity.mjs` (`--with-skills`) and `.claude/scripts/_antigravity-hook.sh`: generates `.agents/skills/`, `.agents/agents/*.md`, `.agents/rules/*.md`, `.agents/hooks.json`, and `.agents/README.md`, maps Antigravity tool payloads onto `.claude/scripts/`, converts exit-2 blocks into `decision: "deny"` JSON, and queues advisory context for `PreInvocation` (`scripts/antigravity-hooks.test.mjs`).


## [0.69.5] - 2026-11-10

### Fixed
- Keep the Rust binary version aligned with package metadata after the knowledge-workflow update, restoring macOS packaging verification.

## [0.69.4] - 2026-11-10

### Changed
- Restore LL-G and BP contributions through authenticated GitHub with atomic commits, privacy review, conflict protection, and commit-specific CI verification; remove Roundtable dependencies from agent guidance.

## [0.69.3] - 2026-11-09

**Known gaps:** two direct dependencies stay where they are on purpose. `gtk` stays at 0.18: the Linux tray's `libappindicator` 0.9.0, its newest release, still requires GTK 0.18, and two `gtk-sys` lines cannot link into one binary. `windows-core` stays at 0.62 to match the `windows` 0.62.2 crate; 0.100 is a separate windows-rs line that `#[implement]` would resolve against wrongly.

### Changed
- **The window and settings UI move to egui and eframe 0.37** (from 0.36). Nothing should look or behave differently. Hark's code moves off the APIs 0.37 deprecated: `Id::new` becomes `Id::unique`, which hashes the same way, so remembered UI state such as lettering, appearance and view choices carries over.
- **Meeting MP3s are encoded with LAME 4.0** (from 3.100), through `mp3lame-encoder` 0.3. The Rust API is byte-for-byte the same as 0.2.5. The vendored LAME is now compiled with the same C toolchain on every platform, so Linux and macOS builds no longer run autotools. It is still LGPL, and `THIRD_PARTY_NOTICES.md` now names 4.0.
- **Dependency refresh.** Updated: the Windows save dialogs (`rfd` 0.18), `quick-xml` 0.42 (used only by the Word-export tests), `tokio` 1.54, `arboard` 3.7, plus 90-odd compatible patch and minor updates across the lockfile (including `hyper`, `serde_json`, `toml`, `uuid` and `aws-lc-rs`).

### Fixed
- **Two yanked crates are gone from the lockfile:** `async-executor` 1.14.0 (now 1.14.1) and `clipboard-win` 5.4.1 (now 5.4.3).
- **CI's documentation drift check passes again.** It had been failing on main since 0.69.2, whose `Cargo.toml` and `rusqlite` changes left Overview, Getting Started and Data Storage behind. Those pages, and the six this release's dependency changes touch, are updated: the bundled SQLite version, the shifted MSRV citation, LAME 4.0, and why saved egui settings survive the `Id::unique` rename.

## [0.69.2] - 2026-11-09

### Security
- **History, stats and meetings now use SQLite 3.54.1, which fixes CVE-2026-119375 (RUSTSEC-2026-0459, GHSA-72w2-rjgg-q729).** Older SQLite could return leftover bytes from the process's memory when it opened a database alongside a crafted `-wal` file with a different page size. Hark only opens databases it created itself, in your own profile directory, so exploiting this needed someone who could already write files there. The bundled SQLite comes from `rusqlite` 0.41.1 (was 0.40.2) and `libsqlite3-sys` 0.39.1 (was 0.38.2). Hark's code needed no changes, and `cargo audit` is clean again.

## [0.69.1] - 2026-11-09

### Changed
- **Word meeting exports are written by Hark itself instead of the docx-rs library.** The file holds the same content and formatting as before: title and heading styles, notes, action items, and each speaker's name in bold in their color, on the same A4 page. It now has five parts instead of fourteen and opens in Word, LibreOffice, Google Docs and Pages. docx-rs had to go because its newest release, 0.4.22, requires a `zip` version with open advisories, and no later release has shipped.
- **The on-device engine moves to sherpa-onnx 2.0.1** (from 1.13.8). The Rust API Hark uses did not change, and the prebuilt native library downloads the same way. Upstream's one change for NeMo models such as Parakeet is meant to improve very quiet audio; on normal speech its output should not differ.

### Security
- **Every `zip` advisory reported against Hark is cleared.** Hark had two copies of `zip`: 8.6.0 in the shipped binary, through docx-rs, and 2.4.2 used only while building, through sherpa-onnx-sys. Both are replaced by zip 9.5.0, which fixes all six advisories. Among them is the critical "zip cloaking" parser differential (GHSA-m665-v7pm-7grj, fixed in 9.0.5). The others are symlink-target extraction (GHSA-q7v9-x93c-xqv3, GHSA-hjqx-j4rw-g7xv), ZIP64 legacy-decoder denial of service (GHSA-9hj8-73fh-9963) and AES HMAC bypass (GHSA-8hxf-qr8p-62x4). Hark never extracted an archive, so none of them was reachable. The seven `zip` and `zstd` ignores in `.cargo/audit.toml` that were accepted on that argument are now removed, and the ten `zstd-safe` 7.3.0 unsoundness warnings that came with zip 2.4.2 are gone too.
- **The glib `VariantStrIter` unsoundness (RUSTSEC-2024-0429, GHSA-wrw7-89jp-8q8g) is triaged and accepted.** glib 0.18.5 ships on Linux only, under the GTK 3 tray stack (tray-icon → libappindicator → gtk 0.18), and no upgrade is possible: the GTK 3 bindings stop at glib 0.18. The affected iterator has no caller in Hark or anywhere in the tray stack. `.cargo/audit.toml` records the reasoning and the trigger to re-evaluate it, which is a tray that no longer uses GTK 3.

## [0.69.0] - 2026-11-03

### Added
- **After you push the default branch, Claude offers to clean up what it merged.** A new `PostToolUse` hook, `post-push-cleanup.sh`, runs after any push that moves `origin/<default>`. It lists the local branches now merged into it, the worktrees on those branches that have no uncommitted changes, and worktree entries whose directory is gone, then has Claude ask you once before removing anything. It never deletes anything itself, and the commands it suggests are `git worktree remove` and `git branch -d`, never `--force` or `-D`, so git still refuses to drop unmerged or modified work. It leaves alone locked worktrees, the worktree the session is working in, and any worktree with uncommitted changes, which it reports. It confirms the push from the `origin/<default>` reflog rather than trusting the command text, so a dry run, a rejected push or a push of another branch stays silent. It also catches `git -C <repo> push`. Synced from claude-code-bootstrap 0.28.0.

### Changed
- **The shared git-command parser can test for any subcommand.** `_git-commit-filter.sh` gains `is_git_subcommand` and `git_target_dir`. The commit hooks use them unchanged through `is_git_commit` and `commit_target_dir`, so a fix to the argv walk reaches every hook that relies on it.

## [0.68.14] - 2026-11-03

### Fixed
- **The macOS bundle check passes again.** It asserts that the packaged app's `--version` matches package.json, and the four commits since 0.68.9 had bumped package.json alone, so the binary still reported 0.68.9. The workspace version in Cargo.toml (and the workspace crates in Cargo.lock) moves to 0.68.14 with package.json.

### Security
- **RUSTSEC-2026-0429 (high, `zip`) is triaged and accepted in `.cargo/audit.toml`, alongside the three earlier `zip` extraction advisories.** It is the same function, `ZipArchive::extract`, which Hark never calls; the only caller in the tree is sherpa-onnx-sys's build script, for the iOS archive Hark does not build. No upgrade can clear it yet: docx-rs 0.4.22, the newest release, requires `zip ^8.6.0`. Its main branch already uses zip 9.0.4, so the entry is retired when the next docx-rs release ships.

## [0.68.13] - 2026-11-03

### Changed
- **CI and release builds run on the newest GitHub runner images.** Ubuntu jobs moved from `ubuntu-latest` (24.04) to `ubuntu-26.04` and macOS jobs from `macos-15` to `macos-26`; Windows was already on the newest image (`windows-latest`, Server 2025). The Linux release build alone stays pinned to `ubuntu-24.04`, so the `.deb`, `.rpm` and tarball still run on Ubuntu 24.04 LTS, Debian 13 and Fedora 40+: building on 26.04 would have raised the glibc floor, and `ubuntu-latest` moves to 26.04 this month, which would have raised it silently.
- **Workflow actions and CI toolchains updated.** `actions/setup-node` 7.0.0 → 7.1.0, `softprops/action-gh-release` 3.0.2 → 3.0.3, `Swatinem/rust-cache` 2.9.2 → 2.9.3, `dtolnay/rust-toolchain` to the October 2026 `stable` commit, and the Arch build container's digest refreshed. Every action stays SHA-pinned. CI already uses Node 26, the current LTS. The other pins were already current: `checkout` 7.0.1, `upload-artifact` 7.0.2, `download-artifact` 8.0.2, `artifact-signing-action` 2.0.0, `cargo-deb` 3.8.0 and `cargo-generate-rpm` 0.21.1. The signtool NuGet packages stay at the versions `artifact-signing-action` 2.0.0 installs itself.

## [0.68.12] - 2026-11-03

### Changed
- **`/dependency-audit` now audits GitHub Actions.** A new step checks every action the workflows use against its latest release (SHA pins are resolved to their real tag, and each action is checked against GitHub's advisory database, for an archived repo, and for a deprecated Node runtime), every `runs-on` label against GitHub's current runner images and their retirement dates, the tool versions workflows install against current releases and the repo's own pins, container images, and whether Dependabot covers `github-actions`. Self-hosted runners are listed for a manual version check. Synced from claude-code-bootstrap.

## [0.68.11] - 2026-11-03

### Changed
- **The changelog follows Keep a Changelog 2.0.0.** 2.0.0 keeps the 1.1.0 format, so only the header link moves and no entry is rewritten. As 2.0.0 recommends, releases are archived only once they are old enough not to need editing: past 100 KB, CHANGELOG.md keeps every release from the last 30 days (at least the 10 newest), and releases inside that window that had been archived are back in it. The commit rule says the same.

## [0.68.10] - 2026-11-03

### Added
- **Older releases now live in `CHANGELOG-archive.md`.** CHANGELOG.md had grown past 100 KB. It now holds the newest releases (about 50 KB) and points to the archive, which holds the rest. The commit rule says when to archive again: past 100 KB, move the oldest releases out until it is under 50 KB, keeping at least the 10 newest.
- **Every release tag has a compare link.** The bottom of each changelog file links each version that has a `v` release tag to its changes since the previous tag. No tags were added: a `v*` tag starts the signed release build, so only versions already released get a link, and the commit rule says to add the link when you cut the next release.

### Changed
- **The changelog follows Keep a Changelog 1.1.0 throughout.** Two release headings held entries from earlier versions that had waited under `[Unreleased]`: 0.50.2 held everything from 0.47.1 to 0.50.1, and 0.44.0 held entries from 0.42.2 on. Each of those entries is now under the version whose commit wrote it (from `git blame`), dated when that version first appeared. The 0.50.2 heading stays, without entries, because that release shipped them; its link shows everything in it. Notes that are not changes ("Known gaps", "Upgrading", "Known platform differences") are bold paragraphs under their version instead of non-standard headings, and the header now says the project follows Semantic Versioning.
- **The commit rule and the changelog reminder list all six section types in Keep a Changelog's order** (Added, Changed, Deprecated, Removed, Fixed, Security), and the rule says to use only those.

### Removed
- **The Claude Code starter template's own release history is no longer in Hark's changelog.** The file still carried the template's entries from before Hark's 0.0.1 (the template's 0.0.1 to 0.7.0 and its older four-part versions), which reused Hark's version numbers. A line at the end of the archive points to them in the template's changelog.

## [0.68.9] - 2026-11-02

### Changed
- **Releases should publish in about half the time.** A release used to take 28–33 minutes. Each platform now runs its checks and its release build side by side, and a separate job signs and publishes once both pass. The Arch package also builds alongside Linux instead of after it. The longest path should now be one Windows build plus signing, about 15 minutes. That figure is an estimate until the first release under this workflow runs.
- **The release checks now start from CI's cache instead of from nothing.** Every release job used to report "No cache found." and compile everything from scratch, because GitHub hides a cache saved by one release tag from the next. Each release then saved about 4.7 GB of cache that no later run could use, which pushed main's own CI caches out. The release check jobs now reuse the cache CI saves on main and never save their own. The builds that produce the shipped, signed files still start clean on purpose.
- **A tag that fails its checks no longer leaves an empty release behind.** The release used to be created before anything was tested, so a failing tag still left an empty release, and the in-app updater would offer it with nothing to download. The release is now created only after the checks pass on every platform. This adds no time, because the checks finish before the builds.
- **Only the jobs that publish can write to the release.** The workflow's default token is now read-only. The publish jobs, which compile nothing, are the only ones given write access and the signing secrets.
- `cargo-deb` now installs from its own pinned `.deb` instead of compiling on every release. `cargo-generate-rpm` publishes no binaries, so it still compiles, but at a pinned version instead of whatever was newest that day.

## [0.68.8] - 2026-11-02

### Fixed
- The developer docs for transcription, voice cleanup, invocations, the spellbook and the glossary now say that a live session's STT time is the wait after release, not the whole hold, and their source citations point at the right lines again. This turns CI's Documentation drift check green on main.

## [0.68.7] - 2026-11-02

### Removed
- **The agent evals, AI code review and scheduled security scan GitHub Actions workflows.** `agent-evals.yml`, `claude-review.yml`, `security-scan.yml` are deleted from `.github/workflows/` and their past runs are cleared from the Actions tab. `ci.yml` and every other workflow are unchanged.

## [0.68.6] - 2026-10-30

### Changed
- The feature-ideas checklist (`tasks/feature-ideas.md`) now matches what has shipped. Speaker colors, moving a line to another speaker, the Parchment theme, Wizard lettering and the book views are marked Complete with the version they arrived in, and each links to its plan and lists what was left out on purpose. A user-picked accent color and the Full / Half / Non-Wizard setting are still open.

## [0.68.5] - 2026-10-30

### Fixed
- **Live transcription timings now measure only the wait after you release the key.** With Gemini Live or OpenAI live, the log's "finalise took N ms" line and the STT time in history counted the whole time the key was held. A 14-second dictation that appeared 1.7 s after release was logged as "finalise took 13772 ms", which read like a slow provider or network. History rows recorded before this update keep the old number.

## [0.68.4] - 2026-10-29

### Changed
- The repo's tooling scripts now target Node 26, the current LTS (up from 24), and npm 12.2.1. Every CI and release workflow sets up Node 26, and `package.json` declares both in `engines`.

### Security
- tokio 1.54.1 (RUSTSEC-2026-0386: `AsyncFd` constructors violated I/O safety). This is the runtime behind the Gemini Live and OpenAI live transcription connections.

## [0.68.3] - 2026-10-29

### Security
- **HTTPS connections no longer trust the distrusted emSign intermediate certificates.** Hark's TLS libraries are updated: rustls-webpki 0.103.16 (RUSTSEC-2026-0382), plus webpki-root-certs and webpki-roots 1.0.11 (RUSTSEC-2026-0381, -0383). This covers every connection to your speech-to-text, cleanup and Gemini/OpenAI live providers.
- ctutils 0.4.7 (RUSTSEC-2026-0385, non-constant-time comparisons) and zstd-sys 2.1.2 (RUSTSEC-2026-0410).
- The `zip` extraction advisories (RUSTSEC-2026-0367/-0368/-0369) and three `zstd` advisories (RUSTSEC-2026-0391/-0398/-0403) are reviewed and recorded in `.cargo/audit.toml`. Hark never extracts zip archives at runtime: it only writes Word documents, and the older `zip`/`zstd` copies run only inside the on-device engine's build script. Each entry names when to look again. With this, the CI dependency audit passes again.

## [0.68.2] - 2026-10-29

### Fixed
- The Linux build compiles again. The 0.63.2 tray fix added meeting-menu fields that the Linux tray does not use yet, and CI's strict lint check rejected them as unused, so a release could not build Linux packages.

## [0.68.1] - 2026-10-29

### Changed
- Getting Started lists the checks a change must pass and where the agent tooling is indexed. The Glossary explains the agent registry rename. With these, `npm run check:docs` passes again.
- The Glossary defines eight more terms: Parchment, Wizard lettering, Book view, Drop cap, Speaker roster, Speaker key, Speaker color and Guessed name. The docs index summarizes 0.65.0–0.67.0.

## [0.68.0] - 2026-10-29

### Added
- **More color on every page, in every theme.** Each page now has its own color: Home green, Insights blue, History gold, Meetings teal, Spellbook violet and Invocations rose (Settings keeps the theme's accent). It tints the page's icon in the sidebar and the top bar, and the icon and a short line under each page title.
- **History** marks each dictation with a colored chip naming the provider that wrote it (Deepgram, OpenAI, Gemini, OpenAI-compatible, On-device), and a rose "Invocation" chip when an invocation fired.
- **Insights** gives every number tile its own colored icon, colors the app and cleanup-voice bars one color per row, and shows provider chips under Performance.
- **Settings** gives each section its own icon color, shows which provider a key belongs to as a chip above the key's status, and names the on-device engine in a chip.
- The Spellbook list shows the alias count as a violet chip, and the Invocations list shows the lightning mark in rose and the scope as a chip.

### Changed
- The status icon on Home now takes the status color: green when ready, the accent while listening or working, and amber when something needs attention. The words next to it are unchanged.
- Colors stay readable in all six themes: every new color is checked against each theme's backgrounds, and colors that mean a category are never used to show a status.

## [0.67.3] - 2026-10-29

### Fixed
- Three stale lines in the project docs are now correct:
  - Data Storage now lists migration 007 (meeting speakers).
  - Data Storage links Gemini window speaker labels to the code that builds them now.
  - The Glossary no longer says echo reduction is disabled. It is an optional setting, off by default.

## [0.67.2] - 2026-10-29

### Changed
- The project docs now describe meeting speakers: speaker colors, renaming Me, moving lines, guessed names, notes that follow renames, the Your name setting, and how speaker names are stored. They also cover the faster Gemini Live text checks from 0.64.1.

## [0.67.1] - 2026-10-29

### Added
- `npm run check:claude` now fails when two tracked paths differ only by letter case, so a template sync cannot bring `agents.md` back.
- The docs overview's "Where to Start" table now links the agent registry.

### Fixed
- **A fresh checkout on Windows or macOS is clean again.** The repo tracked both `AGENTS.md` (the agent guide) and `agents.md` (the agent registry). Windows and macOS treat those names as one file, so every checkout showed one of them as modified. That also stopped the desktop app from syncing a worktree with its base branch. The registry is now `AGENT-REGISTRY.md`, and every link to it is updated.

## [0.67.0] - 2026-10-29

### Added
- **New Parchment theme.** Aged paper with dark-brown ink and lapis-blue accents. Pick it from the theme menu in the top bar or in Settings → Appearance. It is a light theme only, so it stays light even when your computer is set to dark mode. Every color in it is checked to stay readable.
- Hark now ships two free book fonts for Wizard lettering, Alegreya and Cinzel Decorative (SIL Open Font License). Their source, download date and license are recorded in `crates/hark-app/assets/README.md`.
- **Wizard lettering.** A new "Wizard lettering" checkbox in Settings → Appearance sets text in Alegreya, a calligraphic book face, and page titles, dialog titles and the Home headline in Cinzel Decorative. Body text is one size larger than before, because Alegreya reads smaller than Inter. Key names and timings stay in the monospace font, and icons do not change.
- The Parchment theme turns Wizard lettering on until you choose for yourself. Once you tick or untick the box, your choice stays the same in every theme, and Hark remembers it on this device.
- **Book view for the Spellbook.** A new List | Book switch above your vocabulary opens the Spellbook as a book. Each term starts with a large colored first letter, followed by "Also heard as" with its misheard spellings. Turn pages with Previous and Next, or with Page Up, Page Down, Home and End when you are not typing. A wide window shows two facing pages; a narrow one shows one.
- Edit a term right where it sits in the book: Edit opens the term and its misheard spellings, and Done saves. Changes save and apply the same way as in the list.
- After you add a term, including one sent from History, the book opens to the page that holds it and marks it "New".
- List stays the default. Hark remembers your choice on this device.
- **Book view for Invocations.** The same List | Book switch now sits beside "New invocation". In the book, each trigger phrase starts with a large colored capital, followed by its scope, its alternates, and the full text it types with line breaks kept. If an invocation cannot fire, the reason is shown with a warning icon.
- Edit or create an invocation without leaving the book: the editor opens across both pages, and page turning pauses until you save or cancel. Saving works exactly as before (only on Save), and the book then opens to the invocation you saved.

### Changed
- In the book view, the first letter of every Spellbook term and invocation phrase is shown as a capital. Your terms and phrases are not changed.

## [0.66.0] - 2026-10-29

### Added
- Every theme now has a set of six colors (blue, teal, green, gold, rose and violet) for telling things apart, such as pages, providers and meeting speakers. Each one is checked to stay readable in its theme and to look different from the other five.
- **Every meeting speaker has a color and a name you can change, "Me" included.** The Speakers card on a meeting now lists everyone who spoke, you too, each with their color. Click a name to change it. If you give two speakers the same name, the card says "Shown as the same person as Speaker 2" first, and after you save they share one color and one entry everywhere.
- **Fix who said a line.** Each transcript line now starts with a thin bar in the speaker's color and a speaker button. Click the button to move the line to yourself, to anyone else in the meeting, or to a new speaker (for example a second person on your microphone). A moved line shows an arrows icon that says who it was transcribed as, and "As transcribed" puts it back. Moves are saved right away and survive a restart.
- **Hark fills in speaker names it hears in the call.** After each meeting, when someone says "Hi, this is Sam from Acme" or is thanked by name twice, that speaker is named Sam. With notes on and a text provider set up, your provider looks for the names; otherwise Hark looks for introductions on this device. A name is only used when the line it came from really says it. Guessed names carry a sparkle marker that says when the name was heard; click the name to change it, or save it unchanged to keep it. A guess never replaces a name you set.
- **Your name** (Settings > Meetings): names "Me" in every new meeting. Renaming Me in one meeting still wins. Existing meetings are unchanged.
- **Guess speaker names from names said in the call** (Settings > Meetings), on by default.

### Changed
- Each theme now carries its own error, success and warning colors. Nothing looks different yet; this prepares for the new Parchment theme, whose status colors are tuned to its paper.
- Speaker names you gave in past meetings move to a new format the first time this version opens your meetings, so "Me" and new speakers can be named later. Nothing to do on your side, and every name you set is kept. An older Hark opened afterwards still shows those names as they were.
- Meeting speakers now have a fixed color order: "Me" is always blue, and nobody else ever is, so you can spot your own lines in any meeting. The next five people get gold, violet, teal, rose and green.
- **Exports show who really said each line.** Markdown, text, subtitles, Word and audio excerpts now use the names you set, including your own, and follow any line you moved to another speaker.
- In Word documents, each speaker's name is bold and in their color. The colors are dark enough to read on white paper.
- **Meeting notes follow your speaker names.** Rename a speaker and the notes change at once: the summary, key points, decisions, action items and their owners all show the new name. Nothing is sent anywhere, so it works without a text provider key. Notes made before this version follow renames too.
- Exported and copied notes use the same names as the meeting page. A notes-generated title keeps the names it had when it was set, since a title is yours to edit.
- The notes setting now reads "Write notes and guess speaker names after the call (sends the transcript to your text provider)", because name guessing uses the same provider and the same transcript as the notes.
- **Moving a line rewrites the notes to match.** When you move transcript lines to another speaker in a meeting that has notes, Hark asks your text provider for new notes in the background ("Updating notes…"). Several quick moves cause at most two rewrites, and you see the result of the last one. Action items you ticked stay ticked, even if their wording changes a little; a ticked item the new notes leave out is kept. The meeting title never changes.
- If the notes cannot be rewritten (no text provider key, notes turned off, or the provider did not answer), the old notes stay and a line under Summary says why: "Notes still credit the earlier speaker for 3 moved lines." Choose **Update notes** to try again.
- **Re-running the final pass now guesses speaker names again and rewrites the notes.** The notes keep the names they had until the rewrite lands, so they never credit the wrong person after speakers are renumbered. The confirmation explains this before anything is sent.
- Speaker names inside meeting notes are now shown in each speaker's color, so you can see who owns an action item at a glance. The words are the same as before.
- Long meetings stay smooth: a three-hour transcript (about 2,500 lines) now draws only the lines on screen, instead of all of them on every frame.
- The speaker button on each transcript line works from the keyboard: Tab reaches it, Enter opens the menu, Tab moves through the choices, and Esc closes it.

## [0.65.0] - 2026-10-29

### Added
- **Choose cloud or on-device transcription in one place.** Settings → Dictation → Speech to text now holds the whole choice. Your cloud provider comes first. Below it, **Use on-device transcription** turns on the on-device model, as **Primary** (offline, no key needed) or **Backup** (used when the cloud fails or is slow). A line under the choice says where your audio goes. Turning it on picks Backup if you have a cloud key stored and Primary if you do not. In Primary, the cloud controls are hidden.
- **A suggestion when you have no key.** With no cloud key stored, the card offers "No paid AI key? Use on-device." and a **Use on-device instead** button. The button switches to on-device but does not start the download; that stays behind the model's own **Download** button.
- Each provider now has a one-line description under its name (also shown when you hover it), and a **Get a key** link next to "No key for … yet" that opens the provider's key page. The link goes away once a key is stored.
- If you turn on-device off while the model is still downloading, the download's progress and a **Cancel** button stay visible.

### Changed
- Hark's notes about the on-device model no longer call Parakeet TDT 0.6B v3 the latest version. It is the multilingual build Hark ships. Newer English-only builds exist, and Hark keeps v3 for now.
- The separate **On-device** page in Settings is gone; its controls moved into Speech to text.
- "Model & endpoint" is now **Model and endpoint (optional)**, or **(required)** for an OpenAI-compatible server, with a note that leaving it blank uses the recommended model.
- The privacy note under Dictation now matches your mode: cloud, cloud with on-device backup, or on-device only.
- In on-device Primary, a cleanup voice that reuses your speech-to-text key now shows that key's status and a field to store it under **Cleanup provider**, since the Speech to text key box is hidden.
- First-run setup now suggests "No paid AI key? Choose on-device. You can add a cloud provider later in Settings." under the two choices, and the on-device choice shows the download size.
- The README and docs now point to Settings → Dictation → Speech to text for on-device transcription, and describe the first-run tip and the "Use on-device instead" suggestion.

### Fixed
- Saving on-device Primary before the model has downloaded no longer says "Saved. Dictation is ready." It now says dictation works once the download finishes.
- In first-run setup, switching to on-device Primary from the cloud screen (with the new toggle or the "Use on-device instead" button) now shows the on-device setup and waits for the model download, instead of still asking for a cloud key and a passed test. Choosing Backup still needs a passed test.
- If your settings say on-device Primary but your build of Hark has no on-device engine, the Speech to text card now still shows the cloud controls. Before, it hid them along with the on-device controls, so nothing on the card could change the setting.

## [0.64.1] - 2026-10-29

### Fixed
- **Gemini Live no longer wastes about 15 ms on Windows every time it checks for new text.** Hark checks after every 100 ms of audio it sends. Each check started a timer that Windows rounds up to about 15.6 ms, even when nothing had arrived. A 25-second meeting chunk lost about 4 seconds this way, and Test connection and the backup replay of a 10-second clip lost up to 1.5 seconds. Checks now return right away. The OpenAI live adapter in 0.64.0 already worked this way.

## [0.64.0] - 2026-10-29

### Added
- **Live transcribe for OpenAI.** The OpenAI provider has a new "Live transcribe" checkbox. While you hold the key, it streams your speech to OpenAI's `gpt-live-transcribe`, so only the last moment of audio is left to process when you let go. In testing on a 10-second clip, it returned text about 200 ms sooner than `gpt-transcribe`, at the same accuracy. It uses the OpenAI key you already saved, so there is nothing new to enter. It is off by default and costs about four times as much per minute ($0.017 against $0.0045), and Settings says so next to the checkbox. If the live connection fails or comes back empty, Hark sends the dictation through `gpt-transcribe` instead, so it is never lost. Meeting notes always use `gpt-transcribe` and are never billed at the live rate.
- `openai_live_probe`, a diagnostic example that checks OpenAI's realtime transcription socket against the real API and compares it with `gpt-transcribe`: `cargo run -p hark-stt --features live --example openai_live_probe`.

### Changed
- **Live transcribe captures audio at 24 kHz**, converted straight from your microphone's own rate, which keeps more of the high end of speech. OpenAI's realtime API accepts no other rate. Everything else still uses 16 kHz.
- The `gemini-live` build feature is now called `live-stt`, because it covers both live providers. The old name still works.

### Removed
- **Groq is no longer offered for speech-to-text or cleanup.** If you used Groq, your settings are converted on first launch to an OpenAI-compatible provider with the same Groq address and model, so dictation and cleanup keep working as before. Your Groq key is copied across if that slot was empty. Your old settings file is kept beside the new one as `config.toml.v6.bak`.

## [0.63.2] - 2026-10-29

### Added
- A living feature-ideas checklist (`tasks/feature-ideas.md`) that tracks ideas with a status (Incomplete, In progress, Complete, Sidelined, Denied). It starts with seven ideas: speaker colors and fixing a line's speaker in meeting transcripts, an accent color choice per theme, a Parchment theme with a wizard-pen font toggle, a Book view for the Spellbook and Invocations, and a Full / Half / Non-Wizard setting.

### Changed
- The Meetings page now says what Hark does when a call takes your mic: it asks first, starts notes on its own, or never starts them. A "Meeting settings" link goes straight to that setting.

### Fixed
- **The tray's meeting menu no longer starts notes you did not ask for.** If a call ended and the notes stopped while the tray menu was open, the menu still said "Stop meeting notes", and clicking it started a new meeting. The menu now has separate "Start meeting notes" and "Stop meeting notes" entries, and only the one that applies can be clicked. A late "Stop" click does nothing.

## [0.63.1] - 2026-10-29

### Added
- `meeting_chunk_probe`, a diagnostic example that sends 10, 20 and 30 second test clips through the meeting live-transcript path and reports which ones stall: `cargo run -p hark-pipeline --features live --example meeting_chunk_probe`.

### Fixed
- **Dictation no longer takes 8-11 seconds when Gemini Live stalls.** When the Live session failed to finish, Hark sent the whole clip to Gemini again, and at those times Gemini was slow too (4.5-7 s on top of the 3 s already waited). With the on-device model set as a backup (`[local_stt] mode = "fallback"`), Hark now transcribes the clip on-device right away instead. Gemini is asked again only if the on-device model fails. The first time this happens after Hark starts, loading the model adds a few seconds.
- **Meeting live transcripts no longer lose lines to stalled Gemini Live turns.** Gemini Live sometimes takes a meeting piece, sends partial text, and then never finishes. Hark used to wait 15 s, send the same audio again (it stalled again 85% of the time), keep the piece to resend later, and count each stall as a slow connection. One user lost 1,462 lines this way in three weeks, and the live transcript fell minutes behind. A stall is now reported as its own error: it is not resent, not queued, and does not pause live uploads with a "slow connection" notice. With the on-device backup set, the stalled piece is transcribed on-device so the line still appears. Real network failures still pause and resend as before.

## [0.63.0] - 2026-10-23

### Changed
- **The "Take meeting notes?" prompt is easier to notice.** It now has a colored header that says it comes from Hark, a larger "Teams is using your mic." headline, and a colored outline. The header also counts down to when the prompt closes on its own ("Closes in 24 s"), with a bar that shrinks as time runs out. On Windows 11 it has rounded corners like the system's own notifications.

### Fixed
- The meeting prompt no longer cuts off its buttons and bottom edge. Its window was a fixed size that was too short for its contents on every display. It now sizes itself to what it shows, so a long app name wraps onto a second line and the prompt grows to fit.

## [0.62.7] - 2026-10-19

### Changed
- `/add-lesson` and `/add-practice` now publish through Roundtable's `kb_propose` instead of committing to LL-G or BP `main` with `gh`. LL-G and BP are public, and the direct path skipped every check: Roundtable refuses credentials, email addresses and IPs, refuses a duplicate entry, caps each agent at 10 proposals a day, and indexes an entry only once that repository's CI passes. Both skills also have a step to strip client names, internal hostnames and other details no scan can recognise.
- `gh` is used only to start a new LL-G shelf or BP concern, which `kb_propose` cannot create. That path writes the entry, its new index and the master index line as one commit with `.claude/scripts/kb-commit.mjs`, added in this release.
- Without Roundtable the skills stop and say how to set it up. They no longer fall back to `gh`.
- Eval case 17 (`add-lesson-trigger`) no longer counts a direct GitHub-API write to LL-G as equivalent to `/add-lesson`.

## [0.62.6] - 2026-10-15

### Fixed
- Release builds: the macOS build now also retries the check that the notarization ticket was attached. When the 0.62.4 Mac build was re-run, attaching the ticket worked, but the follow-up check lost its connection to Apple in the same way, and 0.62.5 only retried the attach step. Both steps now retry up to four times. 0.62.4 shipped without a Mac download because of this; this release is the first with one since 0.62.3.

## [0.62.5] - 2026-10-15

### Fixed
- Release builds: the macOS release no longer fails when Apple's servers briefly drop the connection while Hark's build downloads the notarization ticket. Apple had already approved the 0.62.4 disk image, but the one download attempt was cut off, so the Mac build failed with nothing wrong in it. The build now retries that download up to four times, about 90 seconds in total, without resubmitting anything to Apple.

## [0.62.4] - 2026-10-15

### Changed
- The Voice picker now warns when Gemini's Smart transcript is on. Smart replaces your cleanup voice entirely, so choosing the Verbatim voice did not stop Smart from dropping openers like "So basically," or carrying out instructions like "Drop all the other bullet points." instead of typing them. The warning points to Speech to text → Transcript → Verbatim, which does. The Smart option's tooltip now spells out the same behaviour.
- History no longer labels Smart text as a "Raw transcript". Smart returns only its tidied text, so expanded Smart rows are headed "Gemini Smart text · not verbatim".

### Fixed
- Gemini's Smart transcript no longer types "I'm just a language model and can't help with that." in place of what you said. Smart runs your speech through a language model, and a dictation that sounds like a request (such as "Add the following MCP server") could come back as Gemini's canned refusal. Hark now recognises that reply and re-transcribes the same recording in Verbatim, so your actual words are typed.

## [0.62.3] - 2026-10-15

### Fixed
- Windows meeting notes record the other people on a Teams call when Teams plays to your default output. 0.62.0 switched to recording the output device only when the call played somewhere other than the Windows default, but Teams turned out to be inaudible to Hark's per-app capture on the default output too, so the other side recorded silence. Hark now records the output a call app is playing to whenever it sees one playing; anything else playing to that same output is recorded with it.

## [0.62.2] - 2026-10-14

### Fixed
- Linux and macOS builds of 0.62.1 failed the strict lint gate: part of the Windows-only meeting audio gap filler was unused on those platforms. It is now covered by a test on every platform, so the build passes without silencing the lint.

### Security
- Updated rustls to 0.23.46 for four TLS advisories published this week (RUSTSEC-2026-0346 to 0349): TLS session secrets exposed through `Debug` output, buffered data sent to the outer server after an ECH rejection, unbounded Brotli certificate decompression before authentication, and a ServerHello downgrade to classical key exchange. Hark's connections to Deepgram, OpenAI, Gemini and GitHub all use rustls.

## [0.62.1] - 2026-10-14

### Changed
- Development tooling: the commit gate now checks that CHANGELOG.md changed, that package.json's version was bumped, and that CHANGELOG.md has a section for that exact version. Before, it only checked that CHANGELOG.md was staged, and a command that merely said `git add CHANGELOG.md` passed even when nothing had changed.

### Fixed
- Development tooling: the `SKIP_CHANGELOG=1` opt-out offered by the commit gate now works when written in front of the commit command (`SKIP_CHANGELOG=1 git commit ...`); before, the gate could never see it. `--amend`, the first commit in a repository, and commits wrapped in `{ ...; }`, `if` or `time` are handled correctly.
- Development tooling: the changelog reminder and the post-commit knowledge-base prompt now reach Claude; they were printed where only the debug log saw them. The post-commit summary lists only what was committed, not uncommitted edits.
- Development tooling: the here-string warning no longer suggests a commit-message file under `.git/`, which cannot be created in a worktree.

## [0.62.0] - 2026-10-14

### Added
- Meeting notes warn during the call when Hark hears nothing: after 45 seconds of silence from your microphone (naming the microphone your call app uses when it is a different one), and after 2 minutes of silence from the call while the call app is playing audio. Before, a silent recording was only discovered after the call.
- Meetings keep a final pass that could not reach Deepgram and retry it on their own: after 1, 5, 15 and 60 minutes, then daily, six tries in all, and again when Hark next starts. The meeting page shows "Waiting for a connection, retrying at 4:35 PM" until the transcript arrives, and notes are made then if the call ended without any.
- A meeting with no transcript offers **Transcribe recording** instead of Re-run final pass.

### Changed
- Settings > Meetings > Microphone: the default entry is now "Automatic: the mic your call app is using", replacing the hint that the communications default is the microphone Teams and Zoom use, which was not true on every computer.
- The final pass uploads the compressed MP3 archive instead of the WAV files, about 8x less data (a 40-minute call drops from 155 MB to 19 MB). With audio compression off it still uploads the WAVs.
- The final pass waits up to 15 seconds to connect instead of 3, so a slow cellular handshake no longer fails it.
- On a weak connection the live transcript pauses for two minutes after three failed lines, then sends the held audio (up to five minutes) once a request gets through. A notice says the full transcript is still made after the call.
- The meeting page shows why a meeting has no transcript (waiting to retry, silent recording, no Deepgram key, key refused) and keeps it after a restart, instead of a message that vanished.

### Fixed
- Windows meeting notes record your voice from the microphone your call app is actually using. Hark used the Windows communications-default microphone, which can be a different device from the one Teams is set to, even a muted one: six Teams meetings in October recorded only silence that way. An explicitly chosen meeting microphone still wins.
- Windows meeting notes record the other people on a call that plays to an output other than the Windows default, such as a headset's separate "Chat" device. Hark's per-app capture heard nothing there; Hark now records that output device itself for the call, keeping both sides in time through pauses when nothing plays.
- A silent recording is no longer uploaded and reported as "Deepgram returned no transcript". Hark now says "This recording is silent; Hark captured no audio from either side."
- Re-running the final pass while Hark was already transcribing that meeting no longer left the page waiting forever.

## [0.61.7] - 2026-10-14

### Fixed
- Development tooling: commits made by Claude Code sessions in a git worktree are no longer refused with "CHANGELOG.md is not staged" when the changelog is staged. The commit hooks run in the main checkout, so they were reading its staged files instead of the worktree's; they now check the repository the commit actually lands in, using the session directory the hook is given plus any `cd DIR &&` or `git -C DIR` in the command. A worktree commit without a staged changelog is still refused even when the main checkout has one staged, and the refusal names the repository it checked.
- Development tooling: the commit hooks keep backslashes inside double-quoted paths (`cd "C:\repos\Hark"`) and no longer forget a `cd` that came before a subshell.

## [0.61.6] - 2026-10-05

### Changed
- Windows meeting detection checks every two seconds instead of waiting for registry change notifications, which no longer arrive. The five-second start delay and the auto-stop delay are unchanged.

### Fixed
- Windows: Hark again offers to take notes when a Teams call (or Zoom, Meet, Discord and the other meeting apps) starts, and a meeting stops on its own once the call ends. A Windows update on 2026-10-03 stopped updating the record of which apps use the microphone that Hark relied on, so calls went unnoticed and a meeting started by hand recorded until stopped by hand. Hark now asks the Windows audio system directly which apps are recording from any microphone, including virtual ones such as Krisp's.
- Meeting detection keeps working after the Windows audio service restarts (a driver update, a Bluetooth headset connecting); before, one failed check turned detection off until Hark was restarted.

## [0.61.5] - 2026-10-01

### Changed
- Linux: Hark no longer uses `libxdo`. The Arch package no longer depends on `xdotool`, and building from source no longer needs `libxdo-dev`.
- Updated the tray icon (tray-icon 0.26), MP3 decoding (symphonia 0.6), X11 (x11rb 0.14) and HTTP (reqwest 0.13.5) libraries, plus compatible updates across the dependency tree. Saved meeting audio still decodes to its exact original length.
- CI workflows run on Node 24.

### Fixed
- Transcription, cleanup, model-download and update requests keep trusting only Hark's built-in certificate list, never certificates installed on the computer, now that the HTTP library has dropped the option Hark used for this. That removal had silently held Hark on an older release of the library; it is now current, with the same certificate behaviour.

## [0.61.4] - 2026-10-01

### Fixed
- The Windows dictation indicator restores its always-on-top position when it appears and while recording or showing feedback, without taking focus from the app receiving dictation.

## [0.61.3] - 2026-09-30

### Fixed
- Linux release builds install the PipeWire runtime module needed by the existing timeout/disconnect test, matching regular CI. The missing dependency stopped 0.61.2 before Linux packaging and skipped the dependent Arch build; test coverage remains unchanged.

## [0.61.2] - 2026-09-30

### Fixed
- Gemini Live rejects a socket close before turn completion and replays the preserved recording instead of accepting a partial transcript. Completion markers sharing a frame with final text retain both pieces of information.
- Cleanup rejects nonempty responses stopped by a token limit, content filter, or tool/function call, preserving the original transcript through the existing fallback.
- Spellbook corrections reuse punctuation already present in terms such as C++, C#, and .NET, so repeated correction no longer grows punctuation.
- Full meeting WAV and MP3 exports use exclusively created temporary files, preserve unrelated companion files, and remove their own partial files on failure.
- Linux PipeWire discovery waits exit on connection failure or timeout. X11 meeting detection finds managed client windows behind window-manager frames, and buffered shortcut presses keep their observed event order while retaining missed-release recovery.

## [0.61.1] - 2026-09-30

### Fixed
- Stopping or restarting dictation now cancels pending work before it can retry, clean up, or paste a late result. Holds begun while dictation is busy are discarded through their release, and accepted edges retain their observed audio positions. A failed live session keeps its spent retry budget even if it disconnects before release.
- Clipboard reads now retry contention without treating an unreadable stash as empty. Verification and paste failures restore saved text, including the delay needed when a paste partially succeeded.
- Linux meeting playback streams are mixed on one timestamped timeline instead of concatenated; a missing app stream now starts the default-sink fallback. Streaming resamplers and meeting capture flush their final buffered audio at stop.
- Meeting deletion keeps the history entry when recording removal fails and reports a retryable error. Transcription, cleanup, and summary diagnostics omit provider response content, URL parameters, and arbitrary remote error values.

## [0.61.0] - 2026-09-30

### Added
- A native Home page with the configured dictation shortcut, today's activity, and recent local history, plus a responsive labeled sidebar for Home, Insights, History, Meetings, Spellbook, Invocations, and Settings.
- Expanded local Insights with 7/30/90-day views, daily output charts, activity streaks, measured pace/time-saved estimates, median/p95 completion latency, provider usage, Spellbook corrections, and invocation statistics. Optional app-name tracking and retained-history word/phrase analysis each default off in Privacy settings.
- Light, Dark, Solarized Light, and Solarized Dark appearances alongside System, with persistent selection, accessible text pairs, and locally embedded Lora headings.

### Changed
- Numeric dictation details now retain up to 366 days independently of transcript capture and history deletion; Reset stats clears those details and lifetime counters. Migration backfills only recoverable history fields, leaving unknown clip durations and correction counts unmeasured. Insights show coverage instead of inventing older measurements.

## [0.60.7] - 2026-09-30

### Fixed
- Windows installs and in-app updates no longer get blocked by Smart App Control ("Part of this app has been blocked ... we can't confirm who published Hark-…-setup.tmp"). The installer was signed, but the setup engine it unpacks and runs was not. The engine and the uninstaller are now signed with the same certificate, and each release is checked by running the installer and verifying both.

## [0.60.6] - 2026-09-30

### Changed
- The add-lesson workflow can write its temporary files in `.git/` again. Claude Code's blanket block on editing `.git/` now covers only the parts that control Git's behaviour or data: hooks, config, info, objects, refs, HEAD, the index, packed refs, submodules, worktrees and LFS.

## [0.60.5] - 2026-09-30

### Fixed
- The Arch Linux package builds again. The release asked pacman for `libclang`, which Arch does not have (it ships inside `clang`), so 0.60.4 published no `.pkg.tar.zst`. The PKGBUILD now names `libpipewire`, the real Arch package for PipeWire's build files, so building from the AUR recipe works too.

## [0.60.4] - 2026-09-30

### Changed
- Mac support now targets Apple Silicon (ARM64) only. Removed Intel Mac CI and release builds; Mac packaging and automatic updates accept only ARM64 assets. macOS 14.2 remains the minimum version.

## [0.60.3] - 2026-09-30

### Fixed
- Mac builds pass strict lint checks after the Linux meeting integration: Windows-only probe imports and Windows/Linux watcher cleanup no longer compile on macOS. Mac meeting detection retains its Core Audio probe and polling fallback.
- Restored Mac meeting capabilities in the platform overview and updated the documentation map to track the renamed audio and detection backends.

## [0.60.2] - 2026-09-30

### Fixed
- The 0.60.1 tree still failed macOS clippy: the Windows probe's
  `snapshot`/`processes` became dead code there once the facade routed macOS
  to its own probe, and denied-warnings builds treat that as an error. They
  are now compiled only for the Windows dispatch; macOS keeps compiling that
  module for its polling-fallback watcher. Mapped documentation pages are
  updated in the same commit, and `package.json` keeps its original
  no-trailing-newline form so version-only bumps stay exempt from the
  documentation drift guard.

## [0.60.1] - 2026-09-30

### Fixed
- The 0.60.0 tree did not compile on Windows or macOS: the Core Audio
  backend still imported the pre-split `loopback_win` module, and the
  Windows probe's inner module name collided with its new `probe::win`
  parent. Both are repaired and cross-checked for the Windows target; the
  broken, asset-less v0.60.0 release and tag were removed in favor of this
  one. No behavior changed on any platform.

## [0.60.0] - 2026-09-30

**Known platform differences (documented in `packaging/LINUX.md`):**
- Meeting auto-detection of browser calls requires X11 (Wayland exposes no
  window titles); native clients detect everywhere. An app that bypasses
  PipeWire has no per-app stream to target, so a detected meeting falls back
  to capturing the default output, as on Windows when the app's process is
  gone.

### Added
- **Meeting mode on Linux** (feature parity with the Windows build): recording
  with the microphone plus system audio, auto-detection with the ask/auto
  prompt, the live transcript, the final pass, notes, the MP3 archive, and the
  Meetings page, tray entry, Settings section, and toggle chord. System audio
  comes from PipeWire — the default sink's monitor for a manual start, the
  meeting app's own output streams for a detected one — and detection watches
  PipeWire's microphone-capture nodes with `/proc` for process trees; browser
  window titles are read on X11 sessions. `libpipewire-0.3` is a new Linux
  runtime dependency (declared by every package); building it needs
  `libpipewire-0.3-dev` and clang for bindgen.
- Linux Share-menu parity: native save dialogs through `zenity`/`kdialog`,
  Word document export, and "show/open in folder" through the desktop's file
  manager (FileManager1 D-Bus with an `xdg-open` fallback). The OS share sheet
  remains Windows-only (no Linux equivalent exists).

### Changed
- The `loopback_win` and `probe_win` modules became `loopback/` and `probe/`
  with per-platform backends (`win.rs`, `linux.rs`, `mac.rs`); their public
  contracts are unchanged. `meetings_supported()` now consults the loopback
  module on every platform.

## [0.59.1] - 2026-09-29

### Fixed
- Mac app packaging now passes architecture validation on the Xcode 16 CI runners for Apple Silicon and Intel.

## [0.59.0] - 2026-09-29

### Added
- macOS native push-to-talk shortcuts, shortcut recording and meeting toggles, with permission setup and recovery controls.
- Meeting capture and microphone-use detection on macOS 14.2+, using the shared transcript, speaker labels, notes, audio processing and retention features.
- Mac save sheets, Word and audio exports, Finder reveal, system sharing, and monitor-aware recording/prompt windows.
- Native Mac launch at login, verified app-bundle updates, architecture-specific app/DMG packaging and Mac CI coverage. Developer ID signing and notarization require configured Apple credentials.

## [0.58.0] - 2026-09-29

### Added
- **Elevate phone calls are detected for meeting notes.** A call in Serverdata's Elevate web phone in Chrome now brings up "Take meeting notes?" like a Teams or Meet call; the prompt says "Chrome is using your mic." Keep Elevate in its own Chrome window or install it as an app: Windows only shows Hark the front tab of each window, so a call in a background tab is missed. The Elevate desktop app is on the built-in list too, as "Elevate", but its program name is inferred and has not been seen on a live call yet.

## [0.57.5] - 2026-09-29

### Fixed
- Two Gemini meeting-transcript tests no longer fail at random on Windows. Their local mock server could read before a request arrived. This failed the Windows CI run for 0.57.4; Hark's behavior is unchanged.

## [0.57.4] - 2026-09-29

### Fixed
- A meeting-folder naming test no longer fails at random when the clock ticks to the next second mid-test. It failed the macOS CI run for 0.57.3; Hark's behavior is unchanged. The Meetings documentation now says how meeting folders are named.

## [0.57.3] - 2026-09-29

### Changed
- The Meetings documentation now explains where the "Take meeting notes?" prompt appears: always on your primary monitor, kept on top while it waits.

## [0.57.2] - 2026-09-29

### Fixed
- The "Take meeting notes?" prompt now always appears on your primary monitor, at the bottom-right above the taskbar. It used to follow the focused window's monitor. On a tall portrait screen, that put it far below a Teams window that filled the top half, and it timed out unseen. While it waits, it also keeps itself above other always-on-top windows, such as Teams' floating call window, without taking focus from your meeting.

## [0.57.1] - 2026-09-29

### Fixed
- Dictation that sounds like a request no longer gets answered by the cleanup voice. Saying "Proceed however you recommend to make this as best as possible." with the Clean voice typed "Please provide the transcript you would like me to rewrite." instead. The cleanup model is now told the text is dictation to edit, never a message to it. A response that still reads as a reply is discarded, and your words are typed uncleaned. Custom voices get the same instruction but not the discard check.

## [0.57.0] - 2026-09-28

### Added
- **Optional meeting speaker echo reduction:** enable **Reduce speaker echo** in Settings > Meetings for the next call. It uses captured playback to reduce echo in your microphone recording and live transcript. It starts off so existing headphone recordings keep their current sound.

### Changed
- Microphone audio falls back to the original when the echo reference is unavailable or processing fails. Recording endings and the other side's audio retain their full sample counts. Config schema 6 preserves existing choices and backs up older settings during migration.
- Include the Rust AEC3/WebRTC notices in Windows and Linux packages. Synthetic echo and headphone comparisons informed this choice; real speakerphone quality remains unverified, with further testing explicitly deferred by the product owner.

Older releases are in [CHANGELOG-archive.md](CHANGELOG-archive.md).

[Unreleased]: https://github.com/BoardPandas/Hark/compare/v0.69.3...HEAD
[0.69.3]: https://github.com/BoardPandas/Hark/compare/v0.68.5...v0.69.3
[0.68.5]: https://github.com/BoardPandas/Hark/compare/v0.68.4...v0.68.5
[0.68.4]: https://github.com/BoardPandas/Hark/compare/v0.68.2...v0.68.4
[0.68.2]: https://github.com/BoardPandas/Hark/compare/v0.63.0...v0.68.2
[0.63.0]: https://github.com/BoardPandas/Hark/compare/v0.62.6...v0.63.0
[0.62.6]: https://github.com/BoardPandas/Hark/compare/v0.62.4...v0.62.6
[0.62.4]: https://github.com/BoardPandas/Hark/compare/v0.62.3...v0.62.4
[0.62.3]: https://github.com/BoardPandas/Hark/compare/v0.62.2...v0.62.3
[0.62.2]: https://github.com/BoardPandas/Hark/compare/v0.61.6...v0.62.2
[0.61.6]: https://github.com/BoardPandas/Hark/compare/v0.61.5...v0.61.6
[0.61.5]: https://github.com/BoardPandas/Hark/compare/v0.61.4...v0.61.5
[0.61.4]: https://github.com/BoardPandas/Hark/compare/v0.61.3...v0.61.4
[0.61.3]: https://github.com/BoardPandas/Hark/compare/v0.61.2...v0.61.3
[0.61.2]: https://github.com/BoardPandas/Hark/compare/v0.61.1...v0.61.2
[0.61.1]: https://github.com/BoardPandas/Hark/compare/v0.61.0...v0.61.1
[0.61.0]: https://github.com/BoardPandas/Hark/compare/v0.60.7...v0.61.0
[0.60.7]: https://github.com/BoardPandas/Hark/compare/v0.60.5...v0.60.7
[0.60.5]: https://github.com/BoardPandas/Hark/compare/v0.60.4...v0.60.5
[0.60.4]: https://github.com/BoardPandas/Hark/compare/v0.60.3...v0.60.4
[0.60.3]: https://github.com/BoardPandas/Hark/compare/v0.59.1...v0.60.3
[0.59.1]: https://github.com/BoardPandas/Hark/compare/v0.59.0...v0.59.1
[0.59.0]: https://github.com/BoardPandas/Hark/compare/v0.58.0...v0.59.0
[0.58.0]: https://github.com/BoardPandas/Hark/compare/v0.57.2...v0.58.0
[0.57.2]: https://github.com/BoardPandas/Hark/compare/v0.57.1...v0.57.2
[0.57.1]: https://github.com/BoardPandas/Hark/compare/v0.57.0...v0.57.1
[0.57.0]: https://github.com/BoardPandas/Hark/compare/v0.56.1...v0.57.0

