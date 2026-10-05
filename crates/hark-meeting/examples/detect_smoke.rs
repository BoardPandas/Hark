//! Hand check for the detection probe (`src/probe/`) on a real session
//! (Windows, Linux, macOS 14.2+):
//! polls every 2 s and prints which apps hold the mic, which own a meeting
//! window, the detector's verdicts (Ask mode), and each detected app's
//! loopback root PID. Prints app ids only; window titles are never shown.
//!
//! ```text
//! cargo run -p hark-meeting --example detect_smoke -- [--secs 60]
//! ```
//!
//! Start a Teams/Zoom call or a Meet tab while it runs: expect `Prompt` about
//! 6 s after the app opens the mic, and `Retract` shortly after it closes it.

use hark_meeting::detect::{
    root_pid, target_exe, DetectConfig, DetectMode, Detector, Verdict, DEFAULT_APPS, POLL_MS,
};
use hark_meeting::probe;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args
        .iter()
        .position(|a| a == "--secs")
        .and_then(|i| args.get(i + 1))
        .map(|v| v.parse().expect("--secs N"))
        .unwrap_or(60);
    let self_exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let mut detector = Detector::new(DetectConfig {
        mode: DetectMode::Ask,
        apps: DEFAULT_APPS.iter().map(|s| s.to_string()).collect(),
        auto_stop_after_ms: 60_000,
        self_exe,
    });

    // The change watcher, so the hand check covers how detection actually
    // wakes: graph events on Linux; Windows and macOS only poll.
    let wakes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counter = wakes.clone();
    let watcher = match probe::ChangeWatcher::start(move || {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }) {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("change watcher unavailable ({e}); polling only");
            None
        }
    };

    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        let took = Instant::now();
        let snapshot = match probe::snapshot() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("probe failed: {e}");
                std::process::exit(1);
            }
        };
        let took = took.elapsed();
        let now = t0.elapsed().as_millis() as u64;
        let holding: Vec<String> = snapshot
            .users
            .iter()
            .filter(|u| u.in_use)
            .map(|u| u.app.id())
            .collect();
        let verdict = detector.observe(&snapshot, now);
        println!(
            "{:>6.1}s  read {:>5.1} ms  mic: {holding:?}  meeting windows: {:?}  -> {verdict:?}",
            now as f64 / 1000.0,
            took.as_secs_f64() * 1000.0,
            snapshot.meeting_windows,
        );
        if let Verdict::Prompt(app) | Verdict::Start(app) = &verdict {
            let exe = target_exe(app);
            match probe::processes() {
                Ok(procs) => println!(
                    "        loopback target: {exe} root pid {:?}",
                    root_pid(&procs, &exe)
                ),
                Err(e) => println!("        process list failed: {e}"),
            }
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
    println!(
        "watcher alive: {}, change notifications: {}",
        watcher.as_ref().is_some_and(probe::ChangeWatcher::is_alive),
        wakes.load(std::sync::atomic::Ordering::Relaxed)
    );
}
