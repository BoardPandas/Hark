use super::*;
use crate::route::LIVE_BUILD;
use hark_inject::Strategy;
use hark_stt::ProviderConfig;

fn settings_from(toml: &str) -> Settings {
    Settings::from_toml(toml).unwrap()
}

/// A thread wired the way `run` wires the worker, held inside its "dictation"
/// until `release` fires.
fn worker_blocked_until(
    release: mpsc::Receiver<()>,
) -> (std::thread::JoinHandle<()>, mpsc::Receiver<()>) {
    let (done_tx, done) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _done: mpsc::Sender<()> = done_tx;
        let _ = release.recv();
    });
    (handle, done)
}

#[test]
fn a_worker_that_has_returned_is_joined() {
    let (release_tx, release) = mpsc::channel();
    let (worker, done) = worker_blocked_until(release);
    release_tx.send(()).unwrap();
    assert!(join_within(worker, &done, Duration::from_secs(5)));
}

#[test]
fn a_worker_stuck_in_a_dictation_is_not_waited_on_past_the_grace() {
    // The freeze this replaced: shutdown joined a worker sitting out a
    // stalled provider, and the UI thread sat there with it.
    let (release_tx, release) = mpsc::channel();
    let (worker, done) = worker_blocked_until(release);
    let started = std::time::Instant::now();
    assert!(!join_within(worker, &done, Duration::from_millis(50)));
    assert!(started.elapsed() < Duration::from_secs(2));
    // Let the abandoned thread finish rather than leak it into other tests.
    release_tx.send(()).unwrap();
}

#[test]
fn a_worker_that_panicked_still_counts_as_finished() {
    let (done_tx, done) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _done: mpsc::Sender<()> = done_tx;
        panic!("dictation blew up");
    });
    assert!(join_within(worker, &done, Duration::from_secs(5)));
}

#[test]
fn default_settings_map_to_deepgram_nova3() {
    let cfg = provider_config(&settings_from(""), "KEY".to_string()).unwrap();
    assert_eq!(cfg.kind, hark_stt::ProviderKind::Deepgram);
    assert_eq!(cfg.label, "deepgram");
    assert_eq!(cfg.base_url, "https://api.deepgram.com");
    assert_eq!(cfg.model, "nova-3");
    assert_eq!(cfg.api_key, "KEY");
    assert!(cfg.bias_terms.is_empty());
}

#[test]
fn whisper_family_models_share_the_openai_compatible_adapter() {
    // A custom Whisper server, and OpenAI pinned back to a Whisper-family
    // model: both speak the single-`prompt` contract.
    for (kind, url, model) in [
        (
            "openai-compatible",
            "http://localhost:8000/v1",
            "whisper-large-v3-turbo",
        ),
        ("openai", "https://api.openai.com/v1", "whisper-1"),
    ] {
        let cfg = provider_config(
            &settings_from(&format!(
                "[provider]\nkind = \"{kind}\"\nmodel = \"{model}\"\nbase_url = \"{url}\""
            )),
            "K".to_string(),
        )
        .unwrap();
        assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiCompatible);
        assert_eq!(cfg.label, kind);
        assert_eq!(cfg.base_url, url);
        assert_eq!(cfg.model, model);
    }
}

#[test]
fn a_migrated_groq_config_still_reaches_groq_through_the_compatible_adapter() {
    // Schema 7 retired the preset; the endpoint it named must not move.
    let cfg = provider_config(
        &settings_from("version = 6\n[provider]\nkind = \"groq\""),
        "K".to_string(),
    )
    .unwrap();
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiCompatible);
    assert_eq!(cfg.label, "openai-compatible");
    assert_eq!(cfg.base_url, "https://api.groq.com/openai/v1");
    assert_eq!(cfg.model, "whisper-large-v3-turbo");
}

#[test]
fn gemini_routes_to_the_live_adapter_and_carries_the_mode() {
    let cfg = provider_config(
        &settings_from("[provider]\nkind = \"gemini\""),
        "K".to_string(),
    )
    .unwrap();
    assert_eq!(cfg.kind, hark_stt::ProviderKind::GeminiLive);
    assert_eq!(cfg.model, "gemini-3.5-transcribe-live");
    assert_eq!(
        cfg.live_mode,
        hark_stt::gemini_live::TranscribeMode::Verbatim
    );

    let smart = provider_config(
        &settings_from("[provider]\nkind = \"gemini\"\nlive_mode = \"smart\""),
        "K".to_string(),
    )
    .unwrap();
    assert_eq!(
        smart.live_mode,
        hark_stt::gemini_live::TranscribeMode::Smart
    );
}

