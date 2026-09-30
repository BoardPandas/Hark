# Specification: Linux meetings parity

Status: Approved scope
Authority: 2026-09-29 request "Build the linux versions to full feature parity
with the Windows version"; implementation and validation delegated in-session.
Design baseline: `tasks/2026-09-26-plan-meeting-transcription.md` (the Windows
seams are the contract); `Docs/features/MEETINGS.md` §"Windows only, for now".

## Scope

Bring meeting mode to Linux through PipeWire, reusing the existing platform
seams unchanged, so the Linux build records, detects, transcribes, refines,
summarizes, archives, and shares meetings exactly as the Windows build does.

1. **Loopback capture** (`hark-audio`, the `start_process_loopback` seam):
   a dedicated-thread `pw_stream` capture, converted to 16 kHz mono f32 in the
   ring exactly like the WASAPI path (PipeWire's converter stands in for
   AUTOCONVERTPCM). `ExcludeTree` captures the monitor of the default audio
   sink (Hark renders no audio, so this equals "everything except Hark").
   `IncludeTree(pid)` captures the output stream nodes whose owning process is
   in `pid`'s tree (resolved from `/proc`), falling back to the sink monitor
   with a logged notice when no such node exists. Stream errors latch through
   `stream_errored()`; shutdown joins the thread; the handle reports sample
   count timeline parity (`discontinuities`, monotonic start timestamp).
2. **Detection probe** (`hark-meeting`, the `probe_win` seam — renamed `probe`
   with per-OS backends): `snapshot()` lists PipeWire capture-stream nodes
   (`Stream/Input/Audio`) per app, excluding Hark's own pid; `processes()`
   reads `/proc` (pid, ppid, exe). Window-title scan for browsers via `x11rb`
   on X11 sessions; none on Wayland (no universal API), so browser-held-mic
   detection is X11-only while native clients detect everywhere. App ids use
   Linux binary names, and `DEFAULT_APPS`/`BROWSERS`/`app_display_name` learn
   them without changing their Windows meanings.
3. **Change watcher**: the same PipeWire registry events wake the coordinator
   (node add/remove), replacing `RegNotifyChangeKeyValue`; the existing timed
   backstops and retry shape are unchanged. A missing/unreachable PipeWire
   keeps the polling fallback working.
4. **Meeting chord** (`hark-hotkey`): the Linux evdev listener gains the
   `ShortcutTracker` routing the Windows hook has — toggle on engage edge,
   watchdog healing, capture-tap precedence — behind the same
   `spawn_shared_listener` signature; evdev remains observe-only.
5. **Feature switch and UI**: `meetings_supported()` becomes
   `cfg!(any(windows, target_os = "linux"))`; every gate keyed on it (nav
   page, Settings section, tray entry, chord binding, coordinator start)
   lights up with no further per-platform edits.
6. **Share parity**: Linux save dialogs via `zenity` then `kdialog` (cancelled
   or absent helper returns None, as today); Word export enabled (`docx-rs` is
   cross-platform); "show in folder"/"open meetings folder" via the
   `org.freedesktop.FileManager1` D-Bus call with `xdg-open` fallback. The
   OS-native share sheet stays Windows-only (no Linux equivalent).
7. **Out of scope, by documented policy**: in-place self-update (package
   managers own the binary), lock-key swallowing (needs `EVIOCGRAB`),
   Wayland window titles, and per-process capture stricter than stream-node
   targeting when the session manager cannot honor it.

## Constraints

- New native dependency `pipewire` (crate 0.10, MIT; links `libpipewire-0.3`)
  only under `cfg(target_os = "linux")`; macOS and Windows builds unchanged.
- CI's Ubuntu leg gains `libpipewire-0.3-dev`; deb/rpm dependency inference
  (`$auto`/`auto-req`) picks the linked soname; PKGBUILD names `pipewire`.
- The ring/spool/chunker/AEC/finisher contracts are untouched; the recorder
  must not need to know which platform feeds it.
- Verification: workspace fmt/clippy/tests green (this Linux machine), plus
  live PipeWire smoke runs for capture and probe. Mic-in-a-real-call behavior
  still needs native user validation, as the Windows path did.

## Delivery

Single working-tree change (this is one seam, split would ship a broken
feature switch), changelog under Unreleased, docs synced (`MEETINGS.md`,
`AUDIO_CAPTURE.md`, `packaging/LINUX.md`, notices).
