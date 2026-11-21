//! End-to-end checks on synthetic meetings (no hardware, no files except temp WAVs).

use super::*;
use crate::synth::{energy, laptop_call as scene};

fn db(a: f64, b: f64) -> f64 {
    10.0 * (a / b.max(1e-12)).log10()
}

fn diff(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x - y).collect()
}

#[test]
fn cancels_delayed_drifting_echo_and_keeps_the_user() {
    let s = scene(120.0, 40.0, 1);
    let t0 = Instant::now();
    let o = cancel_to_vec(&OfflineAec::default(), &s.mic, &s.reference);
    let elapsed = t0.elapsed();
    let out = o.cleaned.as_ref().expect("AEC ran");
    assert!(o.report.ran, "{:?}", o.report);

    // True ERLE where only the far end talks (the user is silent): echo in vs echo out.
    let far_only = [(2.0, 20.0), (42.0, 60.0)];
    let (mut e_in, mut e_out) = (0.0, 0.0);
    for (a, b) in far_only {
        e_in += energy(&s.echo, a, b);
        e_out += energy(out, a, b);
    }
    let erle = db(e_in, e_out);

    // Near-end only: the user's voice passes untouched.
    let near_only = db(energy(out, 30.5, 39.5), energy(&s.near, 30.5, 39.5));
    // Double-talk: output ≈ near; what's left besides near is residual echo + distortion.
    let residual = diff(out, &s.near);
    let dt_echo_reduction = db(energy(&s.echo, 21.0, 30.0), energy(&residual, 21.0, 30.0));
    let dt_near_level = db(energy(out, 21.0, 30.0), energy(&s.near, 21.0, 30.0));

    eprintln!(
        "ERLE far-only {erle:.1} dB (reported {:.1} dB), near-only level {near_only:+.2} dB, \
         double-talk: echo reduction {dt_echo_reduction:.1} dB, near level {dt_near_level:+.2} dB; \
         delay {:?} ms, drift {:?} ppm; {:.0} ms for 60 s ({} build)",
        o.report.erle_db.unwrap(),
        o.report.delay_ms,
        o.report.drift_ppm,
        elapsed.as_secs_f64() * 1000.0,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    );
    assert!(erle > 25.0, "ERLE {erle:.1} dB");
    assert!(o.report.erle_db.unwrap() > 20.0, "{:?}", o.report);
    assert!(
        near_only.abs() < 0.5,
        "near-only level changed {near_only:.2} dB"
    );
    assert!(
        dt_near_level.abs() < 0.5,
        "double-talk near level {dt_near_level:.2} dB"
    );
    assert!(
        dt_echo_reduction > 20.0,
        "double-talk echo reduction {dt_echo_reduction:.1} dB"
    );
    let d = o.report.delay_ms.unwrap();
    assert!((d - 120.0).abs() < 5.0, "delay {d} ms");
}

#[test]
fn handles_negative_offset_and_strong_drift() {
    // System track started late (mic lags by only 30 ms) and the clocks differ by 150 ppm
    // (9 ms over the minute) — a fixed filter would lose the echo half way through.
    let s = scene(30.0, 150.0, 2);
    let o = cancel_to_vec(&OfflineAec::default(), &s.mic, &s.reference);
    let out = o
        .cleaned
        .unwrap_or_else(|| panic!("AEC ran: {:?}", o.report));
    let late = db(energy(&s.echo, 45.0, 60.0), energy(&out, 45.0, 60.0));
    assert!(late > 20.0, "ERLE at the end {late:.1} dB");
    let ppm = o.report.drift_ppm.unwrap();
    assert!((ppm + 150.0).abs() < 40.0, "drift {ppm} ppm");
}

#[test]
fn headphones_are_skipped() {
    // No acoustic path: the mic only has the user and room noise.
    let s = scene(120.0, 0.0, 3);
    let mic: Vec<f32> = s.mic.iter().zip(&s.echo).map(|(m, e)| m - e).collect();
    let o = cancel_to_vec(&OfflineAec::default(), &mic, &s.reference);
    assert_eq!(o.report.skipped, Some(EchoSkip::NoEcho), "{:?}", o.report);
    assert!(!o.report.ran && o.cleaned.is_none());
}

