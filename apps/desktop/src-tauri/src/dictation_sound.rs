//! A short, low-volume output cue. The audio input service is independent of playback.

use std::{f32::consts::TAU, time::Duration};

use cpal::{
    FromSample, SampleFormat, SizedSample, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

pub fn play() -> anyhow::Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("No output device"))?;
    let supported = device.default_output_config()?;
    let config = supported.config();
    match supported.sample_format() {
        SampleFormat::F32 => play_tone::<f32>(&device, &config),
        SampleFormat::I16 => play_tone::<i16>(&device, &config),
        SampleFormat::U16 => play_tone::<u16>(&device, &config),
        _ => Ok(()),
    }
}

fn play_tone<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &StreamConfig,
) -> anyhow::Result<()> {
    let channels = usize::from(config.channels);
    let rate = config.sample_rate as f32;
    let mut frame = 0;
    let (done, wait) = std::sync::mpsc::sync_channel(1);
    let stream = device.build_output_stream(
        *config,
        move |output: &mut [T], _| {
            for samples in output.chunks_mut(channels) {
                let t = frame as f32 / rate;
                samples.fill(T::from_sample(tone(t)));
                frame += 1;
            }
            if frame as f32 / rate >= 0.085 {
                let _ = done.try_send(());
            }
        },
        |e| tracing::debug!(error = %e, "dictation cue playback failed"),
        None,
    )?;
    stream.play()?;
    let _ = wait.recv_timeout(Duration::from_millis(300));
    // Let the output buffer drain before dropping the stream.
    std::thread::sleep(Duration::from_millis(85));
    Ok(())
}

fn tone(seconds: f32) -> f32 {
    let position = seconds / 0.085;
    if position >= 1.0 {
        return 0.0;
    }
    let envelope = (std::f32::consts::PI * position).sin().powi(2);
    0.035 * envelope * (TAU * (660.0 + 120.0 * position) * seconds).sin()
}

#[cfg(test)]
mod tests {
    use super::tone;

    #[test]
    fn cue_has_soft_edges_and_low_amplitude() {
        assert_eq!(tone(0.0), 0.0);
        assert_eq!(tone(0.085), 0.0);
        assert!((1..85).any(|ms| tone(ms as f32 / 1000.0) != 0.0));
        assert!((1..85).all(|ms| tone(ms as f32 / 1000.0).abs() <= 0.035));
    }
}
