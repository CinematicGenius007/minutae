//! Record mic + system audio for N seconds and print levels.
//!
//! `cargo run -p minutae-audio --example record -- 5 [out_dir]`

use std::time::{Duration, Instant};

use minutae_audio::{CaptureConfig, Recorder, list_input_devices};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let out_dir = args
        .next()
        .map(Into::into)
        .unwrap_or_else(|| std::env::temp_dir().join("minutae-record-example"));

    println!("Input devices:");
    for d in list_input_devices()? {
        println!(
            "  {} {} ({})",
            if d.is_default { "*" } else { " " },
            d.name,
            d.id
        );
    }

    let handle = Recorder::new().start(CaptureConfig::new(out_dir))?;
    match handle.system_error() {
        None => println!("System audio: capturing"),
        Some(e) => println!("System audio: NOT captured ({e})"),
    }
    let levels = handle.levels();
    let start = Instant::now();
    let (mut mic_peak, mut sys_peak) = (0f32, 0f32);
    while start.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(250));
        let f = *levels.borrow();
        mic_peak = mic_peak.max(f.mic_rms);
        sys_peak = sys_peak.max(f.system_rms);
        println!(
            "{:>6} ms  mic {:.4} {:<20}  system {:.4} {}",
            f.elapsed_ms,
            f.mic_rms,
            bar(f.mic_rms),
            f.system_rms,
            bar(f.system_rms)
        );
    }
    let tracks = handle.stop()?;
    if let Some(e) = handle.last_stream_error() {
        println!("Last stream error: {e}");
    }
    println!("Peak RMS: mic {mic_peak:.4}, system {sys_peak:.4}");
    println!("Duration: {} ms", tracks.duration_ms);
    println!("mic:    {}", tracks.mic.display());
    match &tracks.system {
        Some(p) => println!("system: {}", p.display()),
        None => println!("system: (none)"),
    }
    Ok(())
}

fn bar(rms: f32) -> String {
    // Map -60..0 dBFS to 0..20 chars.
    let db = 20.0 * rms.max(1e-6).log10();
    let n = (((db + 60.0) / 60.0).clamp(0.0, 1.0) * 20.0) as usize;
    "#".repeat(n)
}
