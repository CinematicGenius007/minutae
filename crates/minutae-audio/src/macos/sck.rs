//! ScreenCaptureKit audio (macOS 13.0+), used where process taps are unavailable.
//! An `SCStream` with `capturesAudio` and `excludesCurrentProcessAudio`; its content
//! filter includes only the call app when it can be found, else the whole display.
//! Needs the Screen Recording permission.

use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMBlockBuffer, CMSampleBuffer, CMTime, CMTimeFlags,
    kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCRunningApplication, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamDelegate, SCStreamOutput, SCStreamOutputType,
};

use crate::capture::StreamSink;
use crate::target::{AppTarget, AudioProcess, ProcessTable};
use crate::{AudioError, Result};

/// ScreenCaptureKit delivers at the rate we ask for.
pub(crate) const RATE: u32 = 48_000;
const TIMEOUT: Duration = Duration::from_secs(5);

struct Ivars {
    producer: Mutex<rtrb::Producer<f32>>,
    paused: Arc<AtomicBool>,
    on_stop: Box<dyn Fn(String) + Send + Sync>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[name = "MinutaeSystemAudioOutput"]
    #[ivars = Ivars]
    struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(
            &self,
            _stream: &SCStream,
            sample: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind != SCStreamOutputType::Audio || self.ivars().paused.load(Ordering::Relaxed) {
                return;
            }
            // Runs on our serial dispatch queue (not a realtime thread); still no allocation.
            let Ok(mut producer) = self.ivars().producer.try_lock() else {
                return;
            };
            push_sample(&mut producer, sample);
        }
    }

    unsafe impl SCStreamDelegate for Output {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            (self.ivars().on_stop)(error.localizedDescription().to_string());
        }
    }
);

impl Output {
    fn new(ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ivars);
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

/// An AudioBufferList with room for 8 buffers.
#[repr(C, align(16))]
struct ListStorage {
    n: u32,
    bufs: [AudioBuffer; 8],
}

fn push_sample(producer: &mut rtrb::Producer<f32>, sample: &CMSampleBuffer) {
    let mut storage = MaybeUninit::<ListStorage>::zeroed();
    let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
    let mut need = 0usize;
    // SAFETY: size query only (no list, no block buffer).
    let status = unsafe {
        sample.audio_buffer_list_with_retained_block_buffer(
            &mut need,
            std::ptr::null_mut(),
            0,
            None,
            None,
            kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
            std::ptr::null_mut(),
        )
    };
    // CoreMedia rejects a list size other than the one it asks for (ArrayTooSmall).
    if status != 0 || need == 0 || need > size_of::<ListStorage>() {
        return;
    }
    // SAFETY: `storage` is a writable buffer of at least `need` bytes; `block` receives a
    // +1 reference.
    let status = unsafe {
        sample.audio_buffer_list_with_retained_block_buffer(
            std::ptr::null_mut(),
            storage.as_mut_ptr().cast::<AudioBufferList>(),
            need,
            None,
            None,
            kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
            &mut block,
        )
    };
    // SAFETY: we own the retained block buffer; it keeps the audio data alive until dropped.
    let _block = NonNull::new(block).map(|b| unsafe { CFRetained::<CMBlockBuffer>::from_raw(b) });
    if status != 0 {
        return;
    }
    // SAFETY: filled in by CoreMedia; ScreenCaptureKit audio is Float32 PCM.
    unsafe { super::push_f32_buffer_list(producer, storage.as_ptr().cast::<AudioBufferList>()) };
}

/// Run an async ScreenCaptureKit call and wait for its completion (or time out).
fn wait<T: Send + 'static>(start: impl FnOnce(mpsc::SyncSender<T>)) -> Option<T> {
    let (tx, rx) = mpsc::sync_channel(1);
    start(tx);
    rx.recv_timeout(TIMEOUT).ok()
}

struct SendBox<T>(T);
// SAFETY: ScreenCaptureKit objects are immutable snapshots / thread-safe; only moved
// between the completion thread and the waiting thread.
unsafe impl<T> Send for SendBox<T> {}

