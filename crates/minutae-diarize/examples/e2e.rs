//! End-to-end check: download models → transcribe → diarize → merge → print.
//!
//! ```sh
//! cargo run --release -p minutae-diarize --example e2e -- <models_dir> <mic.wav> <system.wav> [mixed.wav]
//! ```
//! Env: `WHISPER_MODEL=tiny|base|small|medium` (default base), `NUM_SPEAKERS=<n>`, `NO_GPU=1`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use minutae_asr::{ModelManager, TranscribeOptions, Transcriber, WhisperTranscriber};
use minutae_core::{Segment, Speaker, Track, WhisperModel};
use minutae_diarize::{
    DiarizeModels, Diarizer, SherpaDiarizer, merge, merge_single_track, read_wav_16k_mono,
};
use uuid::Uuid;

fn secs(p: &Path) -> f64 {
    read_wav_16k_mono(p)
        .map(|s| s.len() as f64 / 16_000.0)
        .unwrap_or(0.0)
}

fn print_transcript(title: &str, segs: &[Segment], speakers: &[Speaker]) {
    println!("\n=== {title} ===");
    for s in segs {
        let who = s
            .speaker_id
            .and_then(|id| speakers.iter().find(|p| p.id == id))
            .map(|p| p.label.as_str())
            .unwrap_or("?");
        println!(
            "[{:>6.2}–{:>6.2}] {:<10} {}",
            s.start_ms as f64 / 1000.0,
            s.end_ms as f64 / 1000.0,
            who,
            s.text
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: e2e <models_dir> <mic.wav> <system.wav> [mixed.wav]");
        std::process::exit(2);
    }
    let models_dir = PathBuf::from(&args[0]);
    let (mic, system) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let mixed = args.get(3).map(PathBuf::from);
    let model = match std::env::var("WHISPER_MODEL").as_deref() {
        Ok("tiny") => WhisperModel::Tiny,
        Ok("small") => WhisperModel::Small,
        Ok("medium") => WhisperModel::Medium,
        _ => WhisperModel::Base,
    };
    let num_speakers = std::env::var("NUM_SPEAKERS")
        .ok()
        .and_then(|n| n.parse().ok());

    // 1. Models
    let t = Instant::now();
    let mm = ModelManager::with_dir(&models_dir);
    let mut last = 0;
    mm.download(model, |p| {
        let pct = (p.fraction().unwrap_or(0.0) * 100.0) as u32;
        if pct >= last + 20 {
            last = pct;
            eprintln!("  whisper model {pct}%");
        }
    })
    .await?;
    let dm = DiarizeModels::with_models_dir(&models_dir);
    dm.download(|_| {}).await?;
    println!(
        "models ready in {:.1}s: {:?} + {:?}",
        t.elapsed().as_secs_f64(),
        mm.status(model).path,
        dm.dir()
    );

    // 2. Transcribe
    let asr =
        WhisperTranscriber::from_manager(&mm, model)?.with_gpu(std::env::var("NO_GPU").is_err());
    let t = Instant::now();
    asr.preload()?;
    println!("whisper load: {:.2}s", t.elapsed().as_secs_f64());
    let opts = TranscribeOptions {
        language: Some("en".into()),
        ..Default::default()
    };
    let t = Instant::now();
    let mic_segs = asr.transcribe(&mic, Track::Mic, &opts, None)?;
    let sys_segs = asr.transcribe(&system, Track::System, &opts, None)?;
    let asr_s = t.elapsed().as_secs_f64();
    let audio_s = secs(&mic) + secs(&system);
    println!(
        "transcribe {:.1}s of audio in {:.2}s → RTF {:.3}",
        audio_s,
        asr_s,
        asr_s / audio_s
    );

    // 3. Diarize the system track
    let diar = SherpaDiarizer::new(&dm)?;
    let t = Instant::now();
    let turns = diar.diarize(&system, num_speakers)?;
    let d_s = t.elapsed().as_secs_f64();
    println!(
        "diarize {:.1}s in {:.2}s → RTF {:.3}",
        secs(&system),
        d_s,
        d_s / secs(&system)
    );
    for t in &turns {
        println!(
            "  turn {:>6.2}–{:>6.2} speaker {}",
            t.start_ms as f64 / 1000.0,
            t.end_ms as f64 / 1000.0,
            t.speaker
        );
    }

    // 4. Merge
    let (segs, speakers) = merge(&mic_segs, &sys_segs, &turns, Uuid::new_v4());
    print_transcript("two-track (mic = You, system diarized)", &segs, &speakers);

    // 5. Optional: in-room single-track mode on a mixed recording
    if let Some(mixed) = mixed {
        let t = Instant::now();
        let segs = asr.transcribe(&mixed, Track::Mic, &opts, None)?;
        let a = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let turns = diar.diarize(&mixed, num_speakers)?;
        let d = t.elapsed().as_secs_f64();
        let len = secs(&mixed);
        println!(
            "\nmixed {:.1}s: transcribe {:.2}s (RTF {:.3}), diarize {:.2}s (RTF {:.3}), {} turns",
            len,
            a,
            a / len,
            d,
            d / len,
            turns.len()
        );
        let (segs, speakers) = merge_single_track(&segs, &turns, Uuid::new_v4());
        print_transcript("single-track in-room (all diarized)", &segs, &speakers);
    }
    Ok(())
}
