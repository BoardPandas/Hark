---
name: hark-app-test-build
description: On this Windows dev box hark-app tests only link with --no-default-features --features live-stt (GNU toolchain vs MSVC-only sherpa libs)
metadata:
  type: project
---

`cargo test -p hark-app` with default features fails at link time: "could not find native static library `sherpa-onnx-c-api`". Only `stable-x86_64-pc-windows-gnu` is installed, and sherpa-onnx's prebuilt `win-x64-static-MT` libs are MSVC `.lib` files the GNU linker will not pick up. `cargo check` / `cargo clippy` with default features still pass (no link).

**Why:** discovered 2026-11-14 while verifying the held-clip / preload work.

**How to apply:** run hark-app tests as `cargo test -p hark-app --no-default-features --features live-stt`, separately from the other crates (feature unification would turn the engine back on), and run the rest with `cargo test --workspace --exclude hark-app`. Say so in the report rather than claiming the default-feature test run passed. Headless egui tests follow `ctx.run_ui(RawInput{events,..})`; a reusable pointer-drag rig lives in `crates/hark-app/src/ui/selectable.rs` (`rig` module, test-only).
