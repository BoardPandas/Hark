fn main() {
    println!("cargo:rerun-if-changed=src/macos/native.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let minimum = std::env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "14.2".into());
        cc::Build::new()
            .file("src/macos/native.m")
            .flag(format!("-mmacosx-version-min={minimum}"))
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("hark_app_macos");
        for framework in [
            "AppKit",
            "AVFoundation",
            "ApplicationServices",
            "UniformTypeIdentifiers",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