#[test]
fn silent_or_missing_system_track_is_skipped() {
    let s = scene(120.0, 0.0, 4);
    let silent = vec![0.0f32; s.mic.len()];
    let o = cancel_to_vec(&OfflineAec::default(), &s.mic, &silent);
    assert_eq!(o.report.skipped, Some(EchoSkip::SystemSilent));
    let o = cancel_to_vec(&OfflineAec::default(), &s.mic, &[]);
    assert_eq!(o.report.skipped, Some(EchoSkip::NoSystemTrack));
}

#[test]
fn files_round_trip_and_stale_output_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let s = scene(80.0, 0.0, 5);
    let (mic, sys, out) = (
        dir.path().join("mic.wav"),
        dir.path().join("system.wav"),
        dir.path().join(CLEAN_MIC_FILE),
    );
    write_wav_16k_mono(&mic, &s.mic).unwrap();
    write_wav_16k_mono(&sys, &s.reference).unwrap();
    let r = cancel_files(&OfflineAec::default(), &mic, Some(&sys), &out).unwrap();
    assert!(r.ran && out.exists(), "{r:?}");
    let clean = read_wav_16k_mono(&out).unwrap();
    assert_eq!(clean.len(), s.mic.len());
    assert!(r.erle_db.unwrap() > 15.0);

    // Re-run with the canceller skipping: the old clean file must go.
    let r = cancel_files(
        &FakeEchoCanceller::skipping(EchoSkip::NoEcho),
        &mic,
        Some(&sys),
        &out,
    )
    .unwrap();
    assert!(!r.ran && !out.exists());
    let r = cancel_files(&OfflineAec::default(), &mic, None, &out).unwrap();
    assert_eq!(r.skipped, Some(EchoSkip::NoSystemTrack));
}

#[test]
fn report_is_camel_case() {
    let v = serde_json::to_value(FakeEchoCanceller::ran(20.0).report).unwrap();
    assert_eq!(v["erleDb"], 20.0);
    assert!(v.get("driftPpm").is_some());
    let v = serde_json::to_value(EchoCancelReport::skipped(EchoSkip::NoSystemTrack)).unwrap();
    assert_eq!(v["skipped"], "noSystemTrack");
}

#[test]
fn robust_across_rooms_delays_and_drifts() {
    // (seed, delay ms, drift ppm): the scenes used while tuning, incl. cases where the
    // GCC peak hops between the direct path and a reflection for long stretches.
    for (seed, delay, ppm) in [
        (2u64, 30.0f32, 150.0f32),
        (3, 200.0, -60.0),
        (4, 80.0, 0.0),
        (4, 80.0, 20.0),
        (5, 10.0, 100.0),
        (6, 300.0, 10.0),
        (7, 60.0, -30.0),
    ] {
        let s = scene(delay, ppm, seed);
        let o = cancel_to_vec(&OfflineAec::default(), &s.mic, &s.reference);
        let out = o
            .cleaned
            .unwrap_or_else(|| panic!("{seed}/{delay}/{ppm}: {:?}", o.report));
        let far = db(
            energy(&s.echo, 2.0, 20.0) + energy(&s.echo, 42.0, 60.0),
            energy(&out, 2.0, 20.0) + energy(&out, 42.0, 60.0),
        );
        let residual = diff(&out, &s.near);
        let dt = db(energy(&s.echo, 21.0, 30.0), energy(&residual, 21.0, 30.0));
        let near = db(energy(&out, 30.5, 39.5), energy(&s.near, 30.5, 39.5));
        assert!(
            far > 25.0 && dt > 20.0 && near.abs() < 0.5,
            "{seed}/{delay}/{ppm}: far-only {far:.1} dB, double-talk {dt:.1} dB, near {near:+.2} dB"
        );
    }
}
