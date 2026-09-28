# Intent: finish the Meetings workflow

Status: Approved
Approved by: product owner, 2026-09-28 ("complete them all 1-6", then "commit and push and tag all")

People need to improve saved meeting transcripts, start and stop meetings more
quickly, share useful portions in common formats, and use meeting transcription
without requiring a Deepgram account. Meetings played through speakers should
also have optional echo reduction using the evaluated Rust AEC3 engine.

Scope is the six ordered Polish items in
`tasks/2026-09-26-plan-meeting-transcription.md`. Existing recording, notes,
speaker renaming, and search must continue working. Keep all locked decisions.

## Approved implementation choices

- Excerpt audio: decode and re-encode approved on 2026-09-28.
- DOCX: `docx-rs` 0.4.22, default features off, approved on 2026-09-28.
- Echo cancellation: on 2026-09-28 the product owner delegated the engine choice and explicitly waived further speakerphone testing: "Just ship whatever fix for it you recommend". Select Rust `aec3` 0.4.0, opt-in, with original-microphone fallback. Existing headphone recordings remain unchanged by default.
- The later blanket instruction authorizes the separate feature commits, pushes, and tags; no repeated approval is required. The user reaffirmed permission to commit, push to main, and tag this build on 2026-09-28.

## Lessons Learned / Gotchas

Record verified implementation findings in the approved plan's section 8 and
route reusable findings through `/add-lesson`.

The production AEC review found that equal-length output can still hide ending
audio inside a filter. The implementation now verifies the delay and preserves
the original ending samples across stop and fallback. See the plan's 0.57.0
entry for this finding and the device-close race regression.
