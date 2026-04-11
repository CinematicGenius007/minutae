use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "macos")]
use coreaudio_sys::{
    kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectGetPropertyData,
    AudioObjectID, AudioObjectPropertyAddress,
};

// ─── FourCC constants ─────────────────────────────────────────────────────────

/// `kAudioDevicePropertyDeviceIsRunningSomewhere` = 'gone'
/// Returns 1 when any process has the device open for I/O.
#[cfg(target_os = "macos")]
const PROP_IS_RUNNING_SOMEWHERE: u32 = u32::from_be_bytes(*b"gone");

// ─── Callback context ─────────────────────────────────────────────────────────

/// Passed as `inClientData` to `AudioObjectAddPropertyListener`.
/// The callback fires on a CoreAudio thread and immediately wakes up the
/// monitoring loop via a sync channel, bypassing the 500 ms polling lag.
struct ListenerCtx {
    tx: std::sync::mpsc::SyncSender<()>,
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn property_listener_proc(
    _object_id: AudioObjectID,
    _n_addresses: u32,
    _addresses: *const AudioObjectPropertyAddress,
    client_data: *mut std::ffi::c_void,
) -> coreaudio_sys::OSStatus {
    // SAFETY: pointer is valid for the lifetime of the listener — kept alive by
    // the Box in the monitoring thread until AudioObjectRemovePropertyListener.
    if !client_data.is_null() {
        let ctx = &*(client_data as *const ListenerCtx);
        let _ = ctx.tx.try_send(());
    }
    0 // noErr
}

// ─── MicMonitor ──────────────────────────────────────────────────────────────

pub struct MicMonitor {
    running: Arc<AtomicBool>,
}

impl MicMonitor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self, app: AppHandle) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // already running
        }

        let running = Arc::clone(&self.running);

        std::thread::spawn(move || {
            // recv_timeout(500ms) = property-change wakeup OR 500ms fallback poll.
            let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);

            // Box the context for a stable pointer to pass to the C callback.
            let ctx = Box::new(ListenerCtx { tx });
            let ctx_ptr = Box::into_raw(ctx) as *mut std::ffi::c_void;

            // ── Register CoreAudio property listener (macOS only) ─────────────
            #[cfg(target_os = "macos")]
            let listener_device = {
                use coreaudio_sys::*;
                let device = get_default_input_device();
                if device != 0 {
                    let address = AudioObjectPropertyAddress {
                        mSelector: PROP_IS_RUNNING_SOMEWHERE,
                        mScope: kAudioObjectPropertyScopeGlobal,
                        mElement: kAudioObjectPropertyElementMain,
                    };
                    unsafe {
                        AudioObjectAddPropertyListener(
                            device,
                            &address,
                            Some(property_listener_proc),
                            ctx_ptr,
                        );
                    }
                }
                device
            };

            #[cfg(not(target_os = "macos"))]
            let listener_device: u32 = 0;

            // ── Emit initial state immediately ────────────────────────────────
            // Handles the case where Minutae is opened while a meeting is
            // already in progress — don't wait for the first poll cycle.
            let mut was_active = is_mic_in_use();
            if was_active {
                let _ = app.emit("mic:activity-detected", ());
            }

            // ── Monitoring loop ───────────────────────────────────────────────
            while running.load(Ordering::SeqCst) {
                // Block up to 500 ms; woken immediately when property changes.
                let _ = rx.recv_timeout(Duration::from_millis(500));

                let is_active = is_mic_in_use();

                if is_active && !was_active {
                    let _ = app.emit("mic:activity-detected", ());
                } else if !is_active && was_active {
                    let _ = app.emit("mic:activity-stopped", ());
                }

                was_active = is_active;
            }

            // ── Cleanup ───────────────────────────────────────────────────────
            #[cfg(target_os = "macos")]
            if listener_device != 0 {
                use coreaudio_sys::*;
                let address = AudioObjectPropertyAddress {
                    mSelector: PROP_IS_RUNNING_SOMEWHERE,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMain,
                };
                unsafe {
                    // Remove listener first — after this returns no more callbacks
                    // fire, so ctx_ptr is safe to free.
                    AudioObjectRemovePropertyListener(
                        listener_device,
                        &address,
                        Some(property_listener_proc),
                        ctx_ptr,
                    );
                }
            }

            // Free the context (and the tx sender inside it).
            unsafe {
                drop(Box::from_raw(ctx_ptr as *mut ListenerCtx));
            }
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

// ─── CoreAudio helpers ────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn is_mic_in_use() -> bool {
    let device = get_default_input_device();
    if device == 0 {
        return false;
    }

    let address = AudioObjectPropertyAddress {
        mSelector: PROP_IS_RUNNING_SOMEWHERE,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut is_running: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;

    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut is_running as *mut u32 as *mut std::ffi::c_void,
        )
    };

    status == 0 && is_running != 0
}

#[cfg(target_os = "macos")]
fn get_default_input_device() -> AudioObjectID {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultInputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut device_id: AudioObjectID = 0;
    let mut size = std::mem::size_of::<AudioObjectID>() as u32;

    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut device_id as *mut AudioObjectID as *mut std::ffi::c_void,
        )
    };

    if status == 0 { device_id } else { 0 }
}

#[cfg(not(target_os = "macos"))]
fn is_mic_in_use() -> bool {
    false
}
