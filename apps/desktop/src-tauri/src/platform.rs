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

/// Outcome of this session's system-audio request: 0 = not asked, 1 = granted,
/// 2 = denied. See [`system_audio`].
static SYSTEM_AUDIO_REQUEST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// macOS has no public status API for system-audio capture (Core Audio taps); the
/// prompt appears on first capture. Heuristic: until we asked in this session the state
/// is `notDetermined`; after `request(SystemAudio)` it is `granted` if the short test
/// capture started system audio without an error, else `denied` (so onboarding can move
/// on and offer System Settings). A tap the user denied may still record silence, so
/// this can over-report `granted`; recording surfaces "unavailable" if so.
/// On macOS 13.0 - 14.3 system audio comes from ScreenCaptureKit, whose Screen Recording
/// permission has a real status API, used instead.
/// Elsewhere no permission is needed.
pub fn system_audio() -> PermissionState {
    if !cfg!(target_os = "macos") {
        return PermissionState::NotRequired;
    }
    #[cfg(target_os = "macos")]
    if uses_screen_recording() {
        return if minutae_audio::screen_recording::granted() {
            PermissionState::Granted
        } else if SYSTEM_AUDIO_REQUEST.load(std::sync::atomic::Ordering::SeqCst) != 0 {
            PermissionState::Denied
        } else {
            PermissionState::NotDetermined
        };
    }
    match SYSTEM_AUDIO_REQUEST.load(std::sync::atomic::Ordering::SeqCst) {
        1 => PermissionState::Granted,
        2 => PermissionState::Denied,
        _ => PermissionState::NotDetermined,
    }
}

/// System audio needs Screen Recording here (ScreenCaptureKit, macOS 13.0 - 14.3).
#[cfg(target_os = "macos")]
fn uses_screen_recording() -> bool {
    minutae_audio::app_capture_backend() == minutae_audio::SystemBackend::ScreenCapture
}

/// No permission to ask for on this OS (Windows/Linux): onboarding has nothing to show.
pub fn nothing_to_grant(p: &Permissions) -> bool {
    [p.microphone, p.system_audio, p.accessibility]
        .iter()
        .all(|s| *s == PermissionState::NotRequired)
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
            // Before macOS 14.4: ScreenCaptureKit's Screen Recording prompt.
            #[cfg(target_os = "macos")]
            if uses_screen_recording() {
                let ok = minutae_audio::screen_recording::request();
                SYSTEM_AUDIO_REQUEST
                    .store(if ok { 1 } else { 2 }, std::sync::atomic::Ordering::SeqCst);
                return permissions();
            }
            // A short system-audio capture triggers the macOS prompt.
            #[cfg(target_os = "macos")]
            {
                let dir = std::env::temp_dir()
                    .join(format!("minutae-perm-{}", uuid::Uuid::new_v4().simple()));
                let cfg = minutae_audio::CaptureConfig {
                    mic_device: None,
                    include_system: true,
                    system_app: None,
                    out_dir: dir.clone(),
                    chunk_secs: 10,
                };
                let ok = match minutae_audio::Recorder::new().start(cfg) {
                    Ok(h) => {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        let ok = h.system_error().is_none() && h.last_stream_error().is_none();
                        let _ = h.stop();
                        ok
                    }
                    Err(_) => false,
                };
                SYSTEM_AUDIO_REQUEST
                    .store(if ok { 1 } else { 2 }, std::sync::atomic::Ordering::SeqCst);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_skipped_when_nothing_to_grant() {
        let none = Permissions {
            microphone: PermissionState::NotRequired,
            system_audio: PermissionState::NotRequired,
            accessibility: PermissionState::NotRequired,
        };
        assert!(nothing_to_grant(&none));
        let mac = Permissions {
            microphone: PermissionState::Granted,
            ..none
        };
        assert!(!nothing_to_grant(&mac));
    }
}
