use tauri::State;
use crate::audio::MicMonitor;

#[tauri::command]
pub fn start_mic_monitor(app: tauri::AppHandle, monitor: State<MicMonitor>) {
    monitor.start(app);
}

#[tauri::command]
pub fn stop_mic_monitor(monitor: State<MicMonitor>) {
    monitor.stop();
}
