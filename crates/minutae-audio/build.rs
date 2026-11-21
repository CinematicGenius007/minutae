fn main() {
    // Process taps (AudioHardwareCreateProcessTap, 14.2+) must be weak imports so binaries
    // still launch on macOS 13; they are only called after a runtime version check.
    // Applies to this crate's tests/examples; the app sets the same flag in its build.rs.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
    }
}
