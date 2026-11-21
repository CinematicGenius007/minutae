//! A Core Audio process tap on a set of processes (macOS 14.4+): `CATapDescription`
//! (stereo mixdown of the processes) -> `AudioHardwareCreateProcessTap` -> a private
//! aggregate device holding the tap (clocked by the default output device) -> an IOProc
//! that downmixes into the ring.

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_audio::{
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, AudioObjectID,
    CATapDescription, CATapMuteBehavior, kAudioAggregateDeviceIsPrivateKey,
    kAudioAggregateDeviceIsStackedKey, kAudioAggregateDeviceMainSubDeviceKey,
    kAudioAggregateDeviceNameKey, kAudioAggregateDeviceSubDeviceListKey,
    kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
    kAudioAggregateDeviceUIDKey, kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyNominalSampleRate, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioSubDeviceUIDKey, kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey,
    kAudioTapPropertyFormat,
};
use objc2_core_audio_types::{
    AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, kAudioFormatFlagIsFloat,
    kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use super::{SYSTEM, get_plain, get_string};
use crate::capture::StreamSink;
use crate::{AudioError, Result};

static INSTANCE: AtomicU32 = AtomicU32::new(0);

/// Shared with the IOProc. Only the IO thread touches `producer` while the proc runs.
struct TapCtx {
    producer: UnsafeCell<rtrb::Producer<f32>>,
    paused: Arc<AtomicBool>,
}

/// A running (or prepared) tap. Dropping it stops the IOProc and destroys the aggregate
/// device and the tap.
pub(crate) struct ProcessTap {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    ctx: *mut TapCtx,
    /// The process objects in the tap, sorted.
    pub objects: Vec<AudioObjectID>,
    /// The output device clocking the aggregate (rebuild when the default changes).
    pub output_device: AudioObjectID,
    pub rate: u32,
}

fn os(status: i32, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(AudioError::Device(format!(
            "{what} failed (OSStatus {status})"
        )))
    }
}

fn key(k: &CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().unwrap_or_default())
}

fn yes() -> Retained<AnyObject> {
    obj(NSNumber::new_bool(true))
}

fn no() -> Retained<AnyObject> {
    obj(NSNumber::new_bool(false))
}

fn obj<T: objc2::Message + 'static>(v: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an AnyObject.
    unsafe { Retained::cast_unchecked(v) }
}

pub(crate) fn default_output_device() -> Option<AudioObjectID> {
    get_plain::<AudioObjectID>(SYSTEM, kAudioHardwarePropertyDefaultOutputDevice)
        .filter(|d| *d != 0)
}

pub(crate) fn is_alive(device: AudioObjectID) -> bool {
    get_plain::<u32>(device, kAudioDevicePropertyDeviceIsAlive) == Some(1)
}

