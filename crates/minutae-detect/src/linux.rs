//! Linux: ask PulseAudio / pipewire-pulse which apps have capture streams.

use std::process::Command;

use crate::pactl::{active_mic_apps, parse_short_sources, parse_source_outputs};
use crate::{DetectError, MicActivity, MicMonitor, Result};

pub struct PactlMonitor {
    own_pid: u32,
}

impl PactlMonitor {
    pub fn new() -> Self {
        Self {
            own_pid: std::process::id(),
        }
    }
}

fn pactl(args: &[&str]) -> Result<String> {
    let out = Command::new("pactl")
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| {
            DetectError::Os(format!("pactl unavailable ({e}); install pulseaudio-utils"))
        })?;
    if !out.status.success() {
        return Err(DetectError::Os(format!(
            "pactl {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl MicMonitor for PactlMonitor {
    fn poll(&mut self) -> Result<MicActivity> {
        let outputs = parse_source_outputs(&pactl(&["list", "source-outputs"])?);
        if outputs.is_empty() {
            return Ok(MicActivity::free());
        }
        let sources = parse_short_sources(&pactl(&["list", "short", "sources"])?);
        let apps = active_mic_apps(&outputs, &sources, self.own_pid);
        Ok(if apps.is_empty() {
            MicActivity::free()
        } else {
            MicActivity::busy(apps)
        })
    }
}