#[test]
fn a_smart_stream_falls_back_to_a_verbatim_replay() {
    // SMART strands a turn on "scratch that"; replaying the same clip in
    // SMART hung again. Only the mode may differ -- the fallback must
    // still reach the same provider, model and key.
    let settings = settings_from("[provider]\nkind = \"gemini\"\nlive_mode = \"smart\"");
    let smart = provider_config(&settings, "K".to_string()).unwrap();
    let fallback = fallback_config(&settings.provider, &smart);
    assert_eq!(
        fallback.live_mode,
        hark_stt::gemini_live::TranscribeMode::Verbatim
    );
    assert_eq!(fallback.kind, smart.kind);
    assert_eq!(fallback.model, smart.model);
    assert_eq!(fallback.api_key, smart.api_key);
}

#[test]
fn the_openai_default_routes_to_the_gpt_transcribe_adapter() {
    let cfg = provider_config(
        &settings_from("[provider]\nkind = \"openai\""),
        "K".to_string(),
    )
    .unwrap();
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiTranscribe);
    assert_eq!(cfg.label, "openai");
    assert_eq!(cfg.base_url, "https://api.openai.com/v1");
    assert_eq!(cfg.model, "gpt-transcribe");
}

#[test]
fn a_gateway_proxying_gpt_transcribe_gets_the_same_contract() {
    // The contract follows the model, not the vendor: an openai-compatible
    // base_url pointed at a gpt-transcribe proxy must not fall back to the
    // Whisper `prompt` packing.
    let cfg = provider_config(
        &settings_from(
            "[provider]\nkind = \"openai-compatible\"\n\
             base_url = \"https://gateway.example/v1\"\nmodel = \"gpt-transcribe\"",
        ),
        "K".to_string(),
    )
    .unwrap();
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiTranscribe);
}

// --- OpenAI Live transcribe. The contract with hark-stt's adapter: kind
// OpenAiLive, label "openai", the OpenAI HTTPS base (the adapter derives
// wss:// from it), the live model, the OpenAI key, the spellbook terms. ---

fn openai_live(extra: &str) -> ProviderConfig {
    provider_config(
        &settings_from(&format!(
            "[provider]\nkind = \"openai\"\n{extra}\n[spellbook]\nterms = [\"Hark\"]"
        )),
        "OPENAI-KEY".to_string(),
    )
    .unwrap()
}

#[cfg(feature = "live")]
#[test]
fn openai_with_live_on_routes_to_the_realtime_adapter() {
    let cfg = openai_live("live = true");
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiLive);
    assert_eq!(cfg.label, "openai");
    assert_eq!(cfg.base_url, "https://api.openai.com/v1");
    assert_eq!(cfg.model, "gpt-live-transcribe");
    assert_eq!(cfg.api_key, "OPENAI-KEY");
    assert_eq!(cfg.bias_terms, vec!["Hark"]);
}

#[cfg(feature = "live")]
#[test]
fn live_keeps_a_custom_openai_base_url_for_the_adapter_to_derive_from() {
    let cfg = openai_live("live = true\nbase_url = \"https://proxy.example/v1\"");
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiLive);
    assert_eq!(cfg.base_url, "https://proxy.example/v1");
}

#[cfg(feature = "live")]
#[test]
fn a_pinned_batch_model_never_reaches_the_realtime_adapter() {
    let cfg = openai_live("live = true\nmodel = \"whisper-1\"");
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiLive);
    assert_eq!(cfg.model, "gpt-live-transcribe");
}

