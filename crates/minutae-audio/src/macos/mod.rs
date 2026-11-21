//! macOS system audio: per-app Core Audio process taps (14.4+) and the ScreenCaptureKit
//! fallback for 13.0 - 14.3. The global "all audio" tap stays with cpal's loopback.

pub(crate) mod sck;
pub(crate) mod tap;

use std::ffi::{c_char, c_void};
use std::mem::size_of;
use std::ptr::NonNull;

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertySelector,
    kAudioHardwarePropertyProcessObjectList, kAudioHardwarePropertyTranslatePIDToProcessObject,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioProcessPropertyBundleID, kAudioProcessPropertyPID,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};
use objc2_core_foundation::{CFRetained, CFString};

use crate::target::{AudioProcess, ProcessTable, SystemBackend};

pub(crate) const SYSTEM: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;

/// The per-app backend for this Mac: process taps from macOS 14.4 (the API exists from
/// 14.2, but taps are unreliable before 14.4), ScreenCaptureKit before.
/// `MINUTAE_MAC_SYSTEM_AUDIO=sck|tap` overrides it (testing the fallback on a new Mac).
pub fn app_backend() -> SystemBackend {
    match std::env::var("MINUTAE_MAC_SYSTEM_AUDIO").as_deref() {
        Ok("sck") => return SystemBackend::ScreenCapture,
        Ok("tap") => return SystemBackend::ProcessTap,
        _ => {}
    }
    if os_at_least(14, 4) {
        SystemBackend::ProcessTap
    } else {
        SystemBackend::ScreenCapture
    }
}

pub fn os_at_least(major: isize, minor: isize) -> bool {
    use objc2_foundation::{NSOperatingSystemVersion, NSProcessInfo};
    let v = NSOperatingSystemVersion {
        majorVersion: major,
        minorVersion: minor,
        patchVersion: 0,
    };
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(v)
}

pub(crate) fn addr(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Read a fixed-size property (with an optional qualifier).
pub(crate) fn get<T: Copy, Q>(
    obj: AudioObjectID,
    selector: AudioObjectPropertySelector,
    qualifier: Option<&Q>,
) -> Option<T> {
    let a = addr(selector);
    // SAFETY: only plain-old-data property types (ints, floats, C structs) are read.
    let mut value: T = unsafe { std::mem::zeroed() };
    let mut size = size_of::<T>() as u32;
    let (qsize, qptr) = match qualifier {
        Some(q) => (size_of::<Q>() as u32, q as *const Q as *const c_void),
        None => (0, std::ptr::null()),
    };
    // SAFETY: `value` is a valid, writable T and `size` matches it; the qualifier (if
    // any) is a valid Q of `qsize` bytes.
    let status = unsafe {
        AudioObjectGetPropertyData(
            obj,
            NonNull::from(&a),
            qsize,
            qptr,
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast::<c_void>(),
        )
    };
    (status == 0 && size as usize == size_of::<T>()).then_some(value)
}

pub(crate) fn get_plain<T: Copy>(
    obj: AudioObjectID,
    selector: AudioObjectPropertySelector,
) -> Option<T> {
    get::<T, ()>(obj, selector, None)
}

/// Read a CFString property (returned at +1).
pub(crate) fn get_string(
    obj: AudioObjectID,
    selector: AudioObjectPropertySelector,
) -> Option<String> {
    let a = addr(selector);
    let mut s: *const CFString = std::ptr::null();
    let mut size = size_of::<*const CFString>() as u32;
    // SAFETY: the property is a CFStringRef; we own the returned reference.
    let status = unsafe {
        AudioObjectGetPropertyData(
            obj,
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
    // SAFETY: Core Audio "copy" semantics: the caller releases the string.
    let owned: CFRetained<CFString> = unsafe { CFRetained::from_raw(ptr) };
    Some(owned.to_string())
}

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
    let mut ids = vec![0 as AudioObjectID; size as usize / size_of::<AudioObjectID>()];
    if ids.is_empty() {
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

/// The Core Audio process object for `pid` (`kAudioHardwarePropertyTranslatePIDToProcessObject`).
/// `None` until the process has used audio at least once.
pub(crate) fn process_object(pid: i32) -> Option<AudioObjectID> {
    get::<AudioObjectID, i32>(
        SYSTEM,
        kAudioHardwarePropertyTranslatePIDToProcessObject,
        Some(&pid),
    )
    .filter(|o| *o != 0)
}

pub(crate) fn parent_pid(pid: i32) -> Option<i32> {
    // SAFETY: `info` is a correctly sized, writable proc_bsdinfo.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as i32;
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast::<c_void>(),
            size,
        )
    };
    (n == size).then_some(info.pbi_ppid as i32)
}

/// The app macOS holds responsible for `pid` (helpers -> their app). Uses the private
/// but long-stable `responsibility_get_pid_responsible_for_pid`, looked up at runtime so a
/// missing symbol only disables this hint.
pub(crate) fn responsible_pid(pid: i32) -> Option<i32> {
    type Func = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;
    static FUNC: std::sync::OnceLock<Option<Func>> = std::sync::OnceLock::new();
    let f = FUNC.get_or_init(|| {
        const NAME: &[u8] = b"responsibility_get_pid_responsible_for_pid\0";
        // SAFETY: dlsym with a NUL-terminated name; a non-null result is that function.
        let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, NAME.as_ptr() as *const c_char) };
        (!p.is_null()).then(|| unsafe { std::mem::transmute::<*mut c_void, Func>(p) })
    });
    // SAFETY: the function takes and returns a pid.
    let r = unsafe { (f.as_ref()?)(pid) };
    (r > 0).then_some(r)
}

