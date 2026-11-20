//! Live captions with a real Whisper model, fed from WAV files in real time (100 ms blocks,
//! like the recorder's writer). Prints every caption event and the cost at the end.
//!
//! ```sh
//! say -o /tmp/you.aiff "Let's move the launch to May."    # macOS; any WAV works
//! afconvert -f WAVE -d LEI16@16000 /tmp/you.aiff /tmp/you.wav
//! cargo run --release -p minutae-asr --example live_captions -- \
//!     --model ~/Library/Minutae/models/ggml-base.bin --mic /tmp/you.wav [--system them.wav] [--speed 1]
//! ```
//! `--speed 4` feeds 4x faster than real time (to see the drop policy at work).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use minutae_asr::captions::{Caption, CaptionConfig, CaptionEngine, CaptionSink};
use minutae_asr::{TranscribeOptions, Transcriber, WhisperTranscriber, read_wav_16k_mono};
use minutae_core::Track;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// Process CPU time (user + system) so far.
#[cfg(unix)]
fn cpu_time() -> Duration {
    // SAFETY: getrusage writes into the zeroed struct we pass.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let tv = |t: libc::timeval| Duration::new(t.tv_sec as u64, t.tv_usec as u32 * 1000);
    tv(u.ru_utime) + tv(u.ru_stime)
}
#[cfg(not(unix))]
fn cpu_time() -> Duration {
    Duration::ZERO
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model = arg(&args, "--model").ok_or("--model <ggml-*.bin> is required")?;
    let mic = arg(&args, "--mic")
        .map(|p| read_wav_16k_mono(p.as_ref()))
        .transpose()?
        .unwrap_or_default();
    let system = arg(&args, "--system")
        .map(|p| read_wav_16k_mono(p.as_ref()))
        .transpose()?
        .unwrap_or_default();
    let speed: f32 = arg(&args, "--speed")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1.0);
    let language = arg(&args, "--language");

    let whisper = Arc::new(WhisperTranscriber::new(&model));
    let t0 = Instant::now();
    whisper.preload()?;
    println!("model {} loaded in {:?}", whisper.model_name(), t0.elapsed());

    let started = Instant::now();
    let log: Arc<Mutex<Vec<(Duration, Caption)>>> = Arc::default();
    let l2 = log.clone();
    let sink: CaptionSink = Arc::new(move |c: Caption| {
        let at = started.elapsed();
        println!(
            "[{:>6.2}s wall] {:>4} #{:<3} {:>6.1}-{:>6.1}s {} {:?}",
            at.as_secs_f32(),
            if c.track == Track::Mic { "You" } else { "Them" },
            c.id,
            c.start_ms as f32 / 1000.0,
            c.end_ms as f32 / 1000.0,
            if c.is_final { "FINAL  " } else { "partial" },
            c.text
        );
        l2.lock().unwrap().push((at, c));
    });
    let threads = std::thread::available_parallelism()
        .map(|n| (n.get() / 2).clamp(1, 4))
        .unwrap_or(2) as u32;
    let opts = TranscribeOptions {
        language,
        threads: Some(threads),
        ..Default::default()
    };
    let cpu0 = cpu_time();
    let (engine, mut mic_feed, mut sys_feed) = CaptionEngine::start(
        whisper.clone() as Arc<dyn Transcriber>,
        opts,
        CaptionConfig::default(),
        sink,
    )?;

    // Feed both tracks in 100 ms blocks, paced like a live recording.
    let block = 1_600usize;
    let total = mic.len().max(system.len()) + 16_000 * 6; // trailing silence flushes the last window
    let mut at = 0usize;
    while at < total {
        let take = |v: &[f32]| -> Vec<f32> {
            (at..at + block)
                .map(|i| v.get(i).copied().unwrap_or(0.0))
                .collect()
        };
        mic_feed.push(at as u64, &take(&mic));
        sys_feed.push(at as u64, &take(&system));
        at += block;
        let due = Duration::from_secs_f32(at as f32 / 16_000.0 / speed);
        if let Some(wait) = due.checked_sub(started.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    std::thread::sleep(Duration::from_secs(2));
    let wall = started.elapsed();
    let cpu = cpu_time() - cpu0;
    let stats = engine.stop();
    println!("\n{stats:#?}");
    println!(
        "audio fed: {:.1}s per track ({} tracks) in {:.1}s wall; process CPU {:.2}s = {:.0}% of one core on average",
        total as f32 / 16_000.0,
        if system.is_empty() { 1 } else { 2 },
        wall.as_secs_f32(),
        cpu.as_secs_f32(),
        cpu.as_secs_f32() / wall.as_secs_f32() * 100.0
    );
    if stats.audio_ms > 0 {
        println!(
            "worker busy {:.1}s for {:.1}s of audio transcribed (real-time factor {:.2})",
            stats.busy_ms as f32 / 1000.0,
            stats.audio_ms as f32 / 1000.0,
            stats.busy_ms as f32 / stats.audio_ms as f32
        );
    }
    let finals: Vec<String> = log
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, c)| c.is_final && !c.text.is_empty())
        .map(|(_, c)| c.text.clone())
        .collect();
    println!("final text: {}", finals.join(" "));
    assert!(whisper.unload_and_wait(Duration::from_secs(5)));
    Ok(())
}

