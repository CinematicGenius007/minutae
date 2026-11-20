//! Windows (main, HUD, call prompt), tray menu and global shortcuts.

use std::str::FromStr;

use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use minutae_core::AppSettings;

use crate::dto::{self, RecordingPhase, RecordingState, ev};
use crate::state::AppState;
use crate::util;

pub const MAIN: &str = "main";
pub const HUD: &str = "hud";
pub const PROMPT: &str = "prompt";
const TRAY_ID: &str = "minutae";

const HUD_SIZE: (f64, f64) = (320.0, 64.0);
const PROMPT_SIZE: (f64, f64) = (380.0, 200.0);

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(MinutaePanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

/// Create the hidden HUD and prompt windows. On macOS they become non-activating
/// `NSPanel`s so showing them never steals focus from the app you are typing in.
pub fn create_aux_windows(app: &AppHandle) -> tauri::Result<()> {
    for (label, route, (w, h)) in [(HUD, "hud", HUD_SIZE), (PROMPT, "prompt", PROMPT_SIZE)] {
        let window = WebviewWindowBuilder::new(
            app,
            label,
            WebviewUrl::App(format!("index.html#/{route}").into()),
        )
        .title("Minutae")
        .inner_size(w, h)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;
        #[cfg(target_os = "macos")]
        {
            use tauri_nspanel::{CollectionBehavior, PanelLevel, StyleMask, WebviewWindowExt};
            let panel = window.to_panel::<MinutaePanel>()?;
            panel.set_level(PanelLevel::Status.value());
            if let Err(e) = panel.add_style_mask(StyleMask::empty().nonactivating_panel().value()) {
                tracing::warn!(?e, "nonactivating panel style");
            }
            panel.set_collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .full_screen_auxiliary()
                    .stationary()
                    .value(),
            );
            panel.set_hides_on_deactivate(false);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = window;
    }
    Ok(())
}

fn place(app: &AppHandle, label: &str) {
    let Some(w) = app.get_webview_window(label) else {
        return;
    };
    let monitor = app
        .get_webview_window(MAIN)
        .and_then(|m| m.current_monitor().ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let scale = m.scale_factor();
    let size = m.size().to_logical::<f64>(scale);
    let origin = m.position().to_logical::<f64>(scale);
    let (x, y) = if label == HUD {
        (
            origin.x + (size.width - HUD_SIZE.0) / 2.0,
            origin.y + size.height - HUD_SIZE.1 - 96.0,
        )
    } else {
        (
            origin.x + size.width - PROMPT_SIZE.0 - 16.0,
            origin.y + 40.0,
        )
    };
    let (sw, sh) = if label == HUD { HUD_SIZE } else { PROMPT_SIZE };
    let _ = w.set_size(LogicalSize::new(sw, sh));
    let _ = w.set_position(LogicalPosition::new(x, y));
}

/// Show an auxiliary window without activating the app.
pub fn show_aux(app: &AppHandle, label: &'static str) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        place(&a, label);
        #[cfg(target_os = "macos")]
        {
            use tauri_nspanel::ManagerExt;
            if let Ok(p) = a.get_webview_panel(label) {
                p.order_front_regardless();
                return;
            }
        }
        if let Some(w) = a.get_webview_window(label) {
            let _ = w.show();
        }
    });
}

pub fn hide_aux(app: &AppHandle, label: &'static str) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        {
            use tauri_nspanel::ManagerExt;
            if let Ok(p) = a.get_webview_panel(label) {
                p.hide();
                return;
            }
        }
        if let Some(w) = a.get_webview_window(label) {
            let _ = w.hide();
        }
    });
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn navigate(app: &AppHandle, route: &str) {
    show_main(app);
    let _ = app.emit_to(
        MAIN,
        ev::NAVIGATE,
        dto::Navigate {
            route: route.to_string(),
        },
    );
}

// ---- tray ----

/// 32×32 template image: a ring (idle) or a filled dot (recording).
fn tray_image(filled: bool) -> Image<'static> {
    const N: u32 = 32;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let outer = (11.0 - d).clamp(0.0, 1.0);
            let alpha = if filled {
                outer
            } else {
                outer * (d - 7.0).clamp(0.0, 1.0)
            };
            let i = ((y * N + x) * 4) as usize;
            rgba[i + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new_owned(rgba, N, N)
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, &RecordingState::idle(true), &[])?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_image(false))
        .icon_as_template(true)
        .tooltip("Minutae")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;
    Ok(())
}

