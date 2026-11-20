//! Windows: Capability Access Manager consent store. Every app that has used
//! the mic has a subkey with `LastUsedTimeStart` / `LastUsedTimeStop`
//! (FILETIME); `LastUsedTimeStop == 0` means it's using the mic right now.

use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use crate::names::{consent_entry_in_use, from_windows_key, windows_key_for_path};
use crate::{DetectError, MicActivity, MicMonitor, Result};

const STORE: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

pub struct ConsentStoreMonitor {
    own_key: Option<String>,
}

impl ConsentStoreMonitor {
    pub fn new() -> Self {
        let own_key = std::env::current_exe()
            .ok()
            .map(|p| windows_key_for_path(&p.to_string_lossy()).to_ascii_lowercase());
        Self { own_key }
    }
}

fn in_use(key: &RegKey) -> bool {
    let start = key.get_value::<u64, _>("LastUsedTimeStart").ok();
    let stop = key.get_value::<u64, _>("LastUsedTimeStop").ok();
    consent_entry_in_use(start, stop)
}

impl MicMonitor for ConsentStoreMonitor {
    fn poll(&mut self) -> Result<MicActivity> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let store = hkcu
            .open_subkey(STORE)
            .map_err(|e| DetectError::Os(format!("consent store unavailable: {e}")))?;
        let mut apps = Vec::new();
        for name in store.enum_keys().flatten() {
            if name.eq_ignore_ascii_case("NonPackaged") {
                let Ok(np) = store.open_subkey(&name) else {
                    continue;
                };
                for exe_key in np.enum_keys().flatten() {
                    if self.own_key.as_deref() == Some(exe_key.to_ascii_lowercase().as_str()) {
                        continue;
                    }
                    if np
                        .open_subkey(&exe_key)
                        .map(|k| in_use(&k))
                        .unwrap_or(false)
                    {
                        apps.push(from_windows_key(&exe_key, true));
                    }
                }
            } else if store
                .open_subkey(&name)
                .map(|k| in_use(&k))
                .unwrap_or(false)
            {
                apps.push(from_windows_key(&name, false));
            }
        }
        apps.dedup();
        Ok(if apps.is_empty() {
            MicActivity::free()
        } else {
            MicActivity::busy(apps)
        })
    }
}
