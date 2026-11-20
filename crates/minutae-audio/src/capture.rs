//! Device access through cpal: microphone and per-OS system-audio loopback.
//!
//! System audio ("everyone else on the call") is captured by opening an
//! *input* stream on the default *output* device:
//! - macOS 14.6+: cpal creates a Core Audio process tap + private aggregate device.
//! - Windows: cpal uses WASAPI loopback.
//! - Linux (PipeWire host, `pipewire` feature): cpal sets `stream.capture.sink`.
//! - Linux (PulseAudio host): we record the default sink's `.monitor` source.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig};
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

/// A mono block of samples at the device rate.
pub type Block = Vec<f32>;

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

/// Shared state the realtime callback reads.
#[derive(Clone)]
pub(crate) struct CallbackCtx {
    pub tx: SyncSender<Block>,
    pub paused: Arc<AtomicBool>,
    pub last_error: Arc<Mutex<Option<String>>>,
    pub label: &'static str,
}

pub(crate) fn build_stream(
    device: &cpal::Device,
    config: &SupportedStreamConfig,
    ctx: CallbackCtx,
) -> Result<cpal::Stream> {
    let sc: StreamConfig = config.config();
    match config.sample_format() {
        SampleFormat::F32 => typed::<f32>(device, sc, ctx),
        SampleFormat::I16 => typed::<i16>(device, sc, ctx),
        SampleFormat::I32 => typed::<i32>(device, sc, ctx),
        SampleFormat::U16 => typed::<u16>(device, sc, ctx),
        SampleFormat::U8 => typed::<u8>(device, sc, ctx),
        SampleFormat::I8 => typed::<i8>(device, sc, ctx),
        SampleFormat::F64 => typed::<f64>(device, sc, ctx),
        other => Err(AudioError::Device(format!(
            "unsupported sample format {other}"
        ))),
    }
}

fn typed<T>(device: &cpal::Device, sc: StreamConfig, ctx: CallbackCtx) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = sc.channels.max(1) as usize;
    let err_ctx = ctx.clone();
    device
        .build_input_stream::<T, _, _>(
            sc,
            move |data: &[T], _| {
                if ctx.paused.load(Ordering::Relaxed) {
                    return;
                }
                let mut block = Vec::with_capacity(data.len() / ch);
                if ch == 1 {
                    block.extend(data.iter().map(|&s| f32::from_sample(s)));
                } else {
                    let inv = 1.0 / ch as f32;
                    block.extend(
                        data.chunks_exact(ch)
                            .map(|f| f.iter().map(|&s| f32::from_sample(s)).sum::<f32>() * inv),
                    );
                }
                // Never block the audio thread; a full queue means the writer stalled.
                let _ = ctx.tx.try_send(block);
            },
            move |e: cpal::Error| {
                tracing::warn!(track = err_ctx.label, error = %e, "audio stream error");
                *err_ctx.last_error.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(format!("{}: {e}", err_ctx.label));
            },
            None,
        )
        .map_err(dev_err)
}
