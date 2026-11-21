//! Runtime and ERLE on a long synthetic laptop call (no audio devices, no files).
//!
//!     cargo run --release -p minutae-aec --example bench [minutes] [drift_ppm]
//!
//! The far end talks in 40 s turns, the user answers in 20 s turns overlapping the
//! last 10 s of each (double-talk), through a reverberant room with 90 ms of delay.

use std::time::Instant;

use minutae_aec::synth::{self, energy};
use minutae_aec::{OfflineAec, cancel_to_vec};

fn main() {
    let args: Vec<f32> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let minutes = args.first().copied().unwrap_or(10.0);
    let ppm = args.get(1).copied().unwrap_or(30.0);
    let secs = minutes * 60.0;

    let t = Instant::now();
    let mut far = synth::speech_like(secs, 11, 190.0);
    let mut near = synth::speech_like(secs, 12, 120.0);
    let (mut far_on, mut near_on) = (Vec::new(), Vec::new());
    let mut at = 0.0;
    while at < secs {
        far_on.push((at, at + 40.0));
        near_on.push((at + 30.0, at + 50.0));
        at += 60.0;
    }
    synth::gate(&mut far, &far_on);
    synth::gate(&mut near, &near_on);
    let h = synth::room_ir(200.0, 0.3, 0.45, 13);
    let echo = synth::drift(&synth::delay(&synth::convolve(&far, &h), 90.0), ppm);
    let floor = synth::noise(far.len(), 3e-4, 14);
    let mic: Vec<f32> = echo
        .iter()
        .zip(&near)
        .zip(&floor)
        .map(|((e, n), f)| e + n + f)
        .collect();
    println!(
        "synthesised {minutes} min in {:.1} s",
        t.elapsed().as_secs_f32()
    );

    let t = Instant::now();
    let o = cancel_to_vec(&OfflineAec::default(), &mic, &far);
    let el = t.elapsed().as_secs_f64();
    let out = o.cleaned.expect("AEC ran");
    // Far-only stretches: 2–30 s of every minute.
    let (mut e_in, mut e_out) = (0.0, 0.0);
    let mut m = 0.0;
    while m < secs {
        let (a, b) = (
            energy(&echo, m + 2.0, m + 30.0),
            energy(&out, m + 2.0, m + 30.0),
        );
        if std::env::var_os("VERBOSE").is_some() {
            println!(
                "minute {:>3}: far-only ERLE {:.1} dB",
                (m / 60.0) as u32,
                10.0 * (a / b).log10()
            );
        }
        e_in += a;
        e_out += b;
        m += 60.0;
    }
    println!("{:?}", o.report);
    println!(
        "far-only ERLE {:.1} dB; {:.0} ms per minute of audio ({:.2} s total, {:.0}x real time)",
        10.0 * (e_in / e_out).log10(),
        el * 1000.0 / minutes as f64,
        el,
        secs as f64 / el
    );
}
