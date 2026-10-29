# Intent: Meeting transcripts should say who said what

- **Status:** Approved
- **Originator:** Chaz Vossburg
- **Captured:** 2026-10-29
- **Approved by:** Chaz Vossburg (product owner), 2026-10-29 ("Approve all 3")

## Problem

After a call, a transcript only says "Me" and "Speaker 1/2/3", and only the other side's
speakers can be renamed. A line that lands on the wrong speaker cannot be fixed. Every
speaker looks the same on the page, so you have to read the label on each line to follow
who is talking.

## Proposed outcome

After a call:

- Every speaker is labeled with Deepgram (this already runs).
- The microphone owner is matched automatically and named.
- Hark guesses the other speakers, and people can attach a name to each voice.
- Each speaker gets a color, and the color goes everywhere that speaker appears.

The minimum that counts as solved:

- Each speaker keeps a separate track.
- Any line the app got wrong can be reassigned to the right speaker, including "Me".
- Each speaker has a distinct color.

## Affected users and systems

- **Users:** the single Hark user, after every recorded meeting.
- **Systems:** the meeting detail view, the stored transcript, exports and shares (Markdown,
  text, Word, subtitles), and the after-call notes. This is a best guess; the spec stage
  should confirm it.

## Constraints

- Deepgram is the labeling provider. The Gemini final pass exists too, but its speaker
  numbers restart every five minutes. Unconfirmed: whether the work must cover Gemini.
- A speaker is never identified by color alone. The name stays visible as text.

## Decisions (originator, 2026-10-29)

- **Names apply within one meeting only.** Hark does not need to recognise the same voice
  in a later meeting. (Asked: "should Hark recognise that same person in later meetings?"
  Answer: "no".)
- **"Guess the other speakers" means names spoken in the call.** For example, "Thanks,
  Sam" suggests a speaker is Sam. Past meetings and calendar invites are not the source.
- **A guessed name is applied right away**, and the user can fix it.
- **Renaming a speaker rewrites the notes and summary** with the new name. So does
  reassigning a line, wherever it changes who said something.

## Out of scope

- Recognising a voice across meetings (decided 2026-10-29).
- Unconfirmed: live (during-call) speaker labeling.

## Open questions

None outstanding.

## Lessons Learned / Gotchas

(Filled in after the work ships. Route anything generalisable to LL-G with `/add-lesson`.)
