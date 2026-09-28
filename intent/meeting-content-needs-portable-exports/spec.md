# Specification: subtitles, excerpts, Word, and Windows sharing

Status: Approved scope, implementing Polish item 4
Authority: [intent](intent.md) and [Meetings plan](../../tasks/2026-09-26-plan-meeting-transcription.md#polish)

1. Export SRT/VTT from segment start/end times, preserving overlap, clamping to the meeting end, and escaping content that could inject cues or markup.
2. Select an excerpt using transcript lines or an explicit time range. Decode saved audio, cut exact PCM samples, and export mono MP3 or WAV with a matching plain-text companion. Include crossing transcript segments in full with clipped/rebased times; omit meeting-wide notes.
3. Add text-only DOCX with approved docx-rs 0.4.22, default features disabled. Preserve document text, Unicode, line breaks, notes, action items, and speaker-labelled transcript.
4. Open the native Windows Share chooser for meeting text on the main UI thread. Retain and retire its callback safely. Keep save dialogs, encoding, and file writes on workers.
5. Protect Hark's retained recording directory, avoid unapproved companion-file overwrites, and preserve existing output on failed excerpt selection. Verify timing, selection, and serialization with fixtures; native chooser behavior requires a separate manual check.

Keep schema 4 and the previous three features. Exclude Gemini-specific speaker labels, Gemini processing, and AEC.
