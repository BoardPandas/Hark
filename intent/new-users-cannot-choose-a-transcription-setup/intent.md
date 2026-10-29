# Intent: New users cannot tell how to set up transcription

- **Status:** Approved
- **Originator:** Chaz Vossburg
- **Captured:** 2026-10-29
- **Approved by:** Chaz Vossburg (product owner), 2026-10-29 ("Approve all 3")

## Problem

New users don't know which model to choose or how to get an API key. Settings never says
that there is a free option. On-device transcription has its own Settings section, apart
from Speech-to-text, so the cloud-or-local decision is split across two places.

## Proposed outcome

- Settings and first-run setup carry plain tips at the point of decision. For example: "No
  paid AI key? Use on-device", what each model is for, and where to get a key.
- On-device lives inside the Speech-to-text section as a toggle, with a choice between
  Primary and Backup.
- A new user can reach working dictation without leaving the app to research models or
  keys.

## Affected users and systems

- **Users:** every first-time user, and anyone changing providers later.
- **Systems:** the Settings Speech-to-text and On-device sections, and first-run setup.

## Constraints

- The originator was unsure of the best UI and **delegated the design to Claude** for the
  spec stage.
- The cloud path stays the primary, default path (project rule). On-device stays optional.

## Decisions (originator, 2026-10-29)

- **The on-device model changes only with a Hark release.** It never updates itself.
  (Today Hark pins Parakeet TDT 0.6B v3, int8, with fixed checksums.)
- **Keep the current model unless a newer one is clearly better.** NVIDIA's English-only
  Parakeet builds, which sherpa-onnx can already run, are candidates. Switch only if a
  side-by-side test on real dictation shows a clear gain. "Not much difference" means keep
  v3.

## Out of scope

- The model updating itself (decided 2026-10-29).

## Open questions

- What counts as a clear enough gain to switch models? Owner: engineer, at spec stage,
  with the evidence shown to the product owner.

## Lessons Learned / Gotchas

(Filled in after the work ships. Route anything generalisable to LL-G with `/add-lesson`.)
