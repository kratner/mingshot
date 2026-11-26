fn main() {
    // MingShot runs on macOS 10.14 Mojave and later (src/compat.rs). ScreenCaptureKit only exists from macOS 12.3,
    // so it's weak-linked: the app still launches without it, and compat.rs only calls it where it's present.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,ScreenCaptureKit");
    }
    tauri_build::build()
}