impl ProcessTap {
    /// Create the tap and its aggregate device for `objects` (Core Audio process
    /// objects). Not started yet: [`Self::start`] it with a sink at [`Self::rate`].
    pub fn create(objects: &[AudioObjectID]) -> Result<Self> {
        if objects.is_empty() {
            return Err(AudioError::Unsupported("no process to tap".into()));
        }
        let output_device = default_output_device()
            .ok_or_else(|| AudioError::Unsupported("no default output device".into()))?;
        let output_uid = get_string(output_device, kAudioDevicePropertyDeviceUID)
            .ok_or_else(|| AudioError::Device("output device has no UID".into()))?;

        let numbers: Vec<Retained<NSNumber>> =
            objects.iter().map(|o| NSNumber::new_u32(*o)).collect();
        let list = NSArray::from_retained_slice(&numbers);
        let pid = std::process::id();
        let instance = INSTANCE.fetch_add(1, Ordering::Relaxed);
        // SAFETY: plain Objective-C initialisers/setters on a fresh description.
        let desc = unsafe {
            let d =
                CATapDescription::initStereoMixdownOfProcesses(CATapDescription::alloc(), &list);
            d.setMuteBehavior(CATapMuteBehavior::Unmuted); // the user still hears the call
            d.setPrivate(true);
            d.setName(&NSString::from_str(&format!(
                "Minutae call audio {pid}.{instance}"
            )));
            d
        };
        let mut tap: AudioObjectID = 0;
        // SAFETY: valid description and out pointer.
        os(
            unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap) },
            "AudioHardwareCreateProcessTap",
        )?;
        let mut this = Self {
            tap,
            aggregate: 0,
            proc_id: None,
            ctx: std::ptr::null_mut(),
            objects: objects.to_vec(),
            output_device,
            rate: 0,
        };
        this.objects.sort_unstable();

        let fmt = super::get_plain::<AudioStreamBasicDescription>(tap, kAudioTapPropertyFormat)
            .ok_or_else(|| AudioError::Device("cannot read the tap format".into()))?;
        if fmt.mFormatID != kAudioFormatLinearPCM
            || fmt.mFormatFlags & kAudioFormatFlagIsFloat == 0
            || fmt.mBitsPerChannel != 32
        {
            return Err(AudioError::Device(format!(
                "unexpected tap format {:#x}/{:#x}/{} bits",
                fmt.mFormatID, fmt.mFormatFlags, fmt.mBitsPerChannel
            )));
        }

        // SAFETY: the description's UUID is set at init.
        let tap_uid = unsafe { desc.UUID().UUIDString() };
        let sub_tap = NSDictionary::<NSString, AnyObject>::from_retained_objects(
            &[
                &*key(kAudioSubTapUIDKey),
                &*key(kAudioSubTapDriftCompensationKey),
            ],
            &[obj(tap_uid), yes()],
        );
        let sub_device = NSDictionary::<NSString, AnyObject>::from_retained_objects(
            &[&*key(kAudioSubDeviceUIDKey)],
            &[obj(NSString::from_str(&output_uid))],
        );
        let agg_uid = format!("app.minutae.calltap.{pid}.{instance}");
        let props = NSDictionary::<NSString, AnyObject>::from_retained_objects(
            &[
                &*key(kAudioAggregateDeviceNameKey),
                &*key(kAudioAggregateDeviceUIDKey),
                &*key(kAudioAggregateDeviceMainSubDeviceKey),
                &*key(kAudioAggregateDeviceIsPrivateKey),
                &*key(kAudioAggregateDeviceIsStackedKey),
                &*key(kAudioAggregateDeviceTapAutoStartKey),
                &*key(kAudioAggregateDeviceSubDeviceListKey),
                &*key(kAudioAggregateDeviceTapListKey),
            ],
            &[
                obj(NSString::from_str("Minutae call audio")),
                obj(NSString::from_str(&agg_uid)),
                obj(NSString::from_str(&output_uid)),
                yes(),
                no(),
                yes(),
                obj(NSArray::from_retained_slice(&[sub_device])),
                obj(NSArray::from_retained_slice(&[sub_tap])),
            ],
        );
        // SAFETY: NSDictionary is toll-free bridged to CFDictionary.
        let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&props) as *const CFDictionary) };
        let mut aggregate: AudioObjectID = 0;
        // SAFETY: valid dictionary and out pointer.
        os(
            unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut aggregate)) },
            "AudioHardwareCreateAggregateDevice",
        )?;
        this.aggregate = aggregate;
        let rate = get_plain::<f64>(aggregate, kAudioDevicePropertyNominalSampleRate)
            .filter(|r| *r > 0.0)
            .unwrap_or(fmt.mSampleRate);
        this.rate = rate.round() as u32;
        if this.rate == 0 {
            return Err(AudioError::Device("tap has no sample rate".into()));
        }
        Ok(this)
    }

    /// Start delivering into `sink` (whose ring must be sized for [`Self::rate`]).
    pub fn start(&mut self, sink: StreamSink) -> Result<()> {
        let ctx = Box::into_raw(Box::new(TapCtx {
            producer: UnsafeCell::new(sink.producer),
            paused: sink.paused,
        }));
        self.ctx = ctx;
        let mut proc_id: AudioDeviceIOProcID = None;
        // SAFETY: `ctx` lives until Drop, after the IOProc is destroyed.
        os(
            unsafe {
                AudioDeviceCreateIOProcID(
                    self.aggregate,
                    Some(io_proc),
                    ctx.cast::<c_void>(),
                    NonNull::from(&mut proc_id),
                )
            },
            "AudioDeviceCreateIOProcID",
        )?;
        self.proc_id = proc_id;
        // SAFETY: device and proc id were just created.
        os(
            unsafe { AudioDeviceStart(self.aggregate, self.proc_id) },
            "AudioDeviceStart",
        )
    }

    /// The aggregate died or the default output device changed: rebuild.
    pub fn stale(&self) -> bool {
        !is_alive(self.aggregate) || default_output_device() != Some(self.output_device)
    }
}

impl Drop for ProcessTap {
    fn drop(&mut self) {
        // SAFETY: tearing down objects this struct created, in reverse order. Statuses are
        // ignored: nothing useful can be done about them while dropping.
        unsafe {
            if self.proc_id.is_some() {
                AudioDeviceStop(self.aggregate, self.proc_id);
                AudioDeviceDestroyIOProcID(self.aggregate, self.proc_id);
            }
            if self.aggregate != 0 {
                AudioHardwareDestroyAggregateDevice(self.aggregate);
            }
            if self.tap != 0 {
                AudioHardwareDestroyProcessTap(self.tap);
            }
            if !self.ctx.is_null() {
                drop(Box::from_raw(self.ctx));
            }
        }
    }
}

/// The realtime callback: downmix the tap's input into the ring. No allocation, no locks.
unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    if client.is_null() {
        return 0;
    }
    // SAFETY: `client` is the TapCtx registered with this IOProc; only this thread uses
    // the producer while the proc is installed.
    unsafe {
        let ctx = &*(client as *const TapCtx);
        if ctx.paused.load(Ordering::Relaxed) {
            return 0;
        }
        super::push_f32_buffer_list(&mut *ctx.producer.get(), input.as_ptr());
    }
    0
}
