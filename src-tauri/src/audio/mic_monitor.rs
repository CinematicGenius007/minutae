use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "macos")]
use coreaudio_sys::{
    kAudioHardwarePropertyDefaultInputDevice,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectSystemObject, AudioObjectGetPropertyData, AudioObjectID,
    AudioObjectPropertyAddress,
};

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
            let mut was_active = false;

            while running.load(Ordering::SeqCst) {
                let is_active = is_mic_in_use();

                if is_active && !was_active {
                    let _ = app.emit("mic:activity-detected", ());
                } else if !is_active && was_active {
                    let _ = app.emit("mic:activity-stopped", ());
                }

                was_active = is_active;
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Returns true if any audio input device is currently in use by any process.
#[cfg(target_os = "macos")]
fn is_mic_in_use() -> bool {
    use std::mem;

    // kAudioDevicePropertyDeviceIsRunningSomewhere = 'gone'
    const K_AUDIO_DEVICE_PROP_IS_RUNNING_SOMEWHERE: u32 = u32::from_be_bytes(*b"gone");

    let default_device = get_default_input_device();
    if default_device == 0 {
        return false;
    }

    let address = AudioObjectPropertyAddress {
        mSelector: K_AUDIO_DEVICE_PROP_IS_RUNNING_SOMEWHERE,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut is_running: u32 = 0;
    let mut size = mem::size_of::<u32>() as u32;

    let status = unsafe {
        AudioObjectGetPropertyData(
            default_device,
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
    use std::mem;

    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultInputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut device_id: AudioObjectID = 0;
    let mut size = mem::size_of::<AudioObjectID>() as u32;

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