#[cfg(feature = "live")]
#[test]
fn a_hand_edited_gpt_live_model_routes_to_the_realtime_adapter() {
    // Replaces the old "never reaches an adapter" pin: the multipart
    // endpoint still rejects this model, so it must go to the WebSocket
    // one, toggle or no toggle, and a dated snapshot is sent as pinned.
    let cfg = openai_live("model = \"gpt-live-transcribe\"");
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiLive);
    assert_eq!(cfg.model, "gpt-live-transcribe");
    let dated = openai_live("model = \"gpt-live-transcribe-2026-10-01\"");
    assert_eq!(dated.kind, hark_stt::ProviderKind::OpenAiLive);
    assert_eq!(dated.model, "gpt-live-transcribe-2026-10-01");
}

#[cfg(not(feature = "live"))]
#[test]
fn without_the_websocket_client_live_falls_back_to_batch() {
    // Failing every dictation would be worse than the slower path.
    for extra in ["live = true", "model = \"gpt-live-transcribe\""] {
        let cfg = openai_live(extra);
        assert_eq!(
            cfg.kind,
            hark_stt::ProviderKind::OpenAiTranscribe,
            "{extra}"
        );
        assert_eq!(cfg.model, "gpt-transcribe", "{extra}");
    }
    // A pinned batch model is what "off" would run, so that is what runs.
    let pinned = openai_live("live = true\nmodel = \"whisper-1\"");
    assert_eq!(pinned.kind, hark_stt::ProviderKind::OpenAiCompatible);
    assert_eq!(pinned.model, "whisper-1");
}

#[test]
fn openai_with_live_off_is_still_batch_gpt_transcribe() {
    let cfg = openai_live("live = false");
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiTranscribe);
    assert_eq!(cfg.model, "gpt-transcribe");
    let pinned = openai_live("model = \"whisper-1\"");
    assert_eq!(pinned.kind, hark_stt::ProviderKind::OpenAiCompatible);
    assert_eq!(pinned.model, "whisper-1");
}

#[test]
fn live_on_any_other_provider_is_ignored() {
    let deepgram = provider_config(
        &settings_from("[provider]\nkind = \"deepgram\"\nlive = true"),
        "K".to_string(),
    )
    .unwrap();
    assert_eq!(deepgram.kind, hark_stt::ProviderKind::Deepgram);
    assert_eq!(deepgram.model, "nova-3");
    let gemini = provider_config(
        &settings_from("[provider]\nkind = \"gemini\"\nlive = true"),
        "K".to_string(),
    )
    .unwrap();
    assert_eq!(gemini.kind, hark_stt::ProviderKind::GeminiLive);
    assert_eq!(gemini.model, "gemini-3.5-transcribe-live");
}

#[test]
fn the_route_names_the_model_the_dictation_record_will_carry() {
    // `run` hands provider_cfg.model to the worker as `stream_model` and
    // the fallback's as `batch_model`; history and stats record whichever
    // path produced the text. The footer reads the same route.
    let s = settings_from("[provider]\nkind = \"openai\"\nlive = true");
    let cfg = provider_config(&s, "K".to_string()).unwrap();
    assert_eq!(stt_route(&s.provider).1, cfg.model);
    let expected = if LIVE_BUILD {
        "gpt-live-transcribe"
    } else {
        "gpt-transcribe"
    };
    assert_eq!(cfg.model, expected);
    assert_eq!(fallback_config(&s.provider, &cfg).model, "gpt-transcribe");
}

// --- The batch route: the dictation fallback and meeting chunks. ---

#[cfg(feature = "live")]
#[test]
fn with_live_on_the_stream_is_realtime_and_the_fallback_is_rest_gpt_transcribe() {
    // A fallback over the same realtime socket lost the dictation to
    // whatever had just broken the stream: a proxy refusing WebSocket
    // upgrades, a realtime outage, a rejected spellbook term.
    let s = settings_from(
        "[provider]\nkind = \"openai\"\nlive = true\n[spellbook]\nterms = [\"Hark\"]",
    );
    let live = provider_config(&s, "OPENAI-KEY".to_string()).unwrap();
    assert_eq!(live.kind, hark_stt::ProviderKind::OpenAiLive);
    assert_eq!(live.model, "gpt-live-transcribe");

    let fallback = fallback_config(&s.provider, &live);
    assert_eq!(fallback.kind, hark_stt::ProviderKind::OpenAiTranscribe);
    assert_eq!(fallback.model, "gpt-transcribe");
    // Same provider, same key, same endpoint host: nothing to re-enter.
    assert_eq!(fallback.label, "openai");
    assert_eq!(fallback.base_url, "https://api.openai.com/v1");
    assert_eq!(fallback.api_key, "OPENAI-KEY");
    assert_eq!(fallback.bias_terms, vec!["Hark"]);

    // The streaming adapter is still built from the live config.
    let asked = std::cell::Cell::new(false);
    live_adapter(s.local_stt.mode, &live, |c| {
        assert_eq!(c.kind, hark_stt::ProviderKind::OpenAiLive);
        assert_eq!(c.model, "gpt-live-transcribe");
        asked.set(true);
        None
    });
    assert!(asked.get());
}

