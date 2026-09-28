<!-- PAGE_ID: hark_04_configuration -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [crates/hark-config/src/lib.rs](../../crates/hark-config/src/lib.rs)
- [crates/hark-config/src/lib.rs](../../crates/hark-config/src/lib.rs)
- [crates/hark-config/src/lib.rs](../../crates/hark-config/src/lib.rs)
- [crates/hark-config/src/voice.rs:1-65](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L1-L65)
- [crates/hark-config/src/voice.rs:67-152](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L67-L152)
- [crates/hark-config/src/voice.rs:214-282](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L214-L282)
- [crates/hark-keychain/src/lib.rs:1-95](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L1-L95)
- [crates/hark-keychain/src/lib.rs:119-169](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L119-L169)
- [config/default-config.toml:1-48](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L1-L48)
- [crates/hark-config/src/meeting.rs](../../crates/hark-config/src/meeting.rs)
- [config/default-config.toml:82-119](../../config/default-config.toml#L82-L119)

</details>

# Configuration and Secrets

> **Related Pages**: [Overview](../OVERVIEW.md), [Data Storage](DATA_STORAGE.md), [Voice Cleanup](../features/VOICE_CLEANUP.md)

---

<!-- BEGIN:AUTOGEN hark_04_configuration_layers -->
## Configuration Layers

Hark layers three sources into one effective `Settings` value: compiled-in defaults, the user's `config.toml`, and (for API keys only) an environment override. The `hark-config` crate owns the TOML half; SQLite storage lives in `hark-store` instead ([lib.rs](../../crates/hark-config/src/lib.rs)).

| Layer | Source | Override order |
|---|---|---|
| Defaults | `Default` impls on each settings struct | lowest |
| Config file | `config.toml` (per-OS path, see [File Locations](#file-locations)) | medium |
| Environment | `HARK_STT_KEY` / `HARK_CLEANUP_KEY` (keys only, dev/CI path) | highest |

A missing config file is not an error: `Settings::load` treats `NotFound` as "use the defaults," which is how first-run onboarding works without a setup wizard ([lib.rs](../../crates/hark-config/src/lib.rs)). A missing *key* inside an existing file behaves the same way because every settings struct carries `#[serde(default)]`, so partially-written or hand-edited files fill in gaps from the corresponding `Default` impl. Unknown keys in the file are tolerated (forward compatibility) rather than rejected ([lib.rs](../../crates/hark-config/src/lib.rs)).

API keys never live in `config.toml` at all: they resolve through `HARK_STT_KEY` / `HARK_CLEANUP_KEY` or the OS keychain, and the empty-vs-unset distinction matters for the env path (`HARK_STT_KEY=` with nothing after it is treated as unset, not an empty key) ([lib.rs](../../crates/hark-config/src/lib.rs)).

```toml
# config/default-config.toml:9-11
# API keys NEVER live in this file. Set the HARK_STT_KEY environment
# variable (dev), or store a key in the OS keychain under service "hark"
# with the provider label as the account name (e.g. "deepgram").
```

Sources: [lib.rs](../../crates/hark-config/src/lib.rs), [lib.rs](../../crates/hark-config/src/lib.rs), [default-config.toml:1-11](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L1-L11)
<!-- END:AUTOGEN hark_04_configuration_layers -->

---

<!-- BEGIN:AUTOGEN hark_04_configuration_schema -->
## Settings Schema

The `Settings` struct is the single source of truth for the config schema; every section below is a field on it, each with its own `Default` impl ([lib.rs](../../crates/hark-config/src/lib.rs)). Saving re-serializes the full struct, so unknown keys a user hand-added are dropped once the schema is additive ([lib.rs](../../crates/hark-config/src/lib.rs)).

| Section | Key | Type | Default | Meaning |
|---|---|---|---|---|
| top-level | `version` | `u32` | `5` (`CONFIG_VERSION`) | Schema stamp written on save; older versions migrate with a backup on load ([config](../../crates/hark-config/src/lib.rs)) |
| `[provider]` | `kind` | enum | `"deepgram"` | STT provider: `deepgram` \| `openai` \| `groq` \| `openai-compatible` ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[provider]` | `base_url` | `Option<String>` | per-kind (e.g. `https://api.deepgram.com`) | Required explicit for `openai-compatible` ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[provider]` | `model` | `Option<String>` | per-kind (e.g. `nova-3`) | Resolved model name sent in the request ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[hotkey]` | `ptt_key` | `String` | `"LCtrl+LWin"` | `"+"`-joined chord; hold all keys to record, release any to stop ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[audio]` | `input_device` | `Option<String>` | `None` | cpal device name; `None` or a stale name falls back to the OS default mic ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[audio]` | `preroll_ms` | `u32` | `300` | Audio kept from before the chord registers ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[audio]` | `tail_ms` | `u32` | `150` | Audio kept after release; adds directly to release-to-inject latency ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[audio]` | `max_hold_s` | `u32` | `120` | Max hold; on exceed, transcribe what's captured so far ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[audio]` | `min_speech_ms` | `u32` | `250` | Holds shorter than this are dropped, no network request ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[audio]` | `silence_rms` | `f32` | `0.01` | Clips below this RMS are dropped, no network request ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[inject]` | `strategy` | enum | `"clipboard"` | `clipboard` (stash/set/paste/restore) or `type` (character-by-character) ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[inject]` | `set_paste_delay_ms` | `u64` | `50` | Delay between clipboard set and the synthesized paste ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[inject]` | `paste_restore_delay_ms` | `u64` | `50` | Delay between paste and restoring the old clipboard ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[inject]` | `clipboard_retries` | `u32` | `8` | Bounded retries while another process holds the clipboard ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[spellbook]` | `terms` | `Vec<String>` | `[]` | Canonical terms for phonetic correction and provider biasing; aliases the legacy `bias_terms` key ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[voice]` | see [Voice Presets Config](#voice-presets-config) | | | |
| `[history]` | `capture` | `bool` | `true` | `false` stores no dictation content; counters still tick ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[history]` | `max_entries` | `u32` | `1000` | Keep at most this many newest entries (>= 1) ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[history]` | `max_age_days` | `u32` | `90` | Delete entries older than this many days (>= 1) ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[updates]` | `check_on_startup` | `bool` | `true` | Check GitHub Releases once at startup and surface a banner ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[startup]` | `launch_at_login` | `bool` | `true` | Source of truth for the OS startup entry; `hark-autostart` reconciles to it ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| `[general]` | `always_on_top` | `bool` | `false` | Keep the main Hark window above other windows; applied at startup and after Save |
| `[general]` | `exit_on_close` | `bool` | `false` | Exit on the window's X instead of hiding in the tray. Close Program and tray Quit always exit |
| `[meeting]` | `enabled` | `bool` | `true` | Master switch for meeting transcription; every other `[meeting]` key is inert while this is `false` ([meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[meeting]` | `toggle_key` | `Option<String>` | `None` | Optional Windows start/stop chord; blank in the UI clears it. Conflicts with push-to-talk are rejected ([meeting settings](../../crates/hark-config/src/meeting.rs), [validation](../../crates/hark-config/src/lib.rs)) |
| `[meeting]` | `mic_device` | `Option<String>` | `None` | cpal device name for "Me"; unset falls back to the Windows communications-default microphone ([meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[meeting]` | `system_source` | enum | `"app"` | `app` (a detected meeting's own process tree) \| `all` (everything except Hark; always used for a manual start) ([meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[meeting]` | `echo_cancellation` | `bool` | `false` | **Reduce speaker echo**: process only the microphone against captured playback, before recording and live transcription. Applies from the next meeting after Save ([meeting.rs](../../crates/hark-config/src/meeting.rs), [settings](../../crates/hark-app/src/ui/settings/meetings.rs)) |
| `[meeting]` | `final_pass` | enum | `"deepgram"` | `deepgram` (full stereo pass), `gemini` (each track in five-minute windows), or `none` (keep live transcript). Missing keys do not change this choice ([meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[meeting]` | `gemini_model` | `String` | `"gemini-3.8-flash"` | Independent Files final-pass model; blank values are rejected. Uses `HARK_GEMINI_KEY` or keychain account `gemini` ([settings](../../crates/hark-app/src/ui/settings/meetings.rs)) |
| `[meeting]` | `audio_cap_mb` | `u32` | `5120` | Circular storage cap for kept recordings, in MB; `0` deletes audio as soon as it is processed. Clamped to `0..=1_048_576` (1 TB) at load ([meeting.rs](../../crates/hark-config/src/meeting.rs), [meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[meeting]` | `compress_audio` | `bool` | `true` | Re-encode kept recordings to one stereo MP3 after processing, then delete the WAVs ([meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[meeting]` | `auto_detect` | enum | `"ask"` | `off` \| `ask` (non-modal prompt) \| `auto` (start silently; the tray/live indicator stay visible either way) ([meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[meeting]` | `auto_stop_after_s` | `u32` | `15` | Stop once the detected app has released the mic this long; `0` disables auto-stop. The default was `60` until 0.50.3; the schema v2 -> v3 migration moves a saved `60` (the old default, written into every file) to `15` once, and leaves any other value alone. A manual start adopting a detected call follows its auto-stop; a manual start with no adopted call does not auto-stop ([meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[meeting]` | `detect_apps` | `Option<Vec<String>>` | `None` | Override for the built-in app-match list; `None` uses `hark-meeting`'s defaults ([meeting.rs](../../crates/hark-config/src/meeting.rs)) |
| `[[invocations.entries]]` | `phrase` | `String` | `""` | Trigger phrase to listen for; also the entry's identity ([invocations.rs:46-52](https://github.com/BoardPandas/Hark/blob/bcfcc3fef6f02252870fc3f06440d99992818ade/crates/hark-config/src/invocations.rs#L46-L52)) |
| `[[invocations.entries]]` | `aliases` | `Vec<String>` | `[]` | Exact alternate transcriptions that fire the same invocation |
| `[[invocations.entries]]` | `expansion` | `String` | `""` | Text injected byte for byte when the trigger fires ([invocations.rs:50-51](https://github.com/BoardPandas/Hark/blob/bcfcc3fef6f02252870fc3f06440d99992818ade/crates/hark-config/src/invocations.rs#L50-L51)) |
| `[[invocations.entries]]` | `scope` | enum | `"utterance"` | `utterance` (whole dictation) \| `anywhere` (spliced inline) ([invocations.rs:20-27](https://github.com/BoardPandas/Hark/blob/bcfcc3fef6f02252870fc3f06440d99992818ade/crates/hark-config/src/invocations.rs#L20-L27)) |

`[meeting]` is additive: it was added after `[local_stt]` and before `[[invocations.entries]]`, and a config file written before it existed loads unchanged, filling every key from `Meeting::default()` via `#[serde(default)]` ([meeting.rs](../../crates/hark-config/src/meeting.rs), [lib.rs](../../crates/hark-config/src/lib.rs)). An out-of-range `audio_cap_mb` is clamped in place with a logged warning rather than rejected outright, on the same "typo, not a reason to refuse to start" reasoning `Settings::from_toml` already applies to the spellbook migration ([meeting.rs](../../crates/hark-config/src/meeting.rs), [lib.rs](../../crates/hark-config/src/lib.rs)).

`invocations` is the last field on `Settings` deliberately: it is the only section holding a TOML array-of-tables, and those must follow every scalar key ([lib.rs](../../crates/hark-config/src/lib.rs)). It is also the one section with **no `validate` rule**: rejecting a malformed entry would make a hand-edited config unloadable and leave the user no UI to repair it, so unusable entries are skipped at pipeline start instead (see [Invocations](../features/INVOCATIONS.md)) ([invocations.rs:1-11](https://github.com/BoardPandas/Hark/blob/bcfcc3fef6f02252870fc3f06440d99992818ade/crates/hark-config/src/invocations.rs#L1-L11)).

`Provider::resolved_base_url` and `resolved_model` fill in the per-kind defaults when the file leaves them unset ([lib.rs](../../crates/hark-config/src/lib.rs)):

```rust
// crates/hark-config/src/lib.rs:110-126
pub fn resolved_model(&self) -> String {
    if let Some(model) = &self.model {
        return model.clone();
    }
    match self.kind {
        ProviderKind::Deepgram => "nova-3",
        ProviderKind::Openai => "gpt-transcribe",
        ProviderKind::Gemini => "gemini-3.5-transcribe-live",
        ProviderKind::Groq => "whisper-large-v3-turbo",
        // Validation guarantees openai-compatible configs are explicit;
        // an empty model is still a valid request for some servers.
        ProviderKind::OpenaiCompatible => "",
    }
    .to_string()
}
```

`Settings::validate` (run on every load and every save) enforces the cross-field rules: `openai-compatible` requires an explicit `base_url`, `ptt_key` must not be blank, and both `history` caps must be >= 1 ([lib.rs](../../crates/hark-config/src/lib.rs)). The annotated `default-config.toml` mirrors this table and ships as the on-disk documentation for a fresh install ([default-config.toml:13-47](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L13-L47)).

Sources: [lib.rs](../../crates/hark-config/src/lib.rs), [lib.rs](../../crates/hark-config/src/lib.rs), [default-config.toml:1-48](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L1-L48), [crates/hark-config/src/meeting.rs](../../crates/hark-config/src/meeting.rs), [config/default-config.toml:82-119](../../config/default-config.toml#L82-L119)
### Meeting Shortcut Migration

Schema 4 adds `meeting.toggle_key`, defaulting to `None`. Schema 5 adds `gemini_model` and the explicit Gemini choice. Migration from either schema 3 or 4 preserves `none`/`deepgram`, an explicit model/binding, and deliberate auto-stop values; it never opts into Gemini automatically. `Settings::load` writes a versioned backup before persisting a migrated file; for a v3 file this is `config.toml.v3.bak`. The v3 → v4 migration preserves final-pass selection and deliberate auto-stop settings, including 60 seconds. An explicit shortcut already present in an older file is retained ([migration and backup tests](../../crates/hark-config/src/meeting.rs), [load/save](../../crates/hark-config/src/lib.rs)).

Schema 6 adds `meeting.echo_cancellation`, defaulting to `false` when absent.
Migration preserves an explicit value and existing meeting preferences; it does
not enable echo processing automatically. Saving a change affects the next
meeting, so the active recording keeps one processing configuration throughout.
See [speaker echo reduction](../features/MEETINGS.md#reduce-speaker-echo) for the
capture scope and fallback behavior ([settings and migration](../../crates/hark-config/src/meeting.rs)).

`validate_meeting_shortcut` parses both chords through `hark-hotkey` and rejects invalid names, empty configured strings, equal key sets, and either-direction subsets. For the default push-to-talk `LCtrl+LWin`, `LWin+LCtrl`, `LCtrl`, and `LCtrl+LWin+M` conflict; `LCtrl+F11` does not. The settings UI converts blank text to `None`, shows validation errors immediately, and restarts the shared listener after a valid save. This version adds no final-pass provider or model setting ([validation](../../crates/hark-config/src/lib.rs), [settings](../../crates/hark-app/src/ui/settings/meetings.rs)).
<!-- END:AUTOGEN hark_04_configuration_schema -->

---

<!-- BEGIN:AUTOGEN hark_04_configuration_voices -->
## Voice Presets Config

The `[voice]` section picks which preset rewrites the transcript before injection, and optionally overrides the cleanup provider. Cleanup is fail-open by design: resolution never hard-errors, so the default config (Deepgram STT + Clean voice) keeps working even though it has no chat-capable provider to inherit from ([voice.rs:1-7](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L1-L7)).

| Field | Type | Default | Meaning |
|---|---|---|---|
| `default` | `VoiceName` | `Clean` | `verbatim` \| `clean` \| `professional` \| `casual` \| `custom` ([voice.rs:16-22](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L16-L22), [voice.rs:56-64](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L56-L64)) |
| `custom_prompt` | `String` | `""` | System prompt for `default = "custom"`; validated non-empty in that case ([voice.rs:44-46](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L44-L46)) |
| `skip_below_words` | `u32` | `5` | Corrected transcripts shorter than this skip the cleanup call; `0` disables the gate ([voice.rs:47-49](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L47-L49)) |
| `provider` | `Option<VoiceProvider>` | `None` | Explicit cleanup provider table; omit to inherit from an openai/groq STT provider ([voice.rs:50-53](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L50-L53)) |

The optional `[voice.provider]` table shares the same shape idea as `[provider]` but is chat-oriented: Deepgram has no chat-completions product and is rejected at validation ([voice.rs:69-93](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L69-L93), [voice.rs:160-167](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L160-L167)).

```rust
// crates/hark-config/src/voice.rs:69-93
pub struct VoiceProvider {
    pub kind: ProviderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_account: Option<String>,
}
```

`resolve_cleanup_provider` decides, in order: an effective `Verbatim` voice needs no provider; an explicit `[voice.provider]` wins outright; absent that, an `openai`/`groq` STT provider is inherited (same kind, same base URL, the kind's default chat model, and the already-resolved STT key is reused rather than reading the keychain again); otherwise the pipeline degrades to Verbatim with a logged warning ([voice.rs:214-225](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L214-L225)). An `openai-compatible` STT endpoint is deliberately **not** inherited, since speaking `/audio/transcriptions` does not imply `/chat/completions` ([voice.rs:271-280](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L271-L280)).

Sources: [voice.rs:1-152](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L1-L152), [voice.rs:214-282](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-config/src/voice.rs#L214-L282)
<!-- END:AUTOGEN hark_04_configuration_voices -->

---

<!-- BEGIN:AUTOGEN hark_04_configuration_loadsave -->
## Load and Save

`Settings::load` uses defaults for a missing file and returns other I/O errors. Existing files pass through `from_toml`, migration, and validation. If their schema version is older than `CONFIG_VERSION`, Hark creates the versioned backup and immediately saves the migrated settings. Both writes are best-effort: a failed backup keeps the original file, and a failed save is logged. Migrated settings are still returned in memory; successful in-memory migration does not prove persistence ([implementation](../../crates/hark-config/src/lib.rs), [v3 migration fixture](../../crates/hark-config/src/meeting.rs)).

Saving writes a pretty TOML representation through a temporary file and replaces the destination. `validate_meeting_shortcut` also applies to hand-built settings at runtime before installing the listener. The optional meeting shortcut is omitted from serialized TOML when unassigned ([settings](../../crates/hark-config/src/lib.rs), [meeting fields](../../crates/hark-config/src/meeting.rs)).
<!-- END:AUTOGEN hark_04_configuration_loadsave -->

---

<!-- BEGIN:AUTOGEN hark_04_configuration_keychain -->
## Keychain Secret Storage

The `hark-keychain` crate is the only place API key material touches disk, and it goes through the OS credential store via the `keyring` crate rather than any file Hark controls. No type in the crate carries key material, so nothing in it can accidentally `Debug`/`Display` a key ([keychain/lib.rs:1-3](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L1-L3)).

Every key is stored under one fixed service name and an account named after the provider label, so the STT and cleanup roles share one slot per provider by design; `voice.provider.key_account` is the escape hatch for two distinct `openai-compatible` endpoints needing separate slots ([keychain/lib.rs:8-10](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L8-L10), [keychain/lib.rs:24-25](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L24-L25)):

```rust
// crates/hark-keychain/src/lib.rs:18-25
pub const ENV_OVERRIDE: &str = "HARK_STT_KEY";
pub const CLEANUP_ENV_OVERRIDE: &str = "HARK_CLEANUP_KEY";
const KEYRING_SERVICE: &str = "hark";
```

| Function | Behavior |
|---|---|
| `store_key(account, key)` | Trims and rejects empty/whitespace keys before ever touching the backend, so a stray paste can't blank a working slot ([keychain/lib.rs:54-69](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L54-L69)) |
| `delete_key(account)` | Idempotent: deleting an absent key is success, not an error ([keychain/lib.rs:71-76](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L71-L76), [keychain/lib.rs:97-107](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L97-L107)) |
| `key_status(account)` | Non-destructive presence check (`Stored` \| `Missing` \| `Backend(detail)`); the stored value is read and immediately dropped ([keychain/lib.rs:78-88](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L78-L88)) |
| `resolve_key(provider)` | Thin wrapper over `resolve_key_for(ENV_OVERRIDE, provider)` for the STT role ([keychain/lib.rs:119-123](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L119-L123)) |
| `resolve_key_for(env_var, account)` | `env_var` (non-blank) beats the keychain; the keychain is never touched when the env override is present ([keychain/lib.rs:125-131](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L125-L131)) |

`resolve_key_for` is used for both roles: the STT role resolves through `HARK_STT_KEY`, the cleanup role through `HARK_CLEANUP_KEY`, and a missing key on either path produces a `KeyError::Missing` naming both the account and the exact env var to set ([keychain/lib.rs:30-42](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L30-L42)):

```rust
// crates/hark-keychain/src/lib.rs:30-35
#[error(
    "no API key for \"{account}\": set the {env_var} environment variable \
     or store a key in the OS keychain (service \"hark\", account \"{account}\")"
)]
Missing { account: String, env_var: String },
```

Keys are never logged: no `KeyError` variant carries key material, and the crate's test suite specifically asserts that no failure path's `Debug`/`Display` output can ever contain a sentinel key value ([keychain/lib.rs:29-42](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L29-L42)).

Sources: [keychain/lib.rs:1-95](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L1-L95), [keychain/lib.rs:119-169](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L119-L169)
<!-- END:AUTOGEN hark_04_configuration_keychain -->

---

<!-- BEGIN:AUTOGEN hark_04_configuration_locations -->
## File Locations

`default_config_path` and `default_data_dir` resolve the per-OS config file and data directory; both return `None` when the OS gives no home directory (headless CI), and callers fall back to in-memory defaults in that case ([lib.rs](../../crates/hark-config/src/lib.rs)).

| Item | Windows | macOS |
|---|---|---|
| `config.toml` | `%APPDATA%\hark\config.toml` | `~/Library/Application Support/hark/config.toml` ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| Data dir (`hark.db` lives here) | `%APPDATA%\hark` (coincides with the config dir) | `~/Library/Application Support/hark` ([lib.rs](../../crates/hark-config/src/lib.rs)) |
| API key | OS credential store (Credential Manager on Windows, Keychain on macOS); service `"hark"`, account = provider label | same ([keychain/lib.rs:24-25](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L24-L25)) |

The header comment in `default-config.toml` documents the same paths for anyone reading the shipped default file directly ([default-config.toml:1-7](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L1-L7)):

```toml
# config/default-config.toml:1-7
# Hark default settings.
#
# Hark reads its config from the OS config dir:
#   Windows: %APPDATA%\hark\config.toml
#   macOS:   ~/Library/Application Support/hark/config.toml
# Every key is optional; a missing file or missing key uses the defaults
# shown here.
```

Sources: [lib.rs](../../crates/hark-config/src/lib.rs), [keychain/lib.rs:24-25](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-keychain/src/lib.rs#L24-L25), [default-config.toml:1-7](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L1-L7)
<!-- END:AUTOGEN hark_04_configuration_locations -->

---