fn shareable_content() -> Result<Retained<SCShareableContent>> {
    let got = wait(|tx| {
        let block = RcBlock::new(
            move |content: *mut SCShareableContent, error: *mut NSError| {
                // SAFETY: valid (or null) objects handed to the completion handler.
                let res = match unsafe { Retained::retain(content) } {
                    Some(c) => Ok(SendBox(c)),
                    None => Err(unsafe { error.as_ref() }
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "no shareable content".into())),
                };
                let _ = tx.try_send(res);
            },
        );
        // SAFETY: the block lives as long as ScreenCaptureKit retains it.
        unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    });
    match got {
        Some(Ok(c)) => Ok(c.0),
        // Usually: the Screen Recording permission is missing.
        Some(Err(e)) => Err(AudioError::PermissionDenied(format!(
            "ScreenCaptureKit: {e}"
        ))),
        None => Err(AudioError::Device("ScreenCaptureKit did not answer".into())),
    }
}

struct Apps(Vec<AudioProcess>);

impl ProcessTable for Apps {
    fn processes(&self) -> Vec<AudioProcess> {
        self.0.clone()
    }
    fn responsible_pid(&self, pid: i32) -> Option<i32> {
        super::responsible_pid(pid)
    }
}

fn running_apps(content: &SCShareableContent) -> Vec<Retained<SCRunningApplication>> {
    // SAFETY: plain getter.
    unsafe { content.applications() }.to_vec()
}

/// The running applications that belong to `target` (empty = not found).
fn select_apps(
    content: &SCShareableContent,
    target: &AppTarget,
) -> Vec<Retained<SCRunningApplication>> {
    let apps = running_apps(content);
    let table = Apps(
        apps.iter()
            .map(|a| {
                // SAFETY: plain getters.
                let pid = unsafe { a.processID() };
                let bundle = unsafe { a.bundleIdentifier() }.to_string();
                AudioProcess {
                    handle: pid as u32,
                    pid,
                    bundle_id: (!bundle.is_empty()).then_some(bundle),
                    parent: super::parent_pid(pid),
                    responsible: super::responsible_pid(pid),
                }
            })
            .collect(),
    );
    let pids = crate::target::resolve(target, &table, std::process::id() as i32);
    apps.into_iter()
        .filter(|a| pids.contains(&(unsafe { a.processID() } as u32)))
        .collect()
}

/// The pids ScreenCaptureKit would capture for `target` right now (for re-targeting).
pub(crate) fn resolve(target: &AppTarget) -> Result<Vec<u32>> {
    let content = shareable_content()?;
    let mut pids: Vec<u32> = select_apps(&content, target)
        .iter()
        .map(|a| unsafe { a.processID() } as u32)
        .collect();
    pids.sort_unstable();
    Ok(pids)
}

/// A running ScreenCaptureKit audio stream. Dropping it stops capture.
pub(crate) struct SckStream {
    stream: Retained<SCStream>,
    _output: Retained<Output>,
    _queue: DispatchRetained<DispatchQueue>,
    /// Pids of the captured apps, sorted; empty = the whole display (all audio).
    pub pids: Vec<u32>,
}

