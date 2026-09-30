fn main() {
    println!("cargo:rerun-if-changed=src/core_audio_mac.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    // Otherwise clang uses the installed SDK's minimum (e.g. 27.0), even
    // while rustc advertises an older deployment target for the final binary.
    let deployment =
        std::env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "14.2".to_owned());
    cc::Build::new()
        .file("src/core_audio_mac.m")
        .flag(format!("-mmacosx-version-min={deployment}"))
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .flag("-Werror=unguarded-availability")
        .compile("hark_core_audio");
    for framework in ["Foundation", "CoreAudio", "CoreGraphics"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
