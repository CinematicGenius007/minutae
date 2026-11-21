//! `org.freedesktop.portal.GlobalShortcuts` over D-Bus (ashpd + zbus, pure Rust).
//! Linux only; compiled and exercised by CI, tests use `crate::shortcuts::FakePortal`.

use std::time::Instant;

use ashpd::desktop::global_shortcuts::{
    BindShortcutsOptions, ConfigureShortcutsOptions, GlobalShortcuts, NewShortcut,
};
use ashpd::desktop::{CreateSessionOptions, ResponseError, Session};
use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::{Mutex, mpsc};

use crate::shortcuts::{
    BoundShortcut, PortalBinding, PortalError, ShortcutId, ShortcutPortal, ShortcutRequest,
    ShortcutSignal,
};

/// The session and proxy stay alive for the life of the app: closing the session
/// unbinds the shortcuts.
pub struct XdgShortcutPortal {
    live: Mutex<Option<Live>>,
}

struct Live {
    proxy: GlobalShortcuts,
    session: Session<GlobalShortcuts>,
}

impl Default for XdgShortcutPortal {
    fn default() -> Self {
        Self {
            live: Mutex::new(None),
        }
    }
}

fn map_err(e: ashpd::Error) -> PortalError {
    match e {
        ashpd::Error::PortalNotFound(i) => PortalError::Unavailable(i.to_string()),
        ashpd::Error::Response(ResponseError::Cancelled) => PortalError::Cancelled,
        ashpd::Error::Zbus(zbus_err) => {
            let text = zbus_err.to_string();
            // No session bus, no portal service, or no GlobalShortcuts backend.
            if text.contains("ServiceUnknown")
                || text.contains("UnknownInterface")
                || text.contains("UnknownMethod")
                || text.contains("DBUS_SESSION_BUS_ADDRESS")
            {
                PortalError::Unavailable(text)
            } else {
                PortalError::Failed(text)
            }
        }
        other => PortalError::Failed(other.to_string()),
    }
}

fn stamp(ts: std::time::Duration, start: Instant) -> u64 {
    let ms = ts.as_millis() as u64;
    if ms > 0 {
        ms
    } else {
        start.elapsed().as_millis() as u64
    }
}

#[async_trait]
impl ShortcutPortal for XdgShortcutPortal {
    async fn bind(&self, requests: &[ShortcutRequest]) -> Result<PortalBinding, PortalError> {
        let proxy = GlobalShortcuts::new().await.map_err(map_err)?;
        let session = proxy
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(map_err)?;
        // Subscribe before binding so no early press is lost.
        let mut activated = proxy.receive_activated().await.map_err(map_err)?;
        let mut deactivated = proxy.receive_deactivated().await.map_err(map_err)?;
        let new: Vec<NewShortcut> = requests
            .iter()
            .map(|r| {
                NewShortcut::new(r.id.as_str(), r.description.as_str())
                    .preferred_trigger(r.preferred_trigger.as_deref())
            })
            .collect();
        let response = proxy
            .bind_shortcuts(&session, &new, None, BindShortcutsOptions::default())
            .await
            .map_err(map_err)?
            .response()
            .map_err(map_err)?;
        let shortcuts = response
            .shortcuts()
            .iter()
            .filter_map(|s| {
                Some(BoundShortcut {
                    id: ShortcutId::parse(s.id())?,
                    trigger: s.trigger_description().to_string(),
                })
            })
            .collect();
        let can_configure = proxy.version() >= 2;

        let (tx, signals) = mpsc::unbounded_channel();
        let start = Instant::now();
        tokio::spawn(async move {
            loop {
                let signal = tokio::select! {
                    Some(a) = activated.next() => ShortcutId::parse(a.shortcut_id())
                        .map(|id| ShortcutSignal::Activated { id, at_ms: stamp(a.timestamp(), start) }),
                    Some(d) = deactivated.next() => ShortcutId::parse(d.shortcut_id())
                        .map(|id| ShortcutSignal::Deactivated { id, at_ms: stamp(d.timestamp(), start) }),
                    else => break,
                };
                if let Some(s) = signal
                    && tx.send(s).is_err()
                {
                    break;
                }
            }
            tracing::info!("global shortcuts portal signal stream ended");
        });

        *self.live.lock().await = Some(Live { proxy, session });
        Ok(PortalBinding {
            shortcuts,
            can_configure,
            signals,
        })
    }

    async fn configure(&self) -> Result<(), PortalError> {
        let live = self.live.lock().await;
        let Some(live) = live.as_ref() else {
            return Err(PortalError::Failed("shortcuts are not bound".into()));
        };
        live.proxy
            .configure_shortcuts(&live.session, None, ConfigureShortcutsOptions::default())
            .await
            .map_err(map_err)
    }
}
