//! In-app updates: check GitHub Releases' `latest.json`, download, verify, install, restart.
//!
//! [`Updates`] owns the state machine and emits `update://status` on every change. The
//! network and installer sit behind [`UpdateSource`]; [`TauriUpdateSource`] wraps
//! `tauri-plugin-updater`, tests use a fake. A build without an updater public key
//! (`plugins.updater.pubkey` empty in tauri.conf.json, the default until the owner adds
//! one) has no source: the phase stays `disabled` and nothing touches the network.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tauri::{AppHandle, Emitter, Manager};

use crate::dto::{UpdatePhase, UpdateStatus, ev};

/// A newer release found by [`UpdateSource::check`].
#[derive(Debug, Clone, PartialEq)]
pub struct Available {
    pub version: String,
    pub notes: Option<String>,
}

/// Progress callback: fraction 0..1, or `None` when the download size is unknown.
pub type OnProgress = Box<dyn FnMut(Option<f32>) + Send>;

#[async_trait]
pub trait UpdateSource: Send + Sync + 'static {
    /// Ask the update server whether a newer version exists. `Err` is one plain sentence.
    async fn check(&self) -> Result<Option<Available>, String>;
    /// Download, verify and install the release found by the last successful `check`.
    /// On Windows the installer takes over and this process exits before returning.
    async fn install(&self, progress: OnProgress) -> Result<(), String>;
}

type Emit = Arc<dyn Fn(&UpdateStatus) + Send + Sync>;

pub struct Updates {
    source: Option<Arc<dyn UpdateSource>>,
    status: Mutex<UpdateStatus>,
    emit: Emit,
}

impl Updates {
    pub fn new(current_version: String, source: Option<Arc<dyn UpdateSource>>, emit: Emit) -> Self {
        let phase = if source.is_some() {
            UpdatePhase::Idle
        } else {
            UpdatePhase::Disabled
        };
        Self {
            source,
            status: Mutex::new(UpdateStatus {
                phase,
                current_version,
                version: None,
                notes: None,
                fraction: None,
                error: None,
            }),
            emit,
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn set(&self, f: impl FnOnce(&mut UpdateStatus)) -> UpdateStatus {
        let s = {
            let mut g = self.status.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut g);
            g.clone()
        };
        (self.emit)(&s);
        s
    }

    /// Moves to `checking` and returns at once with that status, or returns the current
    /// status unchanged when disabled or already busy. Call [`Self::run_check`] next.
    pub fn begin_check(&self) -> Option<UpdateStatus> {
        self.source.as_ref()?;
        {
            let g = self.status.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(g.phase, UpdatePhase::Checking | UpdatePhase::Installing) {
                return None;
            }
        }
        Some(self.set(|s| {
            s.phase = UpdatePhase::Checking;
            s.error = None;
            s.fraction = None;
        }))
    }

    /// Runs the check started by [`Self::begin_check`].
    pub async fn run_check(&self) -> UpdateStatus {
        let Some(source) = self.source.clone() else {
            return self.status();
        };
        match source.check().await {
            Ok(Some(a)) => self.set(|s| {
                s.phase = UpdatePhase::Available;
                s.version = Some(a.version);
                s.notes = a.notes;
            }),
            Ok(None) => self.set(|s| {
                s.phase = UpdatePhase::UpToDate;
                s.version = None;
                s.notes = None;
            }),
            Err(e) => self.set(|s| {
                s.phase = UpdatePhase::Error;
                s.version = None;
                s.notes = None;
                s.error = Some(e);
            }),
        }
    }

    /// `available` → `installing`. `Err` when there is nothing to install.
    pub fn begin_install(&self) -> Result<UpdateStatus, String> {
        if self.source.is_none() {
            return Err("Updates are turned off in this build.".into());
        }
        {
            let g = self.status.lock().unwrap_or_else(|e| e.into_inner());
            if g.phase != UpdatePhase::Available {
                return Err("No update is ready to install. Check for updates first.".into());
            }
        }
        Ok(self.set(|s| {
            s.phase = UpdatePhase::Installing;
            s.fraction = Some(0.0);
            s.error = None;
        }))
    }

    /// Runs the install started by [`Self::begin_install`]. `Ok` means the new version is
    /// in place and the app should restart.
    pub async fn run_install(self: &Arc<Self>) -> Result<(), String> {
        let Some(source) = self.source.clone() else {
            return Err("Updates are turned off in this build.".into());
        };
        let me = self.clone();
        let mut last = Instant::now() - Duration::from_secs(1);
        let progress: OnProgress = Box::new(move |f| {
            // ~10 events a second is plenty for a progress bar.
            if last.elapsed() >= Duration::from_millis(100) {
                last = Instant::now();
                me.set(|s| s.fraction = f);
            }
        });
        match source.install(progress).await {
            Ok(()) => {
                self.set(|s| s.fraction = Some(1.0));
                Ok(())
            }
            Err(e) => {
                self.set(|s| {
                    s.phase = UpdatePhase::Error;
                    s.fraction = None;
                    s.error = Some(e.clone());
                });
                Err(e)
            }
        }
    }
}

/// The updater public key from tauri.conf.json (`plugins.updater.pubkey`), if set.
pub fn configured_pubkey(app: &AppHandle) -> Option<String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(String::from)
}