fn build_menu(
    app: &AppHandle,
    rec: &RecordingState,
    recent: &[(String, String)],
) -> tauri::Result<Menu<tauri::Wry>> {
    let st = app.state::<AppState>();
    let settings = st.settings();
    let record = MenuItem::with_id(app, "record", record_label(rec), true, None::<&str>)?;
    *st.tray_record_item
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(record.clone());
    let hint = MenuItem::with_id(
        app,
        "dictate_hint",
        dictate_hint(&settings, &st.shortcut_status()),
        false,
        None::<&str>,
    )?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let open = MenuItem::with_id(app, "open", "Open Minutae", true, None::<&str>)?;
    let prefs = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit Minutae", true, Some("CmdOrCtrl+Q"))?;
    let recent_items: Vec<MenuItem<tauri::Wry>> = recent
        .iter()
        .map(|(id, title)| {
            MenuItem::with_id(app, format!("meeting:{id}"), title, true, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;
    let recent_header =
        MenuItem::with_id(app, "recent_header", "Recent meetings", false, None::<&str>)?;
    let mut items: Vec<&dyn IsMenuItem<tauri::Wry>> = vec![&record, &hint, &sep1];
    if !recent_items.is_empty() {
        items.push(&recent_header);
        for i in &recent_items {
            items.push(i);
        }
        items.push(&sep2);
    }
    items.extend([&open as &dyn IsMenuItem<tauri::Wry>, &prefs, &sep3, &quit]);
    Menu::with_items(app, &items)
}

/// "Hold ⌥Space to dictate", or the portal's own trigger on Wayland.
fn dictate_hint(settings: &AppSettings, status: &dto::ShortcutStatus) -> String {
    let verb = if status.hold_supported == Some(false)
        || settings.dictation_mode == minutae_core::HotkeyMode::Toggle
    {
        "Press"
    } else {
        "Hold"
    };
    match status.backend {
        dto::ShortcutBackend::Native => format!(
            "{verb} {} to dictate",
            util::shortcut_label(&settings.dictation_hotkey)
        ),
        dto::ShortcutBackend::Portal => match &status.dictate_trigger {
            Some(t) => format!("{verb} {t} to dictate"),
            None => "Dictation shortcut not set".into(),
        },
        dto::ShortcutBackend::Unavailable => "Dictation shortcut unavailable".into(),
    }
}

fn record_label(rec: &RecordingState) -> String {
    match rec.phase {
        RecordingPhase::Idle => "Start recording".into(),
        RecordingPhase::Stopping => "Saving…".into(),
        RecordingPhase::Paused => {
            format!("Stop recording · paused {}", util::clock(rec.elapsed_ms))
        }
        RecordingPhase::Recording => format!("Stop recording · {}", util::clock(rec.elapsed_ms)),
    }
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref().to_string();
    match id.as_str() {
        "record" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { crate::recording::toggle(&app).await });
        }
        "open" => show_main(app),
        "settings" => navigate(app, "/settings/general"),
        "quit" => quit(app),
        other => {
            if let Some(mid) = other.strip_prefix("meeting:") {
                navigate(app, &format!("/meetings/{mid}"));
            }
        }
    }
}

static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Quit for real (tray, Cmd+Q, SIGTERM/SIGINT): stop a recording first so its audio is
/// saved, park the job queue, free the whisper models, then exit. Idempotent.
pub fn quit(app: &AppHandle) {
    if QUITTING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    tracing::info!("quitting");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<AppState>();
        // Waits for an in-flight stop too, so we never exit mid-stitch.
        crate::recording::stop_and_wait(&app).await;
        if let Ok(r) = st.runner() {
            // Aborts running steps; their guards raise whisper's abort flag.
            r.shutdown().await;
        }
        if let Some(c) = st.dictation.get() {
            c.cancel();
        }
        if let Some(h) = st.monitor.lock().unwrap_or_else(|e| e.into_inner()).take() {
            h.stop();
        }
        // Free whisper contexts before exit: ggml-metal asserts if a Metal device is
        // still referenced when static destructors run.
        let cache = st.transcribers.clone();
        let _ =
            tokio::task::spawn_blocking(move || cache.shutdown(std::time::Duration::from_secs(10)))
                .await;
        tracing::info!("models unloaded; exiting");
        app.exit(0);
    });
}

