//! Device access through cpal: microphone and per-OS system-audio loopback.
//!
//! System audio ("everyone else on the call") is captured by opening an
//! *input* stream on the default *output* device:
//! - macOS 14.6+: cpal creates a Core Audio process tap + private aggregate device.
//! - Windows: cpal uses WASAPI loopback.
//! - Linux (PipeWire host, `pipewire` feature): cpal sets `stream.capture.sink`.
//! - Linux (PulseAudio host): we record the default sink's `.monitor` source.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig};
use minutae_core::Track;
use serde::{Deserialize, Serialize};

use crate::{AudioError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    /// Stable id (cpal `DeviceId` string, e.g. `coreaudio:BuiltInMicrophoneDevice`).
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn list_input_devices() -> Result<Vec<InputDevice>> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().and_then(|d| d.id().ok());
    let mut out = Vec::new();
    for d in host.input_devices().map_err(dev_err)? {
        let Ok(id) = d.id() else { continue };
        let name = d
            .description()
            .map(|desc| desc.name().to_string())
            .unwrap_or_else(|_| id.to_string());
        out.push(InputDevice {
            is_default: Some(&id) == default_id.as_ref(),
            id: id.to_string(),
            name,
        });
    }
    Ok(out)
}

pub(crate) fn device_name(d: &cpal::Device) -> String {
    d.description()
        .map(|desc| desc.name().to_string())
        .or_else(|_| d.id().map(|id| id.to_string()))
        .unwrap_or_else(|_| "unknown device".into())
}

pub(crate) fn dev_err(e: cpal::Error) -> AudioError {
    match e.kind() {
        cpal::ErrorKind::PermissionDenied => AudioError::PermissionDenied(e.to_string()),
        _ => AudioError::Device(e.to_string()),
    }
}

pub(crate) fn open_mic(id: Option<&str>) -> Result<(cpal::Device, SupportedStreamConfig)> {
    let host = cpal::default_host();
    let device = match id {
        Some(id) => {
            let parsed: cpal::DeviceId = id
                .parse()
                .map_err(|_| AudioError::Device(format!("invalid device id: {id}")))?;
            host.device_by_id(&parsed)
                .ok_or_else(|| AudioError::Device(format!("input device not found: {id}")))?
        }
        None => host
            .default_input_device()
            .ok_or(AudioError::NoInputDevice)?,
    };
    let config = device.default_input_config().map_err(dev_err)?;
    Ok((device, config))
}

pub(crate) fn open_system() -> Result<(cpal::Device, SupportedStreamConfig)> {
    let host = cpal::default_host();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::Unsupported("no default output device".into()))?;
        let config = device.default_output_config().map_err(dev_err)?;
        Ok((device, config))
    }
    #[cfg(target_os = "linux")]
    {
        // PipeWire host: input on a sink captures what plays to it.
        if host.id() != cpal::HostId::Alsa
            && let Some(device) = host.default_output_device()
            && device.supports_input()
            && let Ok(config) = device.default_output_config()
        {
            return Ok((device, config));
        }
        // PulseAudio host: the sink's monitor source is a regular input device.
        let sink_name = host
            .default_output_device()
            .and_then(|d| d.id().ok())
            .map(|id| id.id().to_string());
        let mut monitors: Vec<cpal::Device> = host
            .input_devices()
            .map_err(dev_err)?
            .filter(|d| {
                d.id()
                    .map(|i| i.id().ends_with(".monitor"))
                    .unwrap_or(false)
            })
            .collect();
        if let Some(sink) = sink_name {
            let want = format!("{sink}.monitor");
            monitors.sort_by_key(|d| d.id().map(|i| i.id() != want).unwrap_or(true));
        }
        let device = monitors.into_iter().next().ok_or_else(|| {
            AudioError::Unsupported(
                "no PulseAudio/PipeWire monitor source found (ALSA-only systems cannot capture system audio)"
                    .into(),
            )
        })?;
        let config = device.default_input_config().map_err(dev_err)?;
        Ok((device, config))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = host;
        Err(AudioError::Unsupported(
            "system audio capture not supported on this OS".into(),
        ))
    }
}

