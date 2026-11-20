//! OS info and privacy permissions.

use crate::dto::{Os, PermissionKind, PermissionState, Permissions, PlatformInfo, SessionKind};

pub fn info(app_version: String) -> PlatformInfo {
    let os = if cfg!(target_os = "macos") {
        Os::Macos
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else {
        Os::Linux
    };
    let session_kind = match minutae_dictation::session_kind() {
        minutae_dictation::SessionKind::X11 => Some(SessionKind::X11),
        minutae_dictation::SessionKind::Wayland => Some(SessionKind::Wayland),
        _ => None,
    };
    PlatformInfo {
        os,
        session_kind,
        app_version,
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    use super::PermissionState;

    pub fn microphone() -> PermissionState {
        let Some(media) = (unsafe { AVMediaTypeAudio }) else {
            return PermissionState::Unsupported;
        };
        let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) };
        match status {
            AVAuthorizationStatus::Authorized => PermissionState::Granted,
            AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted => {
                PermissionState::Denied
            }
            _ => PermissionState::NotDetermined,
        }
    }

    /// Shows the system prompt (first time only) and waits up to 60 s for the answer.
    pub fn request_microphone() {
        let Some(media) = (unsafe { AVMediaTypeAudio }) else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let block = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
            let _ = tx.send(granted.as_bool());
        });
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &block) };
        let _ = rx.recv_timeout(std::time::Duration::from_secs(60));
    }
}

pub fn microphone() -> PermissionState {
    #[cfg(target_os = "macos")]
    return mac::microphone();
    #[cfg(not(target_os = "macos"))]
    PermissionState::NotRequired
}

/// macOS has no public status API for system-audio capture (Core Audio taps); the
/// prompt appears on first capture. Elsewhere no permission is needed.
pub fn system_audio() -> PermissionState {
    if cfg!(target_os = "macos") {
        PermissionState::NotDetermined
    } else {
        PermissionState::NotRequired
    }
}

pub fn accessibility() -> PermissionState {
    if !cfg!(target_os = "macos") {
        return PermissionState::NotRequired;
    }
    if minutae_dictation::accessibility_trusted() {
        PermissionState::Granted
    } else {
        PermissionState::NotDetermined
    }
}

pub fn permissions() -> Permissions {
    Permissions {
        microphone: microphone(),
        system_audio: system_audio(),
        accessibility: accessibility(),
    }
}

/// Blocking: may wait for a system prompt.
pub fn request(kind: PermissionKind) -> Permissions {
    match kind {
        PermissionKind::Microphone => {
            #[cfg(target_os = "macos")]
            mac::request_microphone();
        }
        PermissionKind::Accessibility => {
            minutae_dictation::prompt_accessibility();
        }
        PermissionKind::SystemAudio => {
            // A short system-audio capture triggers the macOS prompt.
            #[cfg(target_os = "macos")]
            {
                let dir = std::env::temp_dir()
                    .join(format!("minutae-perm-{}", uuid::Uuid::new_v4().simple()));
                let cfg = minutae_audio::CaptureConfig {
                    mic_device: None,
                    include_system: true,
                    out_dir: dir.clone(),
                    chunk_secs: 10,
                };
                if let Ok(h) = minutae_audio::Recorder::new().start(cfg) {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    let _ = h.stop();
                }
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }
    permissions()
}

/// Deep link to the matching privacy pane.
pub fn settings_url(kind: PermissionKind) -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some(match kind {
            PermissionKind::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            PermissionKind::SystemAudio => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            PermissionKind::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
        })
    } else if cfg!(target_os = "windows") {
        Some(match kind {
            PermissionKind::Microphone => "ms-settings:privacy-microphone",
            _ => "ms-settings:privacy",
        })
    } else {
        None
    }
}