/// Build the managed [`Updates`]: real source when a pubkey is configured, else disabled.
pub fn init(app: &AppHandle) -> Arc<Updates> {
    let source: Option<Arc<dyn UpdateSource>> = if configured_pubkey(app).is_some() {
        Some(Arc::new(TauriUpdateSource {
            app: app.clone(),
            pending: Mutex::new(None),
        }))
    } else {
        tracing::info!("updater disabled: no public key in this build");
        None
    };
    let a = app.clone();
    let emit: Emit = Arc::new(move |s| {
        let _ = a.emit(ev::UPDATE_STATUS, s);
    });
    Arc::new(Updates::new(
        app.package_info().version.to_string(),
        source,
        emit,
    ))
}

pub struct TauriUpdateSource {
    app: AppHandle,
    pending: Mutex<Option<tauri_plugin_updater::Update>>,
}

fn friendly(e: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Reqwest(_) | E::Network(_) => {
            "Couldn't reach the update server. Check your connection and try again.".into()
        }
        E::ReleaseNotFound => "No published release was found.".into(),
        E::TargetNotFound(_) | E::TargetsNotFound(_) => {
            "The latest release has no download for this system.".into()
        }
        E::Minisign(_) | E::Base64(_) | E::SignatureUtf8(_) => {
            "The download didn't match its signature, so it wasn't installed.".into()
        }
        _ => "Couldn't update. Try again later or download the latest version from GitHub.".into(),
    }
}

#[async_trait]
impl UpdateSource for TauriUpdateSource {
    async fn check(&self) -> Result<Option<Available>, String> {
        use tauri_plugin_updater::UpdaterExt;
        let app = self.app.clone();
        let updater = self
            .app
            .updater_builder()
            .timeout(Duration::from_secs(30))
            // Windows: the installer exits this process; free whisper's GPU contexts first.
            .on_before_exit(move || {
                if let Some(st) = app.try_state::<crate::state::AppState>() {
                    st.transcribers.shutdown(Duration::from_secs(5));
                }
            })
            .build()
            .map_err(|e| {
                tracing::warn!(error = %e, "updater build");
                friendly(&e)
            })?;
        let found = updater.check().await.map_err(|e| {
            tracing::warn!(error = %e, "update check");
            friendly(&e)
        })?;
        let out = found.as_ref().map(|u| Available {
            version: u.version.clone(),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        });
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = found;
        Ok(out)
    }

