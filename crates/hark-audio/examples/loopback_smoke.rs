//! Hand check for system-audio loopback (`src/loopback/`) on real hardware:
//! WASAPI process or endpoint loopback on Windows, PipeWire on Linux, the
//! process tap on macOS 14.2+.
//! `cargo test` never opens an audio device. Captures for a few seconds and
//! prints, per second, the frames delivered and their level in dBFS. Never
//! prints or saves samples.
//!
//! On a headless Linux box a private stack with a null sink is enough:
//! ```text
//! mkdir -p /tmp/pwrt
//! PIPEWIRE_RUNTIME_DIR=/tmp/pwrt pipewire &
//! PIPEWIRE_RUNTIME_DIR=/tmp/pwrt wireplumber &
//! # then create a sink (any client will do) and run with the same runtime dir
//! ```
//!
//! ```text
//! cargo run -p hark-audio --example loopback_smoke -- [--secs 5] [--include PID | --endpoint NAME]
//! ```
//!
//! Default is exclude mode on this process (everything but the smoke binary),
//! which is what a manual meeting start does. Expect sample_rate() frames every
//! second (16 kHz on Windows, device rate on macOS), even while nothing plays
//! (process loopback sends packets continuously), and no discontinuities
//! after the first.
//!
//! `--endpoint` (Windows) records one output device, matched by a piece of
//! its name ("Chat-Audeze") or by its endpoint id. Endpoint loopback sends no
//! packets while nothing plays; Hark places that silence itself, so expect
//! 16 000 frames a second there too (the first second or so runs 300 ms
//! short: silence is placed that far behind the clock). Start and stop some
//! audio on the device while it runs: the level moves, the frame count does
//! not.

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
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };
    let secs: u64 = value("--secs")
        .map(|v| v.parse().expect("--secs N"))
        .unwrap_or(5);
    let target = match (value("--include"), value("--endpoint")) {
        (Some(pid), _) => LoopbackTarget::IncludeTree(pid.parse().expect("--include PID")),
        (None, Some(name)) => LoopbackTarget::Endpoint {
            id: endpoint_id(name).unwrap_or_else(|| {
                eprintln!("no active output device matches {name:?}");
                std::process::exit(1);
            }),
        },
        (None, None) => LoopbackTarget::ExcludeTree(std::process::id()),
    };

    let (handle, consumer) = match start_process_loopback(target.clone(), 10) {
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

/// The id of the active output endpoint whose friendly name contains `name`
/// (any case), or `name` itself when it is already an endpoint id. Hark
/// takes ids from the meeting probe; this lookup is the hand tool's own.
#[cfg(windows)]
fn endpoint_id(name: &str) -> Option<String> {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
        STGM_READ,
    };

    if name.starts_with('{') {
        return Some(name.to_string());
    }
    let wanted = name.to_lowercase();
    // SAFETY: COM calls on this thread, which keeps its apartment for the
    // rest of the process; GetId's string is copied before it is freed.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let devices = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .ok()?;
        for index in 0..devices.GetCount().ok()? {
            let Ok(device) = devices.Item(index) else {
                continue;
            };
            let Ok(friendly) = device
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
            else {
                continue;
            };
            if !friendly.to_string().to_lowercase().contains(&wanted) {
                continue;
            }
            let raw = device.GetId().ok()?;
            let id = String::from_utf16_lossy(raw.as_wide());
            CoTaskMemFree(Some(raw.0 as *const _));
            println!("endpoint: {friendly} = {id}");
            return Some(id);
        }
    }
    None
}

#[cfg(not(windows))]
fn endpoint_id(_name: &str) -> Option<String> {
    eprintln!("--endpoint is Windows only");
    None
}
