<!-- PAGE_ID: hark_05_data_storage -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [crates/hark-store/src/lib.rs:1-422](../../crates/hark-store/src/lib.rs#L1-L422)
- [crates/hark-store/migrations/001_init.sql:1-31](../../crates/hark-store/migrations/001_init.sql#L1-L31)
- [crates/hark-store/migrations/002_stats_total_ms.sql:1-8](../../crates/hark-store/migrations/002_stats_total_ms.sql#L1-L8)
- [crates/hark-store/migrations/003_entries_invocation.sql:1-6](../../crates/hark-store/migrations/003_entries_invocation.sql#L1-L6)
- [crates/hark-store/migrations/004_meetings.sql](../../crates/hark-store/migrations/004_meetings.sql)
- [crates/hark-store/migrations/006_meeting_transcript_status.sql](../../crates/hark-store/migrations/006_meeting_transcript_status.sql)
- [crates/hark-store/migrations/007_meeting_speakers.sql:1-22](../../crates/hark-store/migrations/007_meeting_speakers.sql#L1-L22)
- [Numeric Insights migration](../../crates/hark-store/migrations/005_insight_events.sql), [persistence](../../crates/hark-store/src/insights/persistence.rs), [queries](../../crates/hark-store/src/insights/query.rs), and [word analysis](../../crates/hark-store/src/insights/patterns.rs)
- [Insights regression tests](../../crates/hark-store/tests/insights.rs)
- [crates/hark-store/src/meetings.rs:1-566](../../crates/hark-store/src/meetings.rs#L1-L566)
- [crates/hark-store/tests/meeting_speakers.rs](../../crates/hark-store/tests/meeting_speakers.rs)
- [crates/hark-store/tests/store.rs:1-465](../../crates/hark-store/tests/store.rs#L1-L465)
- [crates/hark-app/src/storage/mod.rs:1-249](../../crates/hark-app/src/storage/mod.rs#L1-L249)
- [crates/hark-app/src/storage/meetings.rs](../../crates/hark-app/src/storage/meetings.rs)
- [crates/hark-app/src/app.rs](../../crates/hark-app/src/app.rs)
- [crates/hark-config/src/lib.rs](../../crates/hark-config/src/lib.rs)
- [crates/hark-config/src/lib.rs](../../crates/hark-config/src/lib.rs)
- [crates/hark-pipeline/src/events.rs:6-40](../../crates/hark-pipeline/src/events.rs#L6-L40)

</details>

# Data Storage

Config schema 6 adds optional microphone echo reduction, defaulting off, while
preserving explicit meeting preferences and the schema-5 Gemini model setting.
Config schema 7 retires the Groq preset, rewriting saved Groq tables as
`openai-compatible` ([Configuration](CONFIGURATION.md#groq-retirement-schema-7)).
The TOML is at schema 7. The separate SQLite schema is versioned on its own
(migrations 001-006, including 005 for bounded numeric Insights and a
tracking-coverage timestamp); schema 7 does not touch it.
Migration attempts a versioned TOML backup before saving; failure keeps
the in-memory settings usable, and a failed backup leaves the original file alone
([config persistence](../../crates/hark-config/src/lib.rs),
[migration fixtures](../../crates/hark-config/src/meeting.rs)).

> **Related Pages**: [Architecture](ARCHITECTURE.md), [Configuration and Secrets](CONFIGURATION.md), [Desktop UI](../features/DESKTOP_UI.md)

---

<!-- BEGIN:AUTOGEN hark_05_data_storage_overview -->
## Overview

Hark stores dictation history, bounded numeric Insights, and lifetime counters in one local SQLite file, `<data-dir>/hark.db`. `Store::open` creates the parent directory, opens the database, applies embedded migrations, and seeds the singleton stats row. The transcript-bearing `NewDictation` and `Entry` types intentionally do not implement `Debug`, which keeps a reflexive debug log from exposing dictation text ([lib.rs:1-17](../../crates/hark-store/src/lib.rs#L1-L17), [lib.rs:71-110](../../crates/hark-store/src/lib.rs#L71-L110), [lib.rs:141-186](../../crates/hark-store/src/lib.rs#L141-L186)).

The implementation is local plaintext SQLite; it has no app-layer row encryption or multi-user isolation. History capture and numeric stats are separate controls: disabling capture prevents transcript rows from being stored, while numeric Insights and lifetime counters still advance ([lib.rs:204-245](../../crates/hark-store/src/lib.rs#L204-L245)).

The same database also holds meeting transcripts, notes and speaker names (migrations 004, 006 and 007; see [Schema](#schema)). Meeting audio itself is not a database column: it lives on disk under `<data_dir>/meetings/<id>/`, and `hark-app/src/storage/meetings.rs` is the one writer for both the meeting's rows and its audio folder, so a size check against the filesystem and the database's idea of which meetings exist never disagree ([storage/meetings.rs:1-11](../../crates/hark-app/src/storage/meetings.rs#L1-L11)).

| Platform | Data directory |
|---|---|
| Windows | `%APPDATA%\hark` |
| macOS | `~/Library/Application Support/hark` |
| Linux | `$XDG_DATA_HOME/hark` when that variable is absolute; otherwise `~/.local/share/hark` |

The platform resolution lives in `default_data_dir`; if no OS data directory can be resolved, startup disables history and stats for that session without disabling dictation ([lib.rs](../../crates/hark-config/src/lib.rs), [app.rs](../../crates/hark-app/src/app.rs)).

Sources: [crates/hark-store/src/lib.rs:1-17](../../crates/hark-store/src/lib.rs#L1-L17), [crates/hark-store/src/lib.rs:141-186](../../crates/hark-store/src/lib.rs#L141-L186), [crates/hark-config/src/lib.rs](../../crates/hark-config/src/lib.rs), [crates/hark-app/src/app.rs](../../crates/hark-app/src/app.rs)
<!-- END:AUTOGEN hark_05_data_storage_overview -->

---

<!-- BEGIN:AUTOGEN hark_05_data_storage_schema -->
## Schema

The schema is an append-only sequence of seven embedded migrations. `PRAGMA user_version` records how many have been applied; opening an older database runs only the remaining migrations, each in its own transaction ([lib.rs:35-45](../../crates/hark-store/src/lib.rs#L35-L45), [lib.rs:188-202](../../crates/hark-store/src/lib.rs#L188-L202)).

| Migration | Change | Compatibility behavior |
|---|---|---|
| `001_init.sql` | Creates `entries`, its timestamp index, and singleton `stats` | The app seeds stats row `id = 1` without replacing existing counters ([001_init.sql:1-31](../../crates/hark-store/migrations/001_init.sql#L1-L31), [lib.rs:176-181](../../crates/hark-store/src/lib.rs#L176-L181)) |
| `002_stats_total_ms.sql` | Adds `stats.total_ms NOT NULL DEFAULT 0` | Existing counters survive; pre-migration dictations contribute zero to the new sum ([002_stats_total_ms.sql:1-8](../../crates/hark-store/migrations/002_stats_total_ms.sql#L1-L8), [store.rs:427-465](../../crates/hark-store/tests/store.rs#L427-L465)) |
| `003_entries_invocation.sql` | Adds nullable `entries.invocation` | Existing rows read as non-invocations; new rows round-trip the trigger ([003_entries_invocation.sql:1-6](../../crates/hark-store/migrations/003_entries_invocation.sql#L1-L6), [store.rs:378-424](../../crates/hark-store/tests/store.rs#L378-L424)) |
| `004_meetings.sql` | Adds `meetings`, `meeting_segments`, `meeting_speakers`, and an external-content FTS5 index over segment text | A fresh table set; nothing pre-existing to migrate. `meeting_segments`/`meeting_speakers` cascade-delete with their meeting, which only takes effect because `Store::init` now turns `PRAGMA foreign_keys` on for every connection ([004_meetings.sql](../../crates/hark-store/migrations/004_meetings.sql), [lib.rs:170-172](../../crates/hark-store/src/lib.rs#L170-L172)) |
| `005_insight_events.sql` | Adds indexed `insight_events` and singleton `insight_tracking` | Backfills retained, post-reset history within 366 days; keeps historical duration, corrections, and app identity NULL. Does not increment existing lifetime counters ([migration](../../crates/hark-store/migrations/005_insight_events.sql), [backfill](../../crates/hark-store/src/insights/persistence.rs)) |
| `006_meeting_transcript_status.sql` | Adds nullable `meetings.transcript_status`: `pending` or `pending:<unix ms>` while a final-pass retry is scheduled, `failed:<reason>` once Hark stops trying, NULL when nothing is outstanding | No default and no backfill: every earlier meeting reads NULL. The stored text is a label, never transcript content; `hark-pipeline` owns the vocabulary and the store only reads and writes it ([migration](../../crates/hark-store/migrations/006_meeting_transcript_status.sql), [meetings.rs:297-309](../../crates/hark-store/src/meetings.rs#L297-L309)) |
| `007_meeting_speakers.sql` | Adds `meeting_speaker_names` (primary key `(meeting_id, speaker_key)`, `display_name`, `source` of `user` or `guess`, and a nullable `evidence_ms` for the line a guess was heard in), nullable `meeting_segments.speaker_override`, and nullable `meetings.notes_stale_ms` | Copies every `meeting_speakers` row in as key `them:<speaker>` with source `user`, inside the migration transaction, so dormant meetings migrate too and nothing is converted lazily on read. `meeting_speakers` stays, frozen and unread, so a build rolled back to 006 still finds the table it queries; a later migration drops it. Every earlier segment has no override and every earlier meeting has no stale stamp ([migration](../../crates/hark-store/migrations/007_meeting_speakers.sql#L1-L22), [meetings.rs:1-19](../../crates/hark-store/src/meetings.rs#L1-L19)) |

Meeting search does not need a `LIKE` fallback: the FTS5 module is compiled into the SQLite Hark bundles, which `hark-store/tests/meetings.rs` asserts directly rather than assuming ([meetings.rs](../../crates/hark-store/src/meetings.rs)). `meeting_segments_fts` is kept in sync by the standard insert/delete/update trigger trio rather than by application code, so a segment written through any path is searchable ([004_meetings.sql](../../crates/hark-store/migrations/004_meetings.sql)).

```mermaid
erDiagram
    ENTRIES {
        integer id PK
        integer ts_ms
        text raw_text
        text final_text
        text voice
        text stt_provider
        text stt_model
        text cleanup_model
        text invocation
        integer stt_ms
        integer cleanup_ms
        integer total_ms
    }
    STATS {
        integer id PK
        integer dictations
        integer words
        integer audio_ms
        integer stt_ms
        integer cleanup_ms
        integer total_ms
        integer since_ts_ms
    }
```

`entries`, `stats`, and `insight_events` intentionally have independent lifecycles. Clearing history deletes only entries; resetting stats zeroes the fixed stats row, deletes numeric events, and restarts the tracking timestamp ([lib.rs:340-343](../../crates/hark-store/src/lib.rs#L340-L343), [lib.rs:364-380](../../crates/hark-store/src/lib.rs#L364-L380)). `audio_ms` feeds lifetime stats and new numeric events, but is not stored on each transcript history entry ([lib.rs:71-110](../../crates/hark-store/src/lib.rs#L71-L110)).

Sources: [crates/hark-store/src/lib.rs:24-45](../../crates/hark-store/src/lib.rs#L24-L45), [crates/hark-store/src/lib.rs:163-186](../../crates/hark-store/src/lib.rs#L163-L186), [crates/hark-store/migrations/001_init.sql:1-31](../../crates/hark-store/migrations/001_init.sql#L1-L31), [crates/hark-store/migrations/002_stats_total_ms.sql:1-8](../../crates/hark-store/migrations/002_stats_total_ms.sql#L1-L8), [crates/hark-store/migrations/003_entries_invocation.sql:1-6](../../crates/hark-store/migrations/003_entries_invocation.sql#L1-L6), [crates/hark-store/migrations/004_meetings.sql](../../crates/hark-store/migrations/004_meetings.sql), [crates/hark-store/migrations/006_meeting_transcript_status.sql](../../crates/hark-store/migrations/006_meeting_transcript_status.sql), [crates/hark-store/migrations/007_meeting_speakers.sql:1-22](../../crates/hark-store/migrations/007_meeting_speakers.sql#L1-L22), [crates/hark-store/src/meetings.rs:1-566](../../crates/hark-store/src/meetings.rs#L1-L566)

### Meeting speakers

Speaker identity is a text key, `me`, `them`, `them:<n>` or `added:<n>`, whose vocabulary `hark_meeting::speakers` owns; the store treats it as opaque, and names are keyed by key rather than by name so two people called "Sam" cannot collide ([meetings.rs:10-14](../../crates/hark-store/src/meetings.rs#L10-L14)). A name row records who set it: `NameSource::User` is never overwritten, a `Guess` is replaced by a later guess, and unrecognized source text from a newer build reads as `User`, the safe side ([meetings.rs:80-115](../../crates/hark-store/src/meetings.rs#L80-L115)). `meeting()` returns `StoredSegment`s carrying the row id (what a reassignment names) and `speaker_override`, speaker names ordered by key, and `notes_stale_ms` ([meetings.rs:65-128](../../crates/hark-store/src/meetings.rs#L65-L128), [meetings.rs:478-541](../../crates/hark-store/src/meetings.rs#L478-L541)). The store API:

| Method | Behavior |
|---|---|
| `set_speaker_name(id, key, name)` | A user rename. A name that is blank after trimming deletes the row, reverting to the default label; otherwise it upserts with source `user` and clears any guess evidence. A no-op for an unknown meeting ([meetings.rs:330-350](../../crates/hark-store/src/meetings.rs#L330-L350)) |
| `apply_speaker_names(id, names)` | Pipeline names (guesses, and the "Your name" row) in one transaction. A row is written only where the key has no name or its name is itself a guess, blank names are skipped, and the return value is the number of rows written ([meetings.rs:352-382](../../crates/hark-store/src/meetings.rs#L352-L382)) |
| `assign_segment_speaker(id, segment_id, key, now_ms)` | Sets or clears (`None`) one line's `speaker_override` and, in the same transaction, stamps `notes_stale_ms` when the meeting has notes. Returns `None` and changes nothing when the segment is not in that meeting; otherwise whether the notes are now stale ([meetings.rs:384-410](../../crates/hark-store/src/meetings.rs#L384-L410)) |
| `set_refreshed_notes(id, notes_json, requested_ms)` | Stores rewritten notes and clears the stale stamp only when it is not newer than `requested_ms`, so a line moved while the rewrite ran keeps the notes stale. The title is never touched ([meetings.rs:412-429](../../crates/hark-store/src/meetings.rs#L412-L429)) |

Explicit meeting reprocessing uses `reprocess_meeting_segments`: in one transaction it deletes the meeting's `meeting_speaker_names` rows (and the frozen legacy `meeting_speakers` rows, so a build rolled back to 006 does not show names from the old diarization), clears `notes_stale_ms`, optionally replaces the notes with the caller's `baked_notes`, replaces the segments, and marks the meeting refined, with FTS updated by the segment triggers. The title and action-item checkboxes are kept. The ordinary final-pass `replace_meeting_segments` shares the same helper but resets nothing, so names, overrides and notes survive it. A failed insert rolls the whole transaction back ([meetings.rs:193-264](../../crates/hark-store/src/meetings.rs#L193-L264)). The app's storage worker acknowledges this commit before the meeting UI reports rerun success, and computes `baked_notes` first by rendering the stored notes through the old roster, so the notes keep the old names after the reset ([storage/meetings.rs:155-169](../../crates/hark-app/src/storage/meetings.rs#L155-L169)). `set_meeting_transcript_status` writes the final-pass status through the same worker (`MeetingCmd::TranscriptStatus`); it mirrors the retry marker the pipeline keeps in the meeting folder, so the database is what the UI reads after a restart and never what drives a retry ([store implementation](../../crates/hark-store/src/meetings.rs), [rollback and preservation tests](../../crates/hark-store/tests/meetings.rs), [speaker API tests](../../crates/hark-store/tests/meeting_speakers.rs), [storage acknowledgement](../../crates/hark-app/src/storage/meetings.rs)).
<!-- END:AUTOGEN hark_05_data_storage_schema -->

---

<!-- BEGIN:AUTOGEN hark_05_data_storage_history -->
## History API

Every `Store` owns one synchronous `rusqlite::Connection`. File-backed connections request WAL mode, use `synchronous = NORMAL`, and wait up to five seconds when the database is busy, which supports the app's one-writer/one-reader arrangement ([lib.rs:47-47](../../crates/hark-store/src/lib.rs#L47-L47), [lib.rs:163-169](../../crates/hark-store/src/lib.rs#L163-L169)).

| Method | Behavior |
|---|---|
| `record(d, capture)` | In one transaction, optionally inserts the content row, always records a numeric event, and updates lifetime stats ([lib.rs:204-245](../../crates/hark-store/src/lib.rs#L204-L245)) |
| `entries(search, limit, offset)` | Returns newest first by `(ts_ms DESC, id DESC)`; searches both raw and final text ([lib.rs:265-313](../../crates/hark-store/src/lib.rs#L265-L313)) |
| `entry_count(search)` | Counts rows using the same blank/search semantics as `entries` ([lib.rs:315-330](../../crates/hark-store/src/lib.rs#L315-L330)) |
| `delete_entry(id)` | Deletes one row and returns `false` when it was already absent ([lib.rs:332-338](../../crates/hark-store/src/lib.rs#L332-L338)) |
| `clear_entries()` | Deletes all history rows without changing numeric Insights or lifetime stats ([lib.rs:340-343](../../crates/hark-store/src/lib.rs#L340-L343)) |

Search is a case-insensitive SQLite `LIKE` substring match. `%`, `_`, and `\` in user input are escaped, so they remain literal characters rather than becoming pattern operators ([lib.rs:292-301](../../crates/hark-store/src/lib.rs#L292-L301), [lib.rs:404-415](../../crates/hark-store/src/lib.rs#L404-L415)). Tests cover stable newest-first pagination and wildcard escaping ([store.rs:91-161](../../crates/hark-store/tests/store.rs#L91-L161)).

Sources: [crates/hark-store/src/lib.rs:163-169](../../crates/hark-store/src/lib.rs#L163-L169), [crates/hark-store/src/lib.rs:204-343](../../crates/hark-store/src/lib.rs#L204-L343), [crates/hark-store/src/lib.rs:404-415](../../crates/hark-store/src/lib.rs#L404-L415), [crates/hark-store/tests/store.rs:91-176](../../crates/hark-store/tests/store.rs#L91-L176)
<!-- END:AUTOGEN hark_05_data_storage_history -->

---

<!-- BEGIN:AUTOGEN hark_05_data_storage_stats -->
## Lifetime Stats and Detailed Insights

`Stats` holds lifetime counts and summed durations: dictations, words, captured audio, STT, cleanup, release-to-inject time, and the timestamp from which counters apply. Recording a completed dictation updates those counters, its numeric event, and the optional transcript row in one transaction. History capture controls only the transcript row.

Word counting uses whitespace tokenization. Ordinary dictations credit inserted `final_text`; invocation dictations credit `raw_text` so a short trigger does not inflate dictated-word totals. Numeric events also store the full inserted word count when an invocation fired. The UI calls this **Words in invocation output**: anywhere-scope invocations include surrounding speech, so it is not a count of only the configured expansion.

Each numeric event records timestamp, words, measured clip duration, completion latency, provider/voice labels, invocation flag/output word count, actual Spellbook replacements, and an optional foreground app label. Neither transcript text, invocation phrases, expansion text, window/document titles, nor audio is copied into that table. Correction totals include both local Spellbook passes; they do not estimate cloud cleanup edits or transcription accuracy.

Migration 005 backfills only surviving history within retention and at or after the last stats reset. Recoverable counts and latency remain usable; unknown duration, correction counts, and app labels stay NULL. `insight_tracking.since_ts_ms` starts when complete numeric recording begins, separately from the oldest backfilled row. The query reports coverage for selected and preceding periods rather than treating sparse legacy history as a complete record.

### Aggregation and interpretation

`Store::insights` builds day buckets in the requested local timezone, including empty dates. Selected periods are inclusive calendar days ending today; UI choices are 7, 30, and 90 days. Activity covers 366 local dates. Current streak ends today, or yesterday if today is empty; longest streak is the longest observed run within retained numeric history. It is not an all-time streak claim.

Pace divides words from measured rows by those rows' clip durations. Estimated time saved subtracts measured clip duration from a 40 WPM typing baseline and floors at zero; missing durations are excluded from both numerator and denominator. Clip time includes capture padding. Median/p95 use recorded completion latencies; these are not failure-rate statistics. Provider, voice, optional app, and busiest-hour aggregates use the same selected period; absent app labels are counted separately.

Opted-in word analysis reads retained history on the worker, using final text for ordinary dictations and spoken raw text for invocations. It excludes common words, counts two-/three-word phrases, and bounds work to 5,000 recent entries and 200,000 tokens. Results report how much retained history was analyzed and whether truncated. Derived words/phrases are not persisted or uploaded.

### Reset and deletion

`reset_stats(now_ms)` zeroes lifetime counters, removes all numeric events/app labels, and restarts numeric tracking. It preserves transcripts, Spellbook, and invocations. Consequently opted-in word analysis can still calculate results from retained transcripts after a numeric reset. Deleting or clearing history removes the text available to that analysis but preserves numeric details until they expire or are reset.

Sources: [store lifecycle](../../crates/hark-store/src/lib.rs), [Insights types](../../crates/hark-store/src/insights/types.rs), [queries](../../crates/hark-store/src/insights/query.rs), [word analysis](../../crates/hark-store/src/insights/patterns.rs), [migration/backfill](../../crates/hark-store/src/insights/persistence.rs), [regression tests](../../crates/hark-store/tests/insights.rs).
<!-- END:AUTOGEN hark_05_data_storage_stats -->

---

<!-- BEGIN:AUTOGEN hark_05_data_storage_retention -->
## Retention and Pruning

`Retention` combines `max_entries` and `max_age_days`; configuration validation requires both to be at least one, while the store simply executes the supplied policy. Defaults are 1,000 entries and 90 days ([lib.rs:126-134](../../crates/hark-store/src/lib.rs#L126-L134), [lib.rs](../../crates/hark-config/src/lib.rs)).

`Store::prune` applies both rules in one transaction:

1. Delete rows with `ts_ms < now_ms - max_age_days * 86_400_000`. The boundary is strict, so an entry exactly at the cutoff remains.
2. Order the survivors newest-first by timestamp and id, then delete everything after `max_entries`.

The return value includes deleted history rows and expired numeric Insight events. Pruning never changes lifetime stats, even when both rules remove history in the same call ([lib.rs:247-263](../../crates/hark-store/src/lib.rs#L247-L263), [store.rs:226-309](../../crates/hark-store/tests/store.rs#L226-L309)).

The app prunes after every recorded dictation and also sends a standalone prune when a pipeline starts, so lowering retention takes effect after save/startup rather than waiting for another dictation ([storage/mod.rs:33-54](../../crates/hark-app/src/storage/mod.rs#L33-L54), [storage/mod.rs:204-225](../../crates/hark-app/src/storage/mod.rs#L204-L225)).

Numeric Insights have a separate fixed 366-day retention. Events older than `now_ms - 366 × 86,400,000` are physically pruned when opening the database or applying retention after a recording/pipeline start. Insights queries exclude expired rows even before that pruning; lifetime counters are unaffected. Transcript retention and capture settings do not remove these numeric rows. Calendar grouping follows local dates even though expiry is a fixed elapsed-time cutoff ([persistence](../../crates/hark-store/src/insights/persistence.rs), [query](../../crates/hark-store/src/insights/query.rs)).

Meeting audio has its own, separate cap: a circular limit in megabytes (`[meeting] audio_cap_mb`, default 5 GB), enforced against actual bytes on disk rather than a database column, and it only ever deletes audio, never a transcript, note, or segment row. See [Meetings](../features/MEETINGS.md#storage-and-the-audio-cap) for the eviction rules ([hark-meeting/src/storage.rs:1-9](../../crates/hark-meeting/src/storage.rs#L1-L9)).

Sources: [crates/hark-store/src/lib.rs:126-134](../../crates/hark-store/src/lib.rs#L126-L134), [crates/hark-store/src/lib.rs:247-263](../../crates/hark-store/src/lib.rs#L247-L263), [crates/hark-store/tests/store.rs:226-309](../../crates/hark-store/tests/store.rs#L226-L309), [crates/hark-app/src/storage/mod.rs:204-225](../../crates/hark-app/src/storage/mod.rs#L204-L225), [crates/hark-meeting/src/storage.rs:1-9](../../crates/hark-meeting/src/storage.rs#L1-L9)
<!-- END:AUTOGEN hark_05_data_storage_retention -->

---

<!-- BEGIN:AUTOGEN hark_05_data_storage_integration -->
## App Integration

`hark-app` retains a reader for existing history/meeting views, while the `hark-storage` worker owns the sole writer and all new Insights aggregation/text analysis. `StorageCmd::GetInsights` and `GetRecentEntries` return results through reply channels, and completion explicitly wakes the root viewport. All mutations travel through `StorageCmd`; successful changes increment an atomic generation counter so cached panels can refresh ([storage/mod.rs:1-10](../../crates/hark-app/src/storage/mod.rs#L1-L10), [storage/mod.rs:78-112](../../crates/hark-app/src/storage/mod.rs#L78-L112), [storage/mod.rs:140-200](../../crates/hark-app/src/storage/mod.rs#L140-L200)).

`Record` carries the capture and retention policy from the pipeline run that produced it. The worker stamps the record at persistence time, writes it, then prunes. The numeric correction and optional app fields are carried directly with `DictationRecord`. Records originate only from the post-injection `PipelineEvent::Injected`, so a storage failure cannot undo or block text that has already reached the focused application ([storage/mod.rs:33-76](../../crates/hark-app/src/storage/mod.rs#L33-L76), [storage/mod.rs:204-244](../../crates/hark-app/src/storage/mod.rs#L204-L244), [events.rs:75-90](../../crates/hark-pipeline/src/events.rs#L75-L90)).

Home/Insights caches re-query when generation, selected range, local date, or text-analysis choice changes. An obsolete reply is discarded rather than overwriting a newer range. Foreground-app collection is a separate opt-in one-shot worker query at engagement; injection never waits for it. Late, unavailable, unsupported, or disabled app identity remains unknown. No extra permissions, window titles, or provider calls are introduced for Insights ([cache](../../crates/hark-app/src/ui/insights_cache.rs), [app probe](../../crates/hark-pipeline/src/foreground.rs)).

Shutdown is bounded. Dropping `StorageHandle` removes its sender and normally joins the worker after every queued write drains. If an abandoned pipeline request keeps another sender alive, the handle waits only 500 ms, logs a warning, and leaves that worker rather than holding application exit open indefinitely ([storage/mod.rs:24-31](../../crates/hark-app/src/storage/mod.rs#L24-L31), [storage/mod.rs:114-138](../../crates/hark-app/src/storage/mod.rs#L114-L138)). Tests cover both the normal final-write flush and the bounded abandoned-sender case ([storage/mod.rs:289-317](../../crates/hark-app/src/storage/mod.rs#L289-L317)).

`StorageCmd::Meeting` carries the same guarantee for meeting rows and audio: `storage::meetings::apply` is the one function that touches both the `meetings`/`meeting_segments` tables and the `<data_dir>/meetings/<id>/` folder, called from the same worker thread and after the same generation bump, so the Meetings page never has to reconcile two write paths ([storage/mod.rs:9-10](../../crates/hark-app/src/storage/mod.rs#L9-L10), [storage/mod.rs:52-53](../../crates/hark-app/src/storage/mod.rs#L52-L53), [storage/meetings.rs:61-150](../../crates/hark-app/src/storage/meetings.rs#L61-L150), [storage/meetings.rs:153-274](../../crates/hark-app/src/storage/meetings.rs#L153-L274)).

The speaker work rides the same queue as `MeetingCmd` variants. `SetSpeakerName` is the user's rename, from the Speakers card or from naming a new speaker while moving a line, and an empty name reverts the key to its default label. `SpeakerNames` carries the pipeline's guesses and the "Your name" row after a final pass; it logs only how many of the names were applied and returns changed only when a row was written ([storage/meetings.rs:105-117](../../crates/hark-app/src/storage/meetings.rs#L105-L117), [storage/meetings.rs:219-229](../../crates/hark-app/src/storage/meetings.rs#L219-L229)). `AssignSpeaker` moves one line and replies on its own channel with whether the notes went stale; a line that is gone (a re-run replaced the transcript) or a failed write answers with a fixed sentence, `ASSIGN_FAILED`, and logs only ids ([storage/meetings.rs:13](../../crates/hark-app/src/storage/meetings.rs#L13), [storage/meetings.rs:118-125](../../crates/hark-app/src/storage/meetings.rs#L118-L125), [storage/meetings.rs:230-249](../../crates/hark-app/src/storage/meetings.rs#L230-L249)). `RefreshedNotes` is a rewrite finishing: it re-reads the notes stored now and carries over ticked action items (matching ignores default labels), applies the stale-stamp rule above, and is dropped for a meeting that has no notes. `Notes` sets the generated title only while the meeting has none, with its `{{key}}` speaker tokens written out at that moment, so a title does not follow later renames ([storage/meetings.rs:15-44](../../crates/hark-app/src/storage/meetings.rs#L15-L44), [storage/meetings.rs:178-214](../../crates/hark-app/src/storage/meetings.rs#L178-L214)). Every screen and export builds its labels and colors from one `roster_of(detail)` over the stored segments and names, so a stale name cannot linger on one surface ([storage/meetings.rs:46-59](../../crates/hark-app/src/storage/meetings.rs#L46-L59)).

Meeting deletion has an explicit reply channel. The worker deletes the recording directory before the database row, treating only an already-absent recording directory as benign. Filesystem failure preserves the meeting row and its relation to any remaining audio, permitting a retry instead of leaving unknown orphaned audio. Filesystem removal and the database write are not one transaction: partial audio removal cannot be undone, and a later database failure is reported separately so a retry can finish the row deletion. The worker wakes the root viewport even when deletion fails without changing the generation; the detail view waits for acknowledgement before claiming success ([deletion implementation and fixtures](../../crates/hark-app/src/storage/meetings.rs), [worker wakeup](../../crates/hark-app/src/storage/mod.rs), [UI reply handling](../../crates/hark-app/src/ui/meetings/detail.rs)).

Sources: [crates/hark-app/src/storage/mod.rs:1-249](../../crates/hark-app/src/storage/mod.rs#L1-L249), [crates/hark-app/src/storage/mod.rs:289-317](../../crates/hark-app/src/storage/mod.rs#L289-L317), [crates/hark-app/src/storage/meetings.rs](../../crates/hark-app/src/storage/meetings.rs), [crates/hark-pipeline/src/events.rs:75-90](../../crates/hark-pipeline/src/events.rs#L75-L90)
<!-- END:AUTOGEN hark_05_data_storage_integration -->

---

Gemini playback-speaker IDs reserve the high bit and encode the five-minute window
alongside its local speaker number. They remain separate identities in storage,
rename controls, and exports; no cross-window match is implied
([parse](../../crates/hark-stt/src/meeting_gemini.rs),
[label](../../crates/hark-meeting/src/export.rs)).

Mac native capture and UI use the same local history and meeting storage workers as Windows. Permission changes restart capture without replacing the data directory. Updates stage their DMG outside the signed app and replace only the app bundle; settings, keychain credentials, meeting audio and history remain in their existing OS locations ([app lifecycle](../../crates/hark-app/src/app.rs), [Mac updater](../../crates/hark-update/src/macos.rs)).