    async fn install(&self, mut progress: OnProgress) -> Result<(), String> {
        let update = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| "No update is ready to install.".to_string())?;
        let mut got: u64 = 0;
        update
            .download_and_install(
                |chunk, total| {
                    got += chunk as u64;
                    progress(
                        total
                            .filter(|t| *t > 0)
                            .map(|t| (got as f32 / t as f32).min(1.0)),
                    );
                },
                || {},
            )
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "update install");
                friendly(&e)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        check: Mutex<Result<Option<Available>, String>>,
        install: Result<(), String>,
        installs: AtomicUsize,
    }

    #[async_trait]
    impl UpdateSource for Fake {
        async fn check(&self) -> Result<Option<Available>, String> {
            self.check.lock().unwrap().clone()
        }
        async fn install(&self, mut progress: OnProgress) -> Result<(), String> {
            self.installs.fetch_add(1, Ordering::SeqCst);
            progress(Some(0.5));
            self.install.clone()
        }
    }

    fn fake(
        check: Result<Option<Available>, String>,
        install: Result<(), String>,
    ) -> (Arc<Updates>, Arc<Fake>, Arc<Mutex<Vec<UpdatePhase>>>) {
        let src = Arc::new(Fake {
            check: Mutex::new(check),
            install,
            installs: AtomicUsize::new(0),
        });
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let u = Updates::new(
            "0.4.0".into(),
            Some(src.clone() as Arc<dyn UpdateSource>),
            Arc::new(move |s: &UpdateStatus| s2.lock().unwrap().push(s.phase)),
        );
        (Arc::new(u), src, seen)
    }

    fn v(version: &str) -> Available {
        Available {
            version: version.into(),
            notes: Some("Fixes".into()),
        }
    }

    #[tokio::test]
    async fn without_a_source_everything_is_disabled() {
        let u = Updates::new("0.4.0".into(), None, Arc::new(|_: &UpdateStatus| {}));
        assert_eq!(u.status().phase, UpdatePhase::Disabled);
        assert!(u.begin_check().is_none());
        assert_eq!(u.run_check().await.phase, UpdatePhase::Disabled);
        assert!(u.begin_install().is_err());
    }

    #[tokio::test]
    async fn check_reports_up_to_date() {
        let (u, _, seen) = fake(Ok(None), Ok(()));
        assert_eq!(u.status().phase, UpdatePhase::Idle);
        assert_eq!(u.begin_check().unwrap().phase, UpdatePhase::Checking);
        let s = u.run_check().await;
        assert_eq!(s.phase, UpdatePhase::UpToDate);
        assert_eq!(s.current_version, "0.4.0");
        assert_eq!(
            *seen.lock().unwrap(),
            [UpdatePhase::Checking, UpdatePhase::UpToDate]
        );
    }

    #[tokio::test]
    async fn check_reports_available_then_installs() {
        let (u, src, _) = fake(Ok(Some(v("0.5.0"))), Ok(()));
        u.begin_check();
        let s = u.run_check().await;
        assert_eq!(s.phase, UpdatePhase::Available);
        assert_eq!(s.version.as_deref(), Some("0.5.0"));
        assert_eq!(s.notes.as_deref(), Some("Fixes"));

        assert_eq!(u.begin_install().unwrap().phase, UpdatePhase::Installing);
        // A second check while installing is ignored.
        assert!(u.begin_check().is_none());
        u.run_install().await.unwrap();
        let s = u.status();
        assert_eq!(s.phase, UpdatePhase::Installing);
        assert_eq!(s.fraction, Some(1.0));
        assert_eq!(src.installs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn check_error_is_shown_and_can_be_retried() {
        let (u, src, _) = fake(Err("offline".into()), Ok(()));
        u.begin_check();
        let s = u.run_check().await;
        assert_eq!(s.phase, UpdatePhase::Error);
        assert_eq!(s.error.as_deref(), Some("offline"));

        *src.check.lock().unwrap() = Ok(None);
        assert!(u.begin_check().unwrap().error.is_none());
        assert_eq!(u.run_check().await.phase, UpdatePhase::UpToDate);
    }

    #[tokio::test]
    async fn install_needs_an_available_update() {
        let (u, src, _) = fake(Ok(None), Ok(()));
        assert!(u.begin_install().is_err());
        u.begin_check();
        u.run_check().await;
        assert!(u.begin_install().is_err());
        assert_eq!(src.installs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_install_ends_in_error() {
        let (u, _, _) = fake(Ok(Some(v("0.5.0"))), Err("bad signature".into()));
        u.begin_check();
        u.run_check().await;
        u.begin_install().unwrap();
        assert!(u.run_install().await.is_err());
        let s = u.status();
        assert_eq!(s.phase, UpdatePhase::Error);
        assert_eq!(s.error.as_deref(), Some("bad signature"));
        assert_eq!(s.fraction, None);
    }
}
