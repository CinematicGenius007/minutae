//! Which processes make up "the call app" for per-app system audio.
//!
//! The mic monitor tells us one process (pid + bundle id) that is using the microphone.
//! Its audio output may come from that process or from helpers (Chrome's audio service,
//! Zoom's helpers, WebKit's GPU process for Safari). [`resolve`] picks every audio process
//! that belongs to the same app. The OS lookups sit behind [`ProcessTable`] so the
//! matching is tested with a fake table.

// The matching runs only on macOS today; it stays compiled and tested everywhere.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use serde::{Deserialize, Serialize};

/// The app whose audio should be recorded as the system track. Only macOS uses it today;
/// other platforms record all system audio.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTarget {
    /// A process of the app (usually the one using the microphone).
    pub pid: Option<i32>,
    /// Its bundle id, e.g. `us.zoom.xos` or `com.google.Chrome.helper`.
    pub bundle_id: Option<String>,
}

impl AppTarget {
    pub fn is_empty(&self) -> bool {
        self.pid.is_none() && self.bundle_id.as_deref().is_none_or(str::is_empty)
    }
}

/// What the system track currently contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SystemScope {
    /// Only the call app (and its helpers).
    App,
    /// Everything the computer plays (music and videos included).
    All,
}

/// How the system track is captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SystemBackend {
    /// macOS 14.4+: a Core Audio process tap on the call app's processes.
    ProcessTap,
    /// macOS 13.0 - 14.3: ScreenCaptureKit audio (needs Screen Recording).
    ScreenCapture,
    /// The OS output loopback: a global Core Audio tap on macOS, WASAPI loopback on
    /// Windows, the PulseAudio/PipeWire monitor on Linux.
    Loopback,
}

/// The live system-audio source of a recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSource {
    pub scope: SystemScope,
    pub backend: SystemBackend,
}

/// One process known to the audio system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioProcess {
    /// The OS handle to pass to the capture API (Core Audio process object, or the pid
    /// for ScreenCaptureKit).
    pub handle: u32,
    pub pid: i32,
    pub bundle_id: Option<String>,
    pub parent: Option<i32>,
    /// The app macOS holds responsible for the process (helpers -> their app).
    pub responsible: Option<i32>,
}

/// OS lookups needed to resolve an [`AppTarget`].
pub trait ProcessTable {
    /// Processes that can be captured right now.
    fn processes(&self) -> Vec<AudioProcess>;
    /// The app responsible for `pid` (itself for a regular app).
    fn responsible_pid(&self, pid: i32) -> Option<i32>;
}

/// Bundle ids shared by many apps' helpers: matching them by id would pull in other apps.
const SHARED_BUNDLE_PREFIXES: &[&str] = &["com.apple.WebKit"];

fn is_shared_bundle(id: &str) -> bool {
    SHARED_BUNDLE_PREFIXES.iter().any(|p| id.starts_with(p))
}

/// The app's own bundle id from a helper's: everything before the first segment that
/// names a helper (`com.google.Chrome.helper.Renderer` -> `com.google.Chrome`).
pub fn app_bundle_root(bundle_id: &str) -> &str {
    let mut end = bundle_id.len();
    let mut pos = 0;
    for seg in bundle_id.split('.') {
        if pos > 0 && seg.to_ascii_lowercase().contains("helper") {
            end = pos - 1;
            break;
        }
        pos += seg.len() + 1;
    }
    &bundle_id[..end]
}

/// Handles of every capturable process that belongs to `target`'s app, sorted and
/// de-duplicated. `own_pid` is never included. Empty = the app has no audio process yet.
pub fn resolve(target: &AppTarget, table: &dyn ProcessTable, own_pid: i32) -> Vec<u32> {
    let procs = table.processes();
    // A bundled helper (Chrome helper, WebKit GPU) belongs to its responsible app. A
    // command-line tool's responsible app is the terminal that started it, so a pid without
    // a bundle id anchors on itself only (else every sibling would be recorded).
    let bundled = target.bundle_id.as_deref().is_some_and(|b| !b.is_empty());
    let anchor = target.pid.map(|p| {
        if bundled {
            table.responsible_pid(p).filter(|r| *r > 1).unwrap_or(p)
        } else {
            p
        }
    });
    let root = target
        .bundle_id
        .as_deref()
        .filter(|b| !b.is_empty() && !is_shared_bundle(b))
        .map(app_bundle_root)
        // A one-segment id ("zoom") is too vague to prefix-match.
        .filter(|r| r.contains('.'));
    let mut out: Vec<u32> = procs
        .iter()
        .filter(|p| p.pid != own_pid)
        .filter(|p| {
            let by_pid = target.pid.is_some_and(|t| p.pid == t)
                || anchor
                    .is_some_and(|a| p.pid == a || p.parent == Some(a) || p.responsible == Some(a));
            let by_bundle = root.is_some_and(|r| {
                p.bundle_id
                    .as_deref()
                    .is_some_and(|b| b == r || (b.starts_with(r) && b[r.len()..].starts_with('.')))
            });
            by_pid || by_bundle
        })
        .map(|p| p.handle)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// What to do with a running system stream after resolving the target again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retarget {
    Keep,
    /// Rebuild the per-app capture on these handles.
    To(Vec<u32>),
}