#[cfg(feature = "live")]
#[test]
fn the_fallback_honours_a_pinned_batch_model_and_never_a_live_one() {
    let pinned = settings_from("[provider]\nkind = \"openai\"\nlive = true\nmodel = \"whisper-1\"");
    let cfg = provider_config(&pinned, "K".to_string()).unwrap();
    let fallback = fallback_config(&pinned.provider, &cfg);
    assert_eq!(fallback.kind, hark_stt::ProviderKind::OpenAiCompatible);
    assert_eq!(fallback.model, "whisper-1");

    // Live through a hand-edited model: the fallback is the default batch.
    let edited = settings_from("[provider]\nkind = \"openai\"\nmodel = \"gpt-live-transcribe\"");
    let cfg = provider_config(&edited, "K".to_string()).unwrap();
    assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiLive);
    let fallback = fallback_config(&edited.provider, &cfg);
    assert_eq!(fallback.kind, hark_stt::ProviderKind::OpenAiTranscribe);
    assert_eq!(fallback.model, "gpt-transcribe");
}

#[test]
fn without_live_the_fallback_is_the_configured_route() {
    for toml in [
        "",
        "[provider]\nkind = \"openai\"",
        "[provider]\nkind = \"openai\"\nlive = false\nmodel = \"whisper-1\"",
        "[provider]\nkind = \"gemini\"",
        "[provider]\nkind = \"openai-compatible\"\nbase_url = \"http://localhost:8000/v1\"\nmodel = \"whisper-large-v3\"",
    ] {
        let s = settings_from(toml);
        let cfg = provider_config(&s, "K".to_string()).unwrap();
        let fallback = fallback_config(&s.provider, &cfg);
        assert_eq!(fallback.kind, cfg.kind, "{toml:?}");
        assert_eq!(fallback.model, cfg.model, "{toml:?}");
        assert_eq!(fallback.base_url, cfg.base_url, "{toml:?}");
    }
}

#[test]
fn meeting_chunks_take_the_batch_route_and_ignore_live() {
    // A chunk is a finished clip: streaming it buys no latency and bills
    // the realtime rate, about 3.8x gpt-transcribe.
    for toml in [
        "[provider]\nkind = \"openai\"\nlive = true",
        "[provider]\nkind = \"openai\"\nmodel = \"gpt-live-transcribe\"",
    ] {
        let cfg = meeting_chunk_config(&settings_from(toml), "K".to_string()).unwrap();
        assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiTranscribe, "{toml}");
        assert_eq!(cfg.model, "gpt-transcribe", "{toml}");
        assert_eq!(cfg.label, "openai");
    }
    // Gemini's chunks still go over its own socket, verbatim.
    let gemini = meeting_chunk_config(
        &settings_from("[provider]\nkind = \"gemini\"\nlive_mode = \"smart\""),
        "K".to_string(),
    )
    .unwrap();
    assert_eq!(gemini.kind, hark_stt::ProviderKind::GeminiLive);
    assert_eq!(
        gemini.live_mode,
        hark_stt::gemini_live::TranscribeMode::Verbatim
    );
}

#[test]
fn primary_on_device_mode_never_builds_a_live_session_for_openai_live() {
    let primary = settings_from(
        "[provider]\nkind = \"openai\"\nlive = true\n[local_stt]\nmode = \"primary\"",
    );
    let cfg = provider_config(&primary, "K".to_string()).unwrap();
    let asked = std::cell::Cell::new(false);
    let live = live_adapter(primary.local_stt.mode, &cfg, |_| {
        asked.set(true);
        None
    });
    assert!(live.is_none());
    assert!(!asked.get(), "primary mode must not even ask for a session");

    // The gate is the mode, not the provider: a cloud mode does ask.
    let cloud = settings_from("[provider]\nkind = \"openai\"\nlive = true");
    live_adapter(cloud.local_stt.mode, &cfg, |c| {
        assert_eq!(c.label, "openai");
        asked.set(true);
        None
    });
    assert!(asked.get());
}