/// Where a capture stream's samples and errors go.
pub(crate) struct StreamSink {
    /// Mono samples at the device rate. Written only by the realtime callback.
    pub producer: rtrb::Producer<f32>,
    pub paused: Arc<AtomicBool>,
    pub track: Track,
    /// Bumped by the capture thread each time `track` is reopened; stale errors are ignored.
    pub generation: u64,
    pub on_error: Arc<dyn Fn(Track, u64, cpal::Error) + Send + Sync>,
}

/// Ring capacity in samples: ~5 s at the device rate.
pub(crate) fn ring_capacity(rate: u32) -> usize {
    (rate.max(8_000) as usize) * 5
}

pub(crate) fn build_stream(
    device: &cpal::Device,
    config: &SupportedStreamConfig,
    sink: StreamSink,
) -> Result<cpal::Stream> {
    let sc: StreamConfig = config.config();
    match config.sample_format() {
        SampleFormat::F32 => typed::<f32>(device, sc, sink),
        SampleFormat::I16 => typed::<i16>(device, sc, sink),
        SampleFormat::I32 => typed::<i32>(device, sc, sink),
        SampleFormat::U16 => typed::<u16>(device, sc, sink),
        SampleFormat::U8 => typed::<u8>(device, sc, sink),
        SampleFormat::I8 => typed::<i8>(device, sc, sink),
        SampleFormat::F64 => typed::<f64>(device, sc, sink),
        other => Err(AudioError::Device(format!(
            "unsupported sample format {other}"
        ))),
    }
}

/// Downmix interleaved `data` (`ch` channels) to mono into the ring. Never allocates or
/// blocks: if the writer has fallen ~5 s behind, the samples that don't fit are dropped
/// (the writer pads the gap with silence against the wall clock).
pub(crate) fn push_downmixed<T>(producer: &mut rtrb::Producer<f32>, data: &[T], ch: usize)
where
    T: Sample,
    f32: FromSample<T>,
{
    let frames = data.len() / ch.max(1);
    let n = frames.min(producer.slots());
    if n == 0 {
        return;
    }
    let Ok(chunk) = producer.write_chunk_uninit(n) else {
        return;
    };
    if ch <= 1 {
        chunk.fill_from_iter(data.iter().map(|&s| f32::from_sample(s)));
    } else {
        let inv = 1.0 / ch as f32;
        chunk.fill_from_iter(
            data.chunks_exact(ch)
                .map(|f| f.iter().map(|&s| f32::from_sample(s)).sum::<f32>() * inv),
        );
    }
}

fn typed<T>(device: &cpal::Device, sc: StreamConfig, sink: StreamSink) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = sc.channels.max(1) as usize;
    let StreamSink {
        mut producer,
        paused,
        track,
        generation,
        on_error,
    } = sink;
    device
        .build_input_stream::<T, _, _>(
            sc,
            move |data: &[T], _| {
                if paused.load(Ordering::Relaxed) {
                    return;
                }
                push_downmixed(&mut producer, data, ch);
            },
            move |e: cpal::Error| on_error(track, generation, e),
            None,
        )
        .map_err(dev_err)
}

/// Errors after which the stream delivers nothing more and must be rebuilt.
pub(crate) fn needs_reopen(kind: cpal::ErrorKind) -> bool {
    matches!(
        kind,
        cpal::ErrorKind::DeviceNotAvailable
            | cpal::ErrorKind::StreamInvalidated
            | cpal::ErrorKind::HostUnavailable
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_into_ring_drops_overflow() {
        let (mut p, mut c) = rtrb::RingBuffer::<f32>::new(4);
        // Stereo i16: frames (max, max), (0, max) -> 1.0, 0.5
        push_downmixed(&mut p, &[i16::MAX, i16::MAX, 0, i16::MAX], 2);
        assert!((c.pop().unwrap() - 1.0).abs() < 1e-3);
        assert!((c.pop().unwrap() - 0.5).abs() < 1e-3);
        // 6 mono frames into 4 free slots: the last 2 are dropped.
        push_downmixed(&mut p, &[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6], 1);
        assert_eq!(c.slots(), 4);
        assert_eq!(c.pop().unwrap(), 0.1);
        push_downmixed::<f32>(&mut p, &[], 1);
        assert_eq!(c.slots(), 3);
    }
}