/// `current` is `None` while all system audio is being recorded (fallback), or the
/// handles of the running per-app capture. A per-app capture is rebuilt only when a new
/// process of the app appears (an exited one costs nothing), so it does not churn.
pub fn retarget(current: Option<&[u32]>, resolved: &[u32]) -> Retarget {
    if resolved.is_empty() {
        return Retarget::Keep;
    }
    match current {
        None => Retarget::To(resolved.to_vec()),
        Some(cur) if resolved.iter().any(|h| !cur.contains(h)) => Retarget::To(resolved.to_vec()),
        Some(_) => Retarget::Keep,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A fake process table.
    #[derive(Default)]
    pub struct FakeTable {
        pub procs: Vec<AudioProcess>,
        pub responsible: HashMap<i32, i32>,
    }

    impl FakeTable {
        pub fn add(&mut self, handle: u32, pid: i32, bundle: &str, parent: i32, resp: i32) {
            self.procs.push(AudioProcess {
                handle,
                pid,
                bundle_id: (!bundle.is_empty()).then(|| bundle.to_string()),
                parent: Some(parent),
                responsible: Some(resp),
            });
            self.responsible.insert(pid, resp);
        }
    }

    impl ProcessTable for FakeTable {
        fn processes(&self) -> Vec<AudioProcess> {
            self.procs.clone()
        }
        fn responsible_pid(&self, pid: i32) -> Option<i32> {
            self.responsible.get(&pid).copied()
        }
    }

    fn target(pid: i32, bundle: &str) -> AppTarget {
        AppTarget {
            pid: Some(pid),
            bundle_id: Some(bundle.into()),
        }
    }

    #[test]
    fn bundle_root_strips_helpers() {
        assert_eq!(
            app_bundle_root("com.google.Chrome.helper"),
            "com.google.Chrome"
        );
        assert_eq!(
            app_bundle_root("com.google.Chrome.helper.Renderer"),
            "com.google.Chrome"
        );
        assert_eq!(app_bundle_root("us.zoom.xos"), "us.zoom.xos");
        assert_eq!(
            app_bundle_root("com.tinyspeck.slackmacgap.helper"),
            "com.tinyspeck.slackmacgap"
        );
        assert_eq!(app_bundle_root("helper.app"), "helper.app");
    }

    #[test]
    fn chrome_helpers_are_included_but_other_apps_are_not() {
        let mut t = FakeTable::default();
        t.add(10, 100, "com.google.Chrome", 1, 100);
        t.add(11, 101, "com.google.Chrome.helper", 100, 100); // audio service (uses the mic)
        t.add(12, 102, "com.google.Chrome.helper.Renderer", 100, 100);
        t.add(20, 200, "com.spotify.client", 1, 200);
        t.add(21, 201, "com.apple.Music", 1, 201);
        t.add(30, 300, "com.example.minutae", 1, 300); // ourselves
        let got = resolve(&target(101, "com.google.Chrome.helper"), &t, 300);
        assert_eq!(got, vec![10, 11, 12]);
    }

    #[test]
    fn safari_matches_by_responsible_app_not_shared_webkit_id() {
        let mut t = FakeTable::default();
        t.add(10, 100, "com.apple.Safari", 1, 100);
        t.add(11, 101, "com.apple.WebKit.GPU", 1, 100); // Safari's GPU process (mic + output)
        t.add(20, 200, "com.apple.mail", 1, 200);
        t.add(21, 201, "com.apple.WebKit.GPU", 1, 200); // Mail's: must not be recorded
        let got = resolve(&target(101, "com.apple.WebKit.GPU"), &t, 999);
        assert_eq!(got, vec![10, 11]);
    }

    #[test]
    fn zoom_children_by_parent_and_no_bundle_cli() {
        let mut t = FakeTable::default();
        t.add(10, 100, "us.zoom.xos", 1, 100);
        t.add(11, 150, "", 100, 150); // helper without bundle id, child of Zoom
        t.add(20, 200, "com.spotify.client", 1, 200);
        assert_eq!(resolve(&target(100, "us.zoom.xos"), &t, 999), vec![10, 11]);
        // Only a pid known (no bundle id): the process itself.
        let pid_only = AppTarget {
            pid: Some(200),
            bundle_id: None,
        };
        assert_eq!(resolve(&pid_only, &t, 999), vec![20]);
    }

    #[test]
    fn cli_target_does_not_pull_in_terminal_siblings() {
        // Two command-line players started from the same terminal (pid 50): macOS holds
        // the terminal responsible for both.
        let mut t = FakeTable::default();
        t.add(10, 100, "", 60, 50);
        t.add(11, 101, "", 60, 50);
        t.add(12, 102, "", 100, 50); // a child of the target
        let pid_only = AppTarget {
            pid: Some(100),
            bundle_id: None,
        };
        assert_eq!(resolve(&pid_only, &t, 999), vec![10, 12]);
    }

    #[test]
    fn nothing_matches_when_the_app_has_no_audio_process() {
        let mut t = FakeTable::default();
        t.add(20, 200, "com.spotify.client", 1, 200);
        assert!(resolve(&target(100, "us.zoom.xos"), &t, 999).is_empty());
        assert!(resolve(&AppTarget::default(), &t, 999).is_empty());
        assert!(AppTarget::default().is_empty());
    }

    #[test]
    fn retarget_only_when_new_processes_appear() {
        assert_eq!(retarget(None, &[]), Retarget::Keep);
        assert_eq!(retarget(None, &[3]), Retarget::To(vec![3]));
        assert_eq!(retarget(Some(&[3]), &[3]), Retarget::Keep);
        assert_eq!(retarget(Some(&[3, 4]), &[3]), Retarget::Keep); // 4 exited
        assert_eq!(retarget(Some(&[3]), &[3, 5]), Retarget::To(vec![3, 5]));
        assert_eq!(retarget(Some(&[3]), &[]), Retarget::Keep);
    }

    #[test]
    fn source_json_shape() {
        let s = SystemSource {
            scope: SystemScope::App,
            backend: SystemBackend::ProcessTap,
        };
        assert_eq!(
            serde_json::to_value(s).unwrap(),
            serde_json::json!({"scope": "app", "backend": "processTap"})
        );
    }
}
