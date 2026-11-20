//! whisper.cpp engine via `whisper-rs`.
//!
//! GPU: Metal on macOS (always compiled in), Vulkan/CUDA with the `vulkan`/`cuda`
//! cargo features; otherwise CPU. whisper.cpp falls back to CPU at runtime when
//! the GPU backend can't initialise.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};

use minutae_core::{Segment, Track, WhisperModel};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{
    AsrError, ModelManager, ProgressFn, Result, TranscribeOptions, Transcriber, model_info,
};

static LOG_HOOKS: Once = Once::new();

/// Real transcriber. The model is loaded lazily on first use and cached until
/// [`WhisperTranscriber::unload`] or drop. One instance can serve many calls,
/// from many threads (each call gets its own whisper state).
pub struct WhisperTranscriber {
    model_path: PathBuf,
    model_name: String,
    use_gpu: bool,
    ctx: Mutex<Option<Arc<WhisperContext>>>,
}

impl WhisperTranscriber {
    /// Use an explicit `ggml-*.bin` path.
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        let model_path = model_path.into();
        let stem = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("model");
        let model_name = format!("whisper-{}", stem.trim_start_matches("ggml-"));
        LOG_HOOKS.call_once(whisper_rs::install_logging_hooks);
        Self {
            model_path,
            model_name,
            use_gpu: true,
            ctx: Mutex::new(None),
        }
    }

    /// Use a model managed by `manager`; fails if it isn't downloaded.
    pub fn from_manager(manager: &ModelManager, model: WhisperModel) -> Result<Self> {
        let st = manager.status(model);
        if !st.downloaded {
            return Err(AsrError::ModelMissing(model_info(model).name.to_string()));
        }
        Ok(Self::new(st.path))
    }

    /// Disable the GPU backend (Metal/Vulkan/CUDA). Must be called before the first transcription.
    pub fn with_gpu(mut self, use_gpu: bool) -> Self {
        self.use_gpu = use_gpu;
        self
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// Load the model now (e.g. when recording starts) instead of on first call.
    pub fn preload(&self) -> Result<()> {
        self.context().map(|_| ())
    }

    pub fn is_loaded(&self) -> bool {
        self.ctx.lock().map(|c| c.is_some()).unwrap_or(false)
    }

    /// Free the model's memory; it is reloaded on the next call. A transcription that is
    /// running keeps its own reference, so memory is freed when it finishes.
    pub fn unload(&self) {
        if let Ok(mut c) = self.ctx.lock() {
            *c = None;
        }
    }

    /// Unload and wait (up to `timeout`) until no running transcription still holds the
    /// model. Call this before the process exits: ggml-metal asserts in its static
    /// destructor when a Metal context is still alive. Returns `false` on timeout.
    pub fn unload_and_wait(&self, timeout: std::time::Duration) -> bool {
        let weak = match self.ctx.lock() {
            Ok(mut c) => c.take().map(|ctx| Arc::downgrade(&ctx)),
            Err(p) => p.into_inner().take().map(|ctx| Arc::downgrade(&ctx)),
        };
        let Some(weak) = weak else { return true };
        let deadline = std::time::Instant::now() + timeout;
        while weak.strong_count() > 0 {
            if std::time::Instant::now() >= deadline {
                tracing::warn!(target: "minutae_asr", "{} still in use after {:?}", self.model_name, timeout);
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }

    fn context(&self) -> Result<Arc<WhisperContext>> {
        let mut guard = self
            .ctx
            .lock()
            .map_err(|_| AsrError::Whisper("model lock poisoned".into()))?;
        if let Some(ctx) = guard.as_ref() {
            return Ok(ctx.clone());
        }
        if !self.model_path.exists() {
            return Err(AsrError::ModelMissing(
                self.model_name.trim_start_matches("whisper-").to_string(),
            ));
        }
        let mut params = WhisperContextParameters::default();
        params.use_gpu(self.use_gpu);
        let started = std::time::Instant::now();
        let ctx = WhisperContext::new_with_params(&self.model_path, params).map_err(|e| {
            AsrError::Whisper(format!("loading {}: {e}", self.model_path.display()))
        })?;
        tracing::info!(target: "minutae_asr", "loaded {} in {:?} (gpu={})", self.model_path.display(), started.elapsed(), self.use_gpu);
        let ctx = Arc::new(ctx);
        *guard = Some(ctx.clone());
        Ok(ctx)
    }
}

/// Default thread count: all cores up to 8 (whisper.cpp scales poorly beyond that).
fn default_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(4) as i32
}

/// ggml abort callback: `data` points at the caller's cancel `AtomicBool`.
unsafe extern "C" fn abort_trampoline(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    // SAFETY: set from `Arc::as_ptr(&Arc<AtomicBool>)`, alive for the whole `full()` call.
    unsafe { (*(data as *const AtomicBool)).load(Ordering::Relaxed) }
}

/// whisper.cpp progress callback: `data` points at the caller's [`ProgressFn`].
unsafe extern "C" fn progress_trampoline(
    _ctx: *mut whisper_rs_sys::whisper_context,
    _state: *mut whisper_rs_sys::whisper_state,
    pct: std::os::raw::c_int,
    data: *mut c_void,
) {
    if data.is_null() {
        return;
    }
    // SAFETY: set from `&ProgressFn`, alive for the whole `full()` call.
    let cb = unsafe { &*(data as *const ProgressFn) };
    cb((pct.clamp(0, 100) as f32) / 100.0);
}

/// whisper.cpp emits bracketed non-speech markers like `[BLANK_AUDIO]` or `(music)`.
fn is_non_speech(text: &str) -> bool {
    let t = text.trim();
    t.is_empty()
        || ((t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')')))
            && !t[1..t.len() - 1].contains(['[', '('])
}

