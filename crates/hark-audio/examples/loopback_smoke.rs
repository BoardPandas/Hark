//! Hand check for per-process loopback (`src/loopback_win.rs`) on real
//! Windows or macOS 14.2+: `cargo test` never opens an audio device. Captures for a few
//! seconds and prints, per second, the frames delivered and their level in
//! dBFS. Never prints or saves samples.
//!
//! ```text
//! cargo run -p hark-audio --example loopback_smoke -- [--secs 5] [--include PID]
//! ```
//!
//! Default is exclude mode on this process (everything but the smoke binary),
//! which is what a manual meeting start does. Expect sample_rate() frames every
//! second (16 kHz on Windows, device rate on macOS), even while nothing plays (continuous packets; endpoint loopback
//! would deliver none), and no discontinuities after the first.

use hark_audio::window::{peak_window_rms, rms};
use hark_audio::{start_process_loopback, LoopbackTarget};
use std::sync::atomic::Ordering;
use std::time::Duration;

fn db(x: f32) -> f32 {
    if x <= 1e-6 {
        -120.0
    } else {
        20.0 * x.log10()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };
    let secs: u64 = value("--secs")
        .map(|v| v.parse().expect("--secs N"))
        .unwrap_or(5);
    let target = match value("--include") {
        Some(pid) => LoopbackTarget::IncludeTree(pid.parse().expect("--include PID")),
        None => LoopbackTarget::ExcludeTree(std::process::id()),
    };

    let (handle, consumer) = match start_process_loopback(target, 10) {
        Ok(started) => started,
        Err(e) => {
            eprintln!("loopback failed: {e}");
            std::process::exit(1);
        }
    };
    println!("{target:?} at {} Hz", handle.sample_rate());

    let mut read = 0;
    for sec in 1..=secs {
        std::thread::sleep(Duration::from_secs(1));
        let written = consumer.total_written();
        let samples = consumer
            .read_range(read, written)
            .expect("a 10 s ring outlasts a 1 s drain");
        read = written;
        println!(
            "{sec:>3} s  frames {:>6}  rms {:>6.1} dBFS  loudest 100 ms {:>6.1} dBFS",
            samples.len(),
            db(rms(&samples)),
            db(peak_window_rms(&samples, handle.sample_rate(), 100)),
        );
    }
    println!(
        "start host timestamp {}, discontinuities {}, stream errored {}",
        if handle.start_qpc_ns().is_some() {
            "recorded"
        } else {
            "MISSING"
        },
        handle.discontinuities().load(Ordering::Relaxed),
        handle.stream_errored(),
    );
}