#[test]
fn spellbook_terms_feed_provider_bias_terms() {
    let s = settings_from("[spellbook]\nterms = [\"Hark\", \"Levenshtein\"]");
    let cfg = provider_config(&s, "K".to_string()).unwrap();
    assert_eq!(cfg.bias_terms, vec!["Hark", "Levenshtein"]);
}

#[test]
fn audio_settings_map_to_window_params() {
    let s = settings_from("[audio]\npreroll_ms = 200\ntail_ms = 100\nmax_hold_s = 30");
    let w = window_params(&s.audio);
    assert_eq!(w.preroll_ms, 200);
    assert_eq!(w.tail_ms, 100);
    assert_eq!(w.max_hold_s, 30);
    assert_eq!(w.min_speech_ms, 250); // default carried through
}

#[test]
fn inject_settings_map_including_strategy() {
    let s = settings_from("[inject]\nstrategy = \"type\"\nclipboard_retries = 3");
    let i = inject_settings(&s.inject);
    assert_eq!(i.strategy, Strategy::Type);
    assert_eq!(i.clipboard_retries, 3);
    assert_eq!(i.set_paste_delay_ms, 50); // default carried through
}

// --- cleanup plan wiring. Only paths that never read the real OS
// keychain are exercised here: Verbatim, warn-and-degrade, STT key
// reuse, and one Account-path test that the HARK_CLEANUP_KEY env
// override satisfies before the keyring would be touched. ---

fn client() -> reqwest::blocking::Client {
    hark_stt::shared_client().unwrap()
}

#[test]
fn verbatim_voice_never_constructs_a_cleaner() {
    let s = settings_from("[provider]\nkind = \"openai\"\n[voice]\ndefault = \"verbatim\"");
    assert!(build_cleanup(&s, client(), "STT-KEY").is_none());
}

#[test]
fn default_deepgram_config_degrades_to_no_cleaner() {
    // Out-of-the-box config: Deepgram STT + Clean voice. Warn, no plan,
    // and crucially no error: the pipeline must start.
    assert!(build_cleanup(&settings_from(""), client(), "STT-KEY").is_none());
}

#[test]
fn openai_stt_inherits_into_a_cleanup_plan_without_keychain() {
    let s = settings_from(
        "[provider]\nkind = \"openai\"\n[voice]\nskip_below_words = 3\n[spellbook]\nterms = [\"Hark\"]",
    );
    let plan = build_cleanup(&s, client(), "STT-KEY").expect("inherit path builds a plan");
    assert_eq!(plan.voice, hark_voice::Voice::Clean);
    assert_eq!(plan.model, "gpt-5-nano");
    assert_eq!(plan.skip_below_words, 3);
    assert_eq!(plan.cleaner.label(), "openai");
    // Same endpoint as STT: no second pre-warm needed.
    assert_eq!(plan.prewarm_url, None);
}

#[test]
fn explicit_provider_with_env_key_prewarns_the_differing_host() {
    // Deepgram STT + explicit Gemini cleanup: the Account path resolves
    // via HARK_CLEANUP_KEY before any keyring read, and the differing
    // endpoint gets its own pre-warm URL.
    std::env::set_var("HARK_CLEANUP_KEY", "env-test-key");
    let s = settings_from("[voice.provider]\nkind = \"gemini\"");
    let plan = build_cleanup(&s, client(), "STT-KEY");
    std::env::remove_var("HARK_CLEANUP_KEY");
    let plan = plan.expect("env-keyed explicit provider builds a plan");
    assert_eq!(plan.model, "gemini-3.5-flash-lite");
    assert_eq!(plan.cleaner.label(), "gemini");
    assert_eq!(
        plan.prewarm_url.as_deref(),
        Some("https://generativelanguage.googleapis.com/v1beta/openai")
    );
}
