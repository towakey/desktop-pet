use chrono::Timelike;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{Manager, WindowEvent};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmConfig {
    pub enabled: bool,
    pub hour: u32,
    pub minute: u32,
}

impl Default for AlarmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hour: 7,
            minute: 0,
        }
    }
}

pub struct AppState {
    pub alarm: Mutex<AlarmConfig>,
    pub config_path: PathBuf,
}

fn load_alarm(config_path: &PathBuf) -> AlarmConfig {
    fs::read_to_string(config_path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn save_alarm(config_path: &PathBuf, config: &AlarmConfig) -> Result<(), String> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let contents = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(config_path, contents).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_alarm(state: tauri::State<'_, AppState>) -> AlarmConfig {
    state.alarm.lock().unwrap().clone()
}

#[tauri::command]
fn set_alarm(state: tauri::State<'_, AppState>, config: AlarmConfig) -> Result<(), String> {
    save_alarm(&state.config_path, &config)?;
    let mut alarm = state.alarm.lock().unwrap();
    *alarm = config;
    Ok(())
}

#[tauri::command]
fn open_settings(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[tauri::command]
fn check_alarm(state: tauri::State<'_, AppState>) -> bool {
    let alarm = state.alarm.lock().unwrap();
    if !alarm.enabled {
        return false;
    }
    let now = chrono::Local::now();
    now.hour() == alarm.hour && now.minute() == alarm.minute
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let config_path = config_dir.join("alarm.json");
            let alarm = load_alarm(&config_path);
            app.manage(AppState {
                alarm: Mutex::new(alarm),
                config_path,
            });
            if let Some(win) = app.get_webview_window("settings") {
                let settings_win = win.clone();
                win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = settings_win.hide();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_alarm,
            set_alarm,
            open_settings,
            check_alarm,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
