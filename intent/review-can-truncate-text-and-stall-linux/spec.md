# Seven review repairs

Status: Authorized repair of the seven reviewed defects; no new product scope.
Source: owner request in this chat; reviewed baseline `6222187`.

## Required behavior

1. Gemini Live requires explicit turn completion. Premature socket closure or
   EOF returns a sanitized error so the preserved clip can be replayed within
   the existing retry budget; intentional meeting silence behavior remains.
2. Cleanup rejects known incomplete completion reasons even when content is
   nonempty, leaving the original transcript available through fail-open handling.
3. Spellbook replacement reuses matching canonical edge punctuation and is
   idempotent for terms such as C++, C#, and .NET. Sentence punctuation survives.
4. Full WAV and MP3 exports own an exclusively created temporary file. Failure
   preserves the destination and unrelated files and cleans up owned partials.
5. Every Linux PipeWire discovery roundtrip has a timeout and an error exit,
   including the first one, so service failure cannot strand the coordinator.
6. X11 meeting detection enumerates managed client windows, including clients
   nested under window-manager frames, with bounded fallback traversal.
7. Linux shortcuts process buffered key transitions in order without testing
   historical presses against later physical state; release recovery remains.

## Scope and verification

Add deterministic regressions at existing pure or local transport boundaries.
Run focused tests, both npm guards, formatting, strict Clippy, and workspace
tests. Report toolchain and native-hardware limitations explicitly. Update mapped
documentation and the changelog. The owner subsequently requested: "Commit and
push to main and tag a new build." Release 0.61.2 includes a matching patch bump,
commit on main, push, and build tag. Hosted build completion is verified separately.
