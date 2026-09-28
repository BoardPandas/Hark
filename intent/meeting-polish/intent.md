# Intent: finish the Meetings workflow

Status: Approved
Approved by: product owner, 2026-09-28 ("complete them all 1-6", then "commit and push and tag all")

People need to improve saved meeting transcripts, start and stop meetings more
quickly, share useful portions in common formats, and use meeting transcription
without requiring a Deepgram account. Meetings played through speakers should
also be evaluated for echo removal before a solution is selected.

Scope is the six ordered Polish items in
`tasks/2026-09-26-plan-meeting-transcription.md`. Existing recording, notes,
speaker renaming, and search must continue working. Keep all locked decisions.

## Approved implementation choices and remaining decision

- Excerpt audio: decode and re-encode approved on 2026-09-28.
- DOCX: `docx-rs` 0.4.22, default features off, approved on 2026-09-28.
- Echo cancellation: product owner chooses after comparison and a real-speakers test.
- The later blanket instruction authorizes the separate feature commits, pushes, and tags; no repeated approval is required. The production AEC engine choice remains open.

## Lessons Learned / Gotchas

Record verified implementation findings in the approved plan's section 8 and
route reusable findings through `/add-lesson`.