/// Every Core Audio process object (processes that have used audio).
pub(crate) struct CoreAudioProcesses;

impl ProcessTable for CoreAudioProcesses {
    fn processes(&self) -> Vec<AudioProcess> {
        let Some(objs) = get_ids(SYSTEM, kAudioHardwarePropertyProcessObjectList) else {
            return vec![];
        };
        objs.into_iter()
            .filter_map(|o| {
                let pid = get_plain::<i32>(o, kAudioProcessPropertyPID)?;
                Some(AudioProcess {
                    handle: o,
                    pid,
                    bundle_id: get_string(o, kAudioProcessPropertyBundleID)
                        .filter(|b| !b.is_empty()),
                    parent: parent_pid(pid),
                    responsible: responsible_pid(pid),
                })
            })
            .collect()
    }

    fn responsible_pid(&self, pid: i32) -> Option<i32> {
        responsible_pid(pid)
    }
}

/// Downmix a Float32 `AudioBufferList` (one interleaved buffer, or one buffer per
/// channel) to mono into the ring. Never allocates or blocks; what does not fit is dropped.
///
/// # Safety
/// `list` must point to a valid `AudioBufferList` whose buffers hold Float32 samples.
pub(crate) unsafe fn push_f32_buffer_list(
    producer: &mut rtrb::Producer<f32>,
    list: *const AudioBufferList,
) {
    // SAFETY: caller guarantees `list` is valid; `mBuffers` is a C flexible array with
    // `mNumberBuffers` entries.
    let bufs: &[AudioBuffer] = unsafe {
        let n = (*list).mNumberBuffers as usize;
        if n == 0 {
            return;
        }
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*list).mBuffers).cast::<AudioBuffer>(),
            n,
        )
    };
    if bufs.iter().any(|b| b.mData.is_null()) {
        return;
    }
    if let [b] = bufs {
        let ch = b.mNumberChannels.max(1) as usize;
        // SAFETY: the buffer holds `mDataByteSize` bytes of f32 samples.
        let data = unsafe {
            std::slice::from_raw_parts(
                b.mData as *const f32,
                b.mDataByteSize as usize / size_of::<f32>(),
            )
        };
        crate::capture::push_downmixed(producer, data, ch);
        return;
    }
    let frames = bufs
        .iter()
        .map(|b| b.mDataByteSize as usize / size_of::<f32>() / b.mNumberChannels.max(1) as usize)
        .min()
        .unwrap_or(0);
    let total_ch: usize = bufs.iter().map(|b| b.mNumberChannels.max(1) as usize).sum();
    let n = frames.min(producer.slots());
    if n == 0 {
        return;
    }
    let Ok(chunk) = producer.write_chunk_uninit(n) else {
        return;
    };
    let inv = 1.0 / total_ch as f32;
    chunk.fill_from_iter((0..n).map(|i| {
        let mut sum = 0.0f32;
        for b in bufs {
            let ch = b.mNumberChannels.max(1) as usize;
            let d = b.mData as *const f32;
            for c in 0..ch {
                // SAFETY: i < frames, so i * ch + c is inside this buffer.
                sum += unsafe { *d.add(i * ch + c) };
            }
        }
        sum * inv
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct TwoBuffers {
        n: u32,
        bufs: [AudioBuffer; 2],
    }

    #[test]
    fn buffer_list_interleaved_and_planar() {
        let (mut p, mut c) = rtrb::RingBuffer::<f32>::new(16);
        // Interleaved stereo in one buffer.
        let mut inter = [1.0f32, 0.0, 0.5, 0.5];
        let one = AudioBufferList {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: 2,
                mDataByteSize: 16,
                mData: inter.as_mut_ptr().cast(),
            }],
        };
        unsafe { push_f32_buffer_list(&mut p, &one) };
        assert_eq!((c.pop().unwrap(), c.pop().unwrap()), (0.5, 0.5));
        // Planar: left and right in separate buffers, 3 frames.
        let mut l = [1.0f32, 0.0, 0.2];
        let mut r = [0.0f32, 0.0, 0.2];
        let mut two = TwoBuffers {
            n: 2,
            bufs: [
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: 12,
                    mData: l.as_mut_ptr().cast(),
                },
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: 12,
                    mData: r.as_mut_ptr().cast(),
                },
            ],
        };
        unsafe { push_f32_buffer_list(&mut p, (&mut two as *mut TwoBuffers).cast()) };
        assert_eq!(c.slots(), 3);
        assert_eq!(c.pop().unwrap(), 0.5);
        assert_eq!(c.pop().unwrap(), 0.0);
        assert!((c.pop().unwrap() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn process_table_and_version_probe_run() {
        // Real OS calls; must not panic. Our own process may or may not have an object.
        let procs = CoreAudioProcesses.processes();
        eprintln!(
            "{} audio processes; backend {:?}",
            procs.len(),
            app_backend()
        );
        let _ = process_object(std::process::id() as i32);
        assert!(parent_pid(std::process::id() as i32).is_some());
        assert!(os_at_least(13, 0));
    }
}
