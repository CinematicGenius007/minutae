mod audio;
mod commands;
mod db;
mod models;

use audio::MicMonitor;
use tauri_plugin_sql::{Migration, MigrationKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let migrations = vec![
        Migration {
            version: 1,
            description: "initial schema",
            sql: include_str!("../migrations/0001_init.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "fts triggers",
            sql: include_str!("../migrations/0002_fts_triggers.sql"),
            kind: MigrationKind::Up,
        },
    ];

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:minutae.db", migrations)
                .build(),
        )
        .manage(MicMonitor::new())
        .invoke_handler(tauri::generate_handler![
            commands::fs::ensure_app_dirs,
            commands::fs::save_recording,
            commands::fs::write_summary_file,
            commands::fs::delete_meeting_assets,
            commands::mic::start_mic_monitor,
            commands::mic::stop_mic_monitor,
            models::whisper::transcribe_local,
            models::whisper::get_local_transcription_status,
            models::whisper::download_whisper_model,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = commands::fs::ensure_app_dirs(handle) {
                    eprintln!("Failed to create app dirs: {e}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