impl SckStream {
    /// Start capturing `target`'s audio, or all audio when it is `None` or not found.
    /// `sink`'s ring must be sized for [`RATE`]. `on_stop` runs if the system stops the
    /// stream (permission revoked, display gone).
    pub fn start(
        target: Option<&AppTarget>,
        sink: StreamSink,
        on_stop: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<Self> {
        let content = shareable_content()?;
        // SAFETY: plain getter.
        let display = unsafe { content.displays() }.firstObject().ok_or_else(|| {
            AudioError::Unsupported("no display to attach ScreenCaptureKit to".into())
        })?;
        let apps = target.map(|t| select_apps(&content, t)).unwrap_or_default();
        let mut pids: Vec<u32> = apps
            .iter()
            .map(|a| unsafe { a.processID() } as u32)
            .collect();
        pids.sort_unstable();
        let no_windows = NSArray::new();
        // SAFETY: initialisers with valid arguments.
        let filter = unsafe {
            if apps.is_empty() {
                SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
                    SCContentFilter::alloc(),
                    &display,
                    &NSArray::new(),
                    &no_windows,
                )
            } else {
                SCContentFilter::initWithDisplay_includingApplications_exceptingWindows(
                    SCContentFilter::alloc(),
                    &display,
                    &NSArray::from_retained_slice(&apps),
                    &no_windows,
                )
            }
        };
        // SAFETY: setters on a fresh configuration.
        let config = unsafe {
            let c = SCStreamConfiguration::new();
            c.setCapturesAudio(true);
            c.setExcludesCurrentProcessAudio(true);
            c.setSampleRate(RATE as isize);
            c.setChannelCount(2);
            // Video can't be turned off: make it as cheap as possible.
            c.setWidth(2);
            c.setHeight(2);
            c.setMinimumFrameInterval(CMTime {
                value: 1,
                timescale: 1,
                flags: CMTimeFlags::Valid,
                epoch: 0,
            });
            c.setQueueDepth(3);
            c
        };
        let output = Output::new(Ivars {
            producer: Mutex::new(sink.producer),
            paused: sink.paused,
            on_stop,
        });
        let delegate = ProtocolObject::<dyn SCStreamDelegate>::from_ref(&*output);
        // SAFETY: valid filter, configuration and delegate (kept alive by `_output`).
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &config,
                Some(delegate),
            )
        };
        let queue = DispatchQueue::new("app.minutae.system-audio", None);
        let out = ProtocolObject::<dyn SCStreamOutput>::from_ref(&*output);
        for kind in [SCStreamOutputType::Audio, SCStreamOutputType::Screen] {
            // Screen frames are ignored; adding an output for them stops "output not
            // found" log spam.
            // SAFETY: valid output object and queue.
            unsafe {
                stream.addStreamOutput_type_sampleHandlerQueue_error(out, kind, Some(&queue))
            }
            .map_err(|e| {
                AudioError::Device(format!("ScreenCaptureKit: {}", e.localizedDescription()))
            })?;
        }
        let started = wait(|tx| {
            let block = RcBlock::new(move |error: *mut NSError| {
                // SAFETY: null or a valid NSError.
                let res = match unsafe { error.as_ref() } {
                    None => Ok(()),
                    Some(e) => Err(e.localizedDescription().to_string()),
                };
                let _ = tx.try_send(res);
            });
            // SAFETY: the block is retained by ScreenCaptureKit until called.
            unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };
        });
        match started {
            Some(Ok(())) => {}
            Some(Err(e)) => {
                return Err(AudioError::PermissionDenied(format!(
                    "ScreenCaptureKit: {e}"
                )));
            }
            None => return Err(AudioError::Device("ScreenCaptureKit did not start".into())),
        }
        Ok(Self {
            stream,
            _output: output,
            _queue: queue,
            pids,
        })
    }
}

impl Drop for SckStream {
    fn drop(&mut self) {
        let stream = &self.stream;
        let _ = wait(|tx: mpsc::SyncSender<()>| {
            let block = RcBlock::new(move |_error: *mut NSError| {
                let _ = tx.try_send(());
            });
            // SAFETY: the block is retained by ScreenCaptureKit until called.
            unsafe { stream.stopCaptureWithCompletionHandler(Some(&block)) };
        });
    }
}

/// Screen Recording permission (what ScreenCaptureKit needs). `false` = not granted yet.
pub fn screen_recording_granted() -> bool {
    // SAFETY: no arguments; a plain status query.
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Show the Screen Recording prompt (first time only). Returns whether it is granted.
pub fn request_screen_recording() -> bool {
    // SAFETY: no arguments.
    unsafe { CGRequestScreenCaptureAccess() }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}
