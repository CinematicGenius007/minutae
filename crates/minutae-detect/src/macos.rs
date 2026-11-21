//! macOS: Core Audio process objects (macOS 14+) tell us exactly which
//! processes are running audio input; older systems fall back to
//! `kAudioDevicePropertyDeviceIsRunningSomewhere` on the default input device
//! (which cannot exclude our own process or name the app).

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::NonNull;

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertySelector,
    kAudioDevicePropertyDeviceIsRunningSomewhere, kAudioHardwarePropertyDefaultInputDevice,
    kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioProcessPropertyBundleID,
    kAudioProcessPropertyIsRunningInput, kAudioProcessPropertyPID,
};
use objc2_core_foundation::{CFRetained, CFString};

use crate::names::{from_bundle_id, macos_is_ignored};
use crate::{DetectError, MicActivity, MicMonitor, MicProcess, Result};

const SYSTEM: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;

fn addr(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Read a fixed-size property.
fn get<T: Copy + Default>(obj: AudioObjectID, selector: AudioObjectPropertySelector) -> Option<T> {
    let a = addr(selector);
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` is a valid, writable T and `size` matches it.
    let status = unsafe {
        AudioObjectGetPropertyData(
            obj,
            NonNull::from(&a),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast::<c_void>(),
        )
    };
    (status == 0 && size as usize == size_of::<T>()).then_some(value)
}

/// Read an array of AudioObjectIDs.
fn get_ids(
    obj: AudioObjectID,
    selector: AudioObjectPropertySelector,
) -> Option<Vec<AudioObjectID>> {
    let a = addr(selector);
    let mut size = 0u32;
    // SAFETY: valid address and out pointer.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            obj,
            NonNull::from(&a),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        return None;
    }
    let n = size as usize / size_of::<AudioObjectID>();
    let mut ids = vec![0 as AudioObjectID; n];
    if n == 0 {
        return Some(ids);
    }
    // SAFETY: `ids` has room for `size` bytes.
    let status = unsafe {
        AudioObjectGetPropertyData(
            obj,
            NonNull::from(&a),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(ids.as_mut_ptr().cast::<c_void>()).expect("non-null vec"),
        )
    };
    if status != 0 {
        return None;
    }
    ids.truncate(size as usize / size_of::<AudioObjectID>());
    Some(ids)
}

fn bundle_id(process: AudioObjectID) -> Option<String> {
    let a = addr(kAudioProcessPropertyBundleID);
    let mut s: *const CFString = std::ptr::null();
    let mut size = size_of::<*const CFString>() as u32;
    // SAFETY: the property is a CFStringRef returned at +1 (we take ownership).
    let status = unsafe {
        AudioObjectGetPropertyData(
            process,
            NonNull::from(&a),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut s).cast::<c_void>(),
        )
    };
    if status != 0 {
        return None;
    }
    let ptr = NonNull::new(s as *mut CFString)?;
    // SAFETY: Core Audio "copy" semantics: caller releases the string.
    let owned: CFRetained<CFString> = unsafe { CFRetained::from_raw(ptr) };
    let id = owned.to_string();
    (!id.is_empty()).then_some(id)
}

/// Process name for apps without a bundle id (CLI tools).
fn process_name(pid: i32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let name = path.rsplit('/').next()?.to_string();
    (!name.is_empty()).then_some(name)
}

pub struct CoreAudioMonitor {
    own_pid: i32,
}

impl CoreAudioMonitor {
    pub fn new() -> Self {
        Self {
            own_pid: std::process::id() as i32,
        }
    }

    fn poll_processes(&self) -> Option<MicActivity> {
        let procs = get_ids(SYSTEM, kAudioHardwarePropertyProcessObjectList)?;
        let mut apps: Vec<String> = Vec::new();
        let mut process: Option<MicProcess> = None;
        let mut busy = false;
        for p in procs {
            if get::<u32>(p, kAudioProcessPropertyIsRunningInput) != Some(1) {
                continue;
            }
            let pid = get::<i32>(p, kAudioProcessPropertyPID).unwrap_or(-1);
            if pid == self.own_pid {
                continue;
            }
            let bundle = bundle_id(p);
            let name = match &bundle {
                Some(b) if macos_is_ignored(b) => continue,
                Some(b) => from_bundle_id(b),
                None => match process_name(pid) {
                    Some(n) => crate::names::from_exe(&n),
                    None => "Unknown app".to_string(),
                },
            };
            busy = true;
            if !apps.contains(&name) {
                apps.push(name.clone());
            }
            // The first app's lowest pid: stable across polls, so the debouncer does not
            // see a "new" app when processes are listed in another order.
            if pid > 0 && apps[0] == name && process.as_ref().is_none_or(|cur| pid < cur.pid) {
                process = Some(MicProcess {
                    pid,
                    bundle_id: bundle,
                });
            }
        }
        Some(MicActivity {
            busy,
            apps,
            process,
        })
    }

    fn poll_device(&self) -> Result<MicActivity> {
        let dev = get::<AudioObjectID>(SYSTEM, kAudioHardwarePropertyDefaultInputDevice)
            .filter(|d| *d != 0)
            .ok_or_else(|| DetectError::Os("no default input device".into()))?;
        let running = get::<u32>(dev, kAudioDevicePropertyDeviceIsRunningSomewhere)
            .ok_or_else(|| DetectError::Os("cannot read device running state".into()))?;
        Ok(if running != 0 {
            MicActivity::busy(vec![])
        } else {
            MicActivity::free()
        })
    }
}

impl MicMonitor for CoreAudioMonitor {
    fn poll(&mut self) -> Result<MicActivity> {
        match self.poll_processes() {
            Some(a) => Ok(a),
            None => self.poll_device(),
        }
    }
}
