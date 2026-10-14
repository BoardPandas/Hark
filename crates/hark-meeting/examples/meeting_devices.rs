//! Hand check for meeting device choice on a real machine (Windows; Linux
//! and macOS print empty endpoint lists): the default outputs, which apps
//! record from which microphone, where each app plays, what Hark would record
//! as "Me" and "Them" for each meeting app holding the mic, and the level
//! each of those paths hears. Prints names, ids, PIDs and dBFS only; never
//! samples, never window titles.
//!
//! ```text
//! cargo run -p hark-meeting --example meeting_devices -- [--secs 3] [--no-levels]
//! ```
//!
//! Run it during a call. Expect "Me" to be the microphone the call app uses
//! (not necessarily the communications default), and "Them" to be endpoint
//! loopback when the call plays to an output other than the console default.
//! With a call running, the chosen paths should read well above -85 dBFS
//! while people talk; process loopback is printed beside endpoint loopback
//! so a deaf one shows (about -96 dBFS is idle).

use hark_audio::window::peak_window_rms;
use hark_audio::{start_process_loopback, LoopbackTarget};
use hark_meeting::detect::{
    capture_endpoint, root_pid, target_exe, them_source, tree_sessions, Endpoint, ThemSource,
    DEFAULT_APPS,
};
use hark_meeting::probe;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args
        .iter()
        .position(|a| a == "--secs")
        .and_then(|i| args.get(i + 1))
        .map(|v| v.parse().expect("--secs N"))
        .unwrap_or(3);
    let levels = !args.iter().any(|a| a == "--no-levels");

    let snapshot = probe::snapshot().expect("capture probe");
    let render = probe::render_snapshot().expect("render probe");
    let procs = probe::processes().expect("process list");
    let exe_of = |pid: u32| {
        procs
            .iter()
            .find(|p| p.pid == pid)
            .map_or("?", |p| p.exe.as_str())
    };
    let label = |e: &Option<Endpoint>| {
        e.as_ref()
            .map_or("none".to_string(), |e| format!("{} {}", e.name, e.id))
    };

    println!(
        "output default (console):        {}",
        label(&render.console_default)
    );
    println!(
        "output default (communications): {}",
        label(&render.communications_default)
    );
    println!(
        "input default (communications):  {}",
        hark_audio::communications_default_device().unwrap_or_else(|| "none".into())
    );
    let inputs = hark_audio::list_input_devices();
    println!("inputs Hark can open by name:    {inputs:?}");

    println!("output sessions:");
    let mut endpoints: Vec<&Endpoint> = Vec::new();
    for s in &render.sessions {
        if !endpoints.iter().any(|e| e.id == s.endpoint.id) {
            endpoints.push(&s.endpoint);
        }
    }
    for endpoint in &endpoints {
        println!("  {} {}", endpoint.name, endpoint.id);
        for s in render
            .sessions
            .iter()
            .filter(|s| s.endpoint.id == endpoint.id)
        {
            let state = if s.active { "playing" } else { "idle" };
            println!("    pid {:>6} {:<24} {state}", s.pid, exe_of(s.pid));
        }
    }

    println!("apps recording:");
    for user in snapshot.users.iter().filter(|u| u.in_use) {
        let names: Vec<&str> = user
            .capture_endpoints
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        println!("  {:<28} {names:?}", user.app.id());
    }

    let meeting_apps: Vec<String> = snapshot
        .users
        .iter()
        .filter(|u| u.in_use)
        .map(|u| u.app.id())
        .filter(|id| DEFAULT_APPS.iter().any(|a| a.eq_ignore_ascii_case(id)))
        .collect();
    if meeting_apps.is_empty() {
        println!("no meeting app holds the mic; start a call and run again");
    }
    for app in meeting_apps {
        let exe = target_exe(&app);
        let Some(root) = root_pid(&procs, &exe) else {
            println!("{app}: no running {exe}");
            continue;
        };
        println!("{app}: root pid {root} ({exe})");
        for s in tree_sessions(&render, &procs, root) {
            let state = if s.active { "playing" } else { "idle" };
            println!(
                "  plays from pid {} to {} ({state})",
                s.pid, s.endpoint.name
            );
        }
        let mic = capture_endpoint(&snapshot, &app).map(|e| e.name.clone());
        match &mic {
            Some(name) if inputs.contains(name) => println!("  Me:   {name}"),
            Some(name) => {
                println!("  Me:   {name} (NOT openable by that name: Hark would fall back)")
            }
            None => println!("  Me:   communications default (the app's mic is unknown)"),
        }
        let them = them_source(&render, &procs, root);
        match &them {
            ThemSource::Endpoint(e) => println!("  Them: endpoint loopback of {}", e.name),
            ThemSource::ProcessTree => println!("  Them: process loopback of pid {root}"),
        }
        if !levels {
            continue;
        }
        if let Some(name) = mic.filter(|n| inputs.contains(n)) {
            println!("  level, Me ({name}): {}", mic_level(&name, secs));
        }
        if let ThemSource::Endpoint(e) = them {
            println!(
                "  level, endpoint loopback of {}: {}",
                e.name,
                loopback_level(LoopbackTarget::Endpoint { id: e.id }, secs)
            );
        }
        println!(
            "  level, process loopback of pid {root}: {}",
            loopback_level(LoopbackTarget::IncludeTree(root), secs)
        );
    }
}

fn db(x: f32) -> f32 {
    if x <= 1e-6 {
        -120.0
    } else {
        20.0 * x.log10()
    }
}

/// The loudest 100 ms over `secs` of this microphone, opened by name.
fn mic_level(name: &str, secs: u64) -> String {
    match hark_audio::start(10, Some(name.to_string())) {
        Ok((handle, consumer)) => {
            std::thread::sleep(Duration::from_secs(secs));
            let total = consumer.total_written();
            let samples = consumer
                .read_range(consumer.oldest_available(), total)
                .unwrap_or_default();
            format!(
                "{:.1} dBFS over {} frames",
                db(peak_window_rms(&samples, handle.sample_rate(), 100)),
                samples.len()
            )
        }
        Err(e) => format!("cannot open ({e})"),
    }
}

/// The loudest 100 ms over `secs` of one loopback target.
fn loopback_level(target: LoopbackTarget, secs: u64) -> String {
    match start_process_loopback(target, 10) {
        Ok((handle, consumer)) => {
            std::thread::sleep(Duration::from_secs(secs));
            let total = consumer.total_written();
            let samples = consumer
                .read_range(consumer.oldest_available(), total)
                .unwrap_or_default();
            format!(
                "{:.1} dBFS over {} frames",
                db(peak_window_rms(&samples, handle.sample_rate(), 100)),
                samples.len()
            )
        }
        Err(e) => format!("cannot start ({e})"),
    }
}