/// Called every second while recording and on state changes.
pub fn update_tray_recording(app: &AppHandle, rec: &RecordingState) {
    let st = app.state::<AppState>();
    if let Some(item) = st
        .tray_record_item
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = item.set_text(record_label(rec));
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let active = rec.phase != RecordingPhase::Idle;
        let _ = tray.set_icon(Some(tray_image(active)));
        let _ = tray.set_icon_as_template(true);
        #[cfg(target_os = "macos")]
        let _ = tray.set_title(active.then(|| util::clock(rec.elapsed_ms)));
    }
}

/// Rebuild the tray menu (recent meetings, labels).
pub fn refresh_tray(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<AppState>();
        let recent: Vec<(String, String)> = st
            .store
            .call(|s| s.list_meetings(3, 0))
            .await
            .map(|v| {
                v.into_iter()
                    .map(|i| (i.meeting.id.to_string(), i.meeting.title))
                    .collect()
            })
            .unwrap_or_default();
        let rec = crate::recording::current_state(&app).await;
        let a = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let (Some(tray), Ok(menu)) = (a.tray_by_id(TRAY_ID), build_menu(&a, &rec, &recent)) {
                let _ = tray.set_menu(Some(menu));
            }
        });
    });
}

// ---- global shortcuts ----

#[derive(Debug, Default)]
pub struct Shortcuts {
    pub record: Option<Shortcut>,
    pub dictate: Option<Shortcut>,
}

/// Register exactly the record + dictation shortcuts of `settings` (replacing any
/// others). Returns the accelerators the OS refused (e.g. another app owns them).
pub fn register_shortcuts(app: &AppHandle, settings: &AppSettings) -> Vec<String> {
    if crate::portal::use_portal() {
        // Wayland: the portal owns the keys (bound once by `portal::start`).
        return vec![];
    }
    let st = app.state::<AppState>();
    let gs = app.global_shortcut();
    if let Err(e) = gs.unregister_all() {
        tracing::debug!(error = %e, "unregister shortcuts");
    }
    let parse = |s: &str| -> Option<Shortcut> {
        Shortcut::from_str(s)
            .map_err(|e| tracing::warn!(shortcut = s, error = %e, "invalid shortcut"))
            .ok()
    };
    let record = parse(&settings.record_shortcut);
    let dictate = parse(&settings.dictation_hotkey);
    let mut failed = vec![];
    for (s, text) in [
        (record, &settings.record_shortcut),
        (dictate, &settings.dictation_hotkey),
    ] {
        let Some(s) = s else { continue };
        if let Err(e) = gs.register(s) {
            tracing::warn!(shortcut = %s, error = %e, "could not register shortcut");
            failed.push(text.clone());
        }
    }
    *st.shortcuts.lock().unwrap_or_else(|e| e.into_inner()) = Shortcuts { record, dictate };
    failed
}

/// At startup: bind through the portal on Wayland, else register with the plugin.
pub fn init_shortcuts(app: &AppHandle) {
    if crate::portal::use_portal() {
        crate::portal::start(app);
    } else {
        apply_shortcuts(app);
    }
}

/// (Re)register the record and dictation shortcuts from the current settings.
pub fn apply_shortcuts(app: &AppHandle) {
    let settings = app.state::<AppState>().settings();
    let _ = register_shortcuts(app, &settings);
}

pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    let st = app.state::<AppState>();
    let (record, dictate) = {
        let s = st.shortcuts.lock().unwrap_or_else(|e| e.into_inner());
        (s.record.map(|r| r.id()), s.dictate.map(|d| d.id()))
    };
    if Some(shortcut.id()) == dictate {
        // Starting/stopping the mic (and stitching + reading the clip) blocks, so the
        // key events go, in order, to the dictation worker thread instead.
        let key = if state == ShortcutState::Pressed {
            crate::dictation::DictationKey::Down
        } else {
            crate::dictation::DictationKey::Up
        };
        crate::dictation::send_key(app, key);
    } else if Some(shortcut.id()) == record && state == ShortcutState::Pressed {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { crate::recording::toggle(&app).await });
    }
}
