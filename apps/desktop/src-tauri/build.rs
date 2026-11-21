fn main() {
    // minimumSystemVersion is 13.0, but Core Audio process taps (AudioHardwareCreateProcessTap)
    // exist only from 14.2. Weak-link CoreAudio so the app launches on 13.x; minutae-audio
    // only calls tap APIs after checking the OS version (ScreenCaptureKit is used before 14.4).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
    }
    tauri_build::build()
}