impl Transcriber for WhisperTranscriber {
    fn model_name(&self) -> String {
        self.model_name.clone()
    }

    fn transcribe_samples(
        &self,
        samples: &[f32],
        track: Track,
        opts: &TranscribeOptions,
        progress: Option<ProgressFn>,
    ) -> Result<Vec<Segment>> {
        // whisper.cpp needs at least ~1 s of audio; pad short clips with silence.
        const MIN_SAMPLES: usize = 16_000 + 1_600;
        let padded;
        let audio: &[f32] = if samples.len() < MIN_SAMPLES {
            padded = {
                let mut v = samples.to_vec();
                v.resize(MIN_SAMPLES, 0.0);
                v
            };
            &padded
        } else {
            samples
        };

        let ctx = self.context()?;
        let mut state = ctx
            .create_state()
            .map_err(|e| AsrError::Whisper(e.to_string()))?;

        let strategy = match opts.beam_size {
            Some(b) if b > 1 => SamplingStrategy::BeamSearch {
                beam_size: b as i32,
                patience: -1.0,
            },
            _ => SamplingStrategy::Greedy { best_of: 1 },
        };
        let mut params = FullParams::new(strategy);
        params.set_n_threads(
            opts.threads
                .map(|t| t.max(1) as i32)
                .unwrap_or_else(default_threads),
        );
        params.set_language(Some(opts.language.as_deref().unwrap_or("auto")));
        if let Some(prompt) = opts
            .initial_prompt
            .as_deref()
            .filter(|p| !p.trim().is_empty())
        {
            params.set_initial_prompt(prompt);
        }
        params.set_translate(false);
        params.set_no_context(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // `set_progress_callback_safe` leaks its boxed closure on every call (whisper-rs
        // never frees it), so use the raw API like the abort callback below: user data
        // points at `progress`, which lives on this stack frame until after `state.full`.
        if let Some(cb) = progress.as_ref() {
            // SAFETY: `progress_trampoline` only calls the `ProgressFn` behind the pointer,
            // and `progress` outlives `state.full` (it is used again after it returns).
            unsafe {
                params.set_progress_callback(Some(progress_trampoline));
                params.set_progress_callback_user_data(cb as *const ProgressFn as *mut c_void);
            }
        }
        // whisper-rs 0.16's `set_abort_callback_safe` casts its user data to the wrong
        // type (it boxes a `Box<dyn FnMut>` but the trampoline reads it as `F`), so the
        // callback returns garbage and whisper aborts with "failed to encode". Use the raw
        // API instead: user data is a pointer to the `AtomicBool` inside `opts.cancel`,
        // which outlives `state.full` because `opts` is borrowed for this whole call.
        if let Some(cancel) = opts.cancel.as_ref() {
            // SAFETY: `abort_trampoline` only reads an `AtomicBool` through the pointer,
            // and the pointee is kept alive by `opts.cancel` until after `state.full` returns.
            unsafe {
                params.set_abort_callback(Some(abort_trampoline));
                params.set_abort_callback_user_data(Arc::as_ptr(cancel) as *mut c_void);
            }
        }

        let result = state.full(params, audio);
        if opts
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
        {
            return Err(AsrError::Cancelled);
        }
        result.map_err(|e| AsrError::Whisper(e.to_string()))?;

        let len_ms = samples.len() as u64 * 1000 / crate::TARGET_SAMPLE_RATE as u64;
        let mut out = Vec::new();
        for seg in state.as_iter() {
            let text = seg
                .to_str_lossy()
                .map_err(|e| AsrError::Whisper(e.to_string()))?;
            if is_non_speech(&text) {
                continue;
            }
            // whisper timestamps are centiseconds.
            let start_ms = (seg.start_timestamp().max(0) as u64 * 10).min(len_ms);
            let end_ms =
                (seg.end_timestamp().max(0) as u64 * 10).clamp(start_ms, len_ms.max(start_ms));
            out.push(Segment {
                start_ms,
                end_ms,
                text: text.trim().to_string(),
                track,
                speaker_id: None,
            });
        }
        if let Some(cb) = progress {
            cb(1.0);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_speech_markers() {
        assert!(is_non_speech(" [BLANK_AUDIO]"));
        assert!(is_non_speech("(music)"));
        assert!(is_non_speech("   "));
        assert!(!is_non_speech("Hello (world)"));
        assert!(!is_non_speech("[a] and [b]"));
    }

    #[test]
    fn progress_trampoline_calls_closure_without_leaking() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let cb: ProgressFn = Arc::new(move |f| s2.lock().unwrap().push(f));
        let ptr = &cb as *const ProgressFn as *mut c_void;
        for pct in [0, 50, 150] {
            unsafe { progress_trampoline(std::ptr::null_mut(), std::ptr::null_mut(), pct, ptr) };
        }
        unsafe {
            progress_trampoline(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                5,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(*seen.lock().unwrap(), vec![0.0, 0.5, 1.0]);
        // Nothing else holds the closure (the old `_safe` API kept a leaked Box of it).
        assert_eq!(Arc::strong_count(&cb), 1);
    }

    #[test]
    fn abort_trampoline_reads_flag() {
        let flag = Arc::new(AtomicBool::new(false));
        let ptr = Arc::as_ptr(&flag) as *mut c_void;
        assert!(!unsafe { abort_trampoline(ptr) });
        flag.store(true, Ordering::Relaxed);
        assert!(unsafe { abort_trampoline(ptr) });
        assert!(!unsafe { abort_trampoline(std::ptr::null_mut()) });
    }

    /// Regression for the whisper-rs `set_abort_callback_safe` bug: passing an unset
    /// cancel flag must not abort, and a set flag must yield `Cancelled`.
    /// `MINUTAE_TEST_MODEL=/path/ggml-tiny.bin cargo test -p minutae-asr -- --ignored`
    #[test]
    #[ignore]
    fn real_model_cancel_flag() {
        let Ok(path) = std::env::var("MINUTAE_TEST_MODEL") else {
            return;
        };
        let t = WhisperTranscriber::new(path);
        let flag = Arc::new(AtomicBool::new(false));
        let opts = TranscribeOptions {
            cancel: Some(flag.clone()),
            ..Default::default()
        };
        let audio: Vec<f32> = (0..16_000 * 3)
            .map(|i| (i as f32 * 0.05).sin() * 0.1)
            .collect();
        t.transcribe_samples(&audio, Track::Mic, &opts, None)
            .expect("unset cancel flag must not abort");
        flag.store(true, Ordering::Relaxed);
        let err = t
            .transcribe_samples(&audio, Track::Mic, &opts, None)
            .unwrap_err();
        assert!(matches!(err, AsrError::Cancelled), "{err:?}");
        assert!(t.unload_and_wait(std::time::Duration::from_secs(5)));
        assert!(!t.is_loaded());
    }

    #[test]
    fn missing_model_is_reported() {
        let dir = std::env::temp_dir().join("minutae-asr-missing");
        let m = ModelManager::with_dir(&dir);
        assert!(matches!(
            WhisperTranscriber::from_manager(&m, WhisperModel::Tiny),
            Err(AsrError::ModelMissing(_))
        ));
        let t = WhisperTranscriber::new(dir.join("ggml-tiny.bin"));
        assert_eq!(t.model_name(), "whisper-tiny");
        let err = t
            .transcribe_samples(&[0.0; 100], Track::Mic, &TranscribeOptions::default(), None)
            .unwrap_err();
        assert!(
            matches!(err, AsrError::ModelMissing(ref m) if m == "tiny"),
            "{err:?}"
        );
    }

    /// Real model smoke test: `MINUTAE_TEST_MODEL=/path/ggml-tiny.bin cargo test -p minutae-asr -- --ignored`
    #[test]
    #[ignore]
    fn real_model_on_silence() {
        let Ok(path) = std::env::var("MINUTAE_TEST_MODEL") else {
            return;
        };
        let t = WhisperTranscriber::new(path);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let progress: ProgressFn = Arc::new(move |f| s2.lock().unwrap().push(f));
        let segs = t
            .transcribe_samples(
                &vec![0.0; 16_000 * 2],
                Track::Mic,
                &TranscribeOptions::default(),
                Some(progress.clone()),
            )
            .unwrap();
        assert!(segs.iter().all(|s| !s.text.is_empty()));
        assert!(t.is_loaded());
        // Progress arrives through the raw callback and ends at 1.0; nothing leaked a
        // reference to the closure.
        assert_eq!(seen.lock().unwrap().last(), Some(&1.0));
        assert_eq!(Arc::strong_count(&progress), 1);
    }
}
