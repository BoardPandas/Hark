# Linux builds need full meeting feature parity with Windows

Meeting mode shipped Windows-first (D4): recording, auto-detection, the live
transcript, the final pass, notes, and the Share menu all exist, but the Linux
build hides all of it — `meetings_supported()` is `cfg!(windows)`, so Linux
users cannot record a meeting at all, let alone detect one or export it.

The pure machinery (session state machine, chunker, merge, spools, AEC, MP3,
STT passes, finisher, store) is already platform-neutral and tested on Linux.
What is missing is the platform seam: system-audio loopback capture (WASAPI
process loopback has no Linux path), the microphone-usage detection probe and
its change watcher (ConsentStore registry is Windows-only), and the meeting
toggle chord routing in the Linux evdev hook. A few Share-menu actions are also
compiled out (save dialogs, Word export, folder reveal).

The user asked (2026-09-29): "Build the linux versions to full feature parity
with the Windows version." Authorization covers implementing on Linux via
PipeWire, validating on real Linux hardware where possible, and the CI,
packaging, docs, and changelog that follow. Deliberate platform policy — no
in-place self-update (package managers own the binary) and no lock-key
swallowing (EVIOCGRAB takes the whole device) — remains as documented.

See the [specification](spec.md).
