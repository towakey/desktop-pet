use chrono::Timelike;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::{fs, path::PathBuf};
use tauri::{Manager, WindowEvent};

#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{CreatePolygonRgn, SetWindowRgn, ALTERNATE};
#[cfg(windows)]
use windows::Win32::Foundation::POINT;
#[cfg(windows)]
use windows::Win32::Media::Audio::{
    PlaySoundW, SND_ALIAS, SND_FILENAME, SND_LOOP, SND_ASYNC, SND_PURGE,
};

const TASK_NAME: &str = "DesktopPetAlarm";

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
    pub alarm: Arc<Mutex<AlarmConfig>>,
    pub config_path: PathBuf,
    pub alarm_active: Arc<AtomicBool>,
    pub dismissed: Arc<AtomicBool>,
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

#[cfg(windows)]
fn play_alarm_sound() {
    let candidates = [
        r"C:\Windows\Media\Alarm01.wav",
        r"C:\Windows\Media\Alarm02.wav",
        r"C:\Windows\Media\Alarm03.wav",
        r"C:\Windows\Media\Windows Notify.wav",
        r"C:\Windows\Media\Windows Logon.wav",
    ];
    for path in candidates {
        if std::path::Path::new(path).exists() {
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let _ = PlaySoundW(
                    PCWSTR(wide.as_ptr()),
                    None,
                    SND_FILENAME | SND_LOOP | SND_ASYNC,
                );
            }
            return;
        }
    }
    let alias: Vec<u16> = "SystemDefault\0".encode_utf16().collect();
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(alias.as_ptr()),
            None,
            SND_ALIAS | SND_LOOP | SND_ASYNC,
        );
    }
}

#[cfg(windows)]
fn stop_alarm_sound() {
    unsafe {
        let _ = PlaySoundW(PCWSTR::null(), None, SND_PURGE);
    }
}

#[cfg(not(windows))]
fn play_alarm_sound() {}
#[cfg(not(windows))]
fn stop_alarm_sound() {}

/// Register a Windows Task Scheduler task that wakes the PC from sleep
/// at the specified alarm time. The task runs a trivial command; the
/// actual alarm sound is played by the polling thread after wake.
#[cfg(windows)]
fn schedule_wake_task(config: &AlarmConfig) {
    // Always remove existing task first
    remove_wake_task();

    if !config.enabled {
        return;
    }

    let now = chrono::Local::now();
    let today = now.date_naive();
    let alarm_naive = match today.and_hms_opt(config.hour, config.minute, 0) {
        Some(t) => t,
        None => {
            eprintln!("[wake_task] invalid alarm time");
            return;
        }
    };
    let alarm_local = if alarm_naive <= now.naive_local() {
        alarm_naive + chrono::Duration::days(1)
    } else {
        alarm_naive
    };

    let time_str = alarm_local.format("%Y-%m-%dT%H:%M:%S").to_string();
    eprintln!("[wake_task] scheduling for {}", time_str);

    // PowerShell command to create a scheduled task with WakeToRun
    let ps_script = format!(
        r#"
$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument '/c exit'
$trigger = New-ScheduledTaskTrigger -Once -At '{time}'
$settings = New-ScheduledTaskSettingsSet -WakeToRun -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
Register-ScheduledTask -TaskName '{task}' -Action $action -Trigger $trigger -Settings $settings -Force
"#,
        time = time_str,
        task = TASK_NAME,
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output();

    match output {
        Ok(o) => {
            if o.status.success() {
                eprintln!("[wake_task] task registered successfully");
            } else {
                let stderr = String::from_utf8_lossy(&o.stderr);
                let stdout = String::from_utf8_lossy(&o.stdout);
                eprintln!("[wake_task] task registration failed: stdout={}, stderr={}", stdout, stderr);
            }
        }
        Err(e) => {
            eprintln!("[wake_task] failed to run powershell: {}", e);
        }
    }
}

#[cfg(windows)]
fn remove_wake_task() {
    let ps_script = format!("Unregister-ScheduledTask -TaskName '{}' -Confirm:$false -ErrorAction SilentlyContinue", TASK_NAME);
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output();
    eprintln!("[wake_task] removed existing task");
}

#[cfg(not(windows))]
fn schedule_wake_task(_config: &AlarmConfig) {}
#[cfg(not(windows))]
fn remove_wake_task() {}

/// Polling thread: checks every second and triggers alarm when time matches.
/// Also serves as backup after wake-from-sleep.
fn start_alarm_thread(
    alarm: Arc<Mutex<AlarmConfig>>,
    alarm_active: Arc<AtomicBool>,
    dismissed: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut last_triggered: Option<(u32, u32)> = None;
        loop {
            thread::sleep(Duration::from_secs(1));

            let config = alarm.lock().unwrap().clone();

            if !config.enabled {
                if alarm_active.load(Ordering::Relaxed) {
                    stop_alarm_sound();
                    alarm_active.store(false, Ordering::Relaxed);
                }
                last_triggered = None;
                dismissed.store(false, Ordering::Relaxed);
                continue;
            }

            let now = chrono::Local::now();
            let current = (now.hour(), now.minute());

            if dismissed.load(Ordering::Relaxed) {
                if alarm_active.load(Ordering::Relaxed) {
                    stop_alarm_sound();
                    alarm_active.store(false, Ordering::Relaxed);
                }
                if last_triggered.is_some() && last_triggered != Some(current) {
                    last_triggered = None;
                    dismissed.store(false, Ordering::Relaxed);
                }
                continue;
            }

            if current == (config.hour, config.minute) {
                if !alarm_active.load(Ordering::Relaxed)
                    && last_triggered != Some(current)
                {
                    play_alarm_sound();
                    alarm_active.store(true, Ordering::Relaxed);
                    last_triggered = Some(current);
                }
            } else {
                if alarm_active.load(Ordering::Relaxed) {
                    stop_alarm_sound();
                    alarm_active.store(false, Ordering::Relaxed);
                }
                last_triggered = None;
                dismissed.store(false, Ordering::Relaxed);
            }
        }
    });
}

#[cfg(windows)]
fn set_window_region(window: &tauri::WebviewWindow) {
    let handle = match window.window_handle() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[window_region] failed to get window handle: {}", e);
            return;
        }
    };
    let hwnd = match handle.as_raw() {
        RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut std::ffi::c_void),
        _ => {
            eprintln!("[window_region] unsupported window handle type");
            return;
        }
    };

    let scale = window.scale_factor().unwrap_or(1.0);
    let mut points = [
        (88, 28),
        (138, 58),
        (153, 98),
        (163, 88),
        (158, 108),
        (128, 138),
        (123, 153),
        (103, 153),
        (113, 138),
        (73, 138),
        (65, 153),
        (45, 153),
        (55, 138),
        (48, 138),
        (23, 98),
        (38, 58),
    ].map(|(x, y)| POINT {
        x: (x as f64 * scale).round() as i32,
        y: (y as f64 * scale).round() as i32,
    });

    unsafe {
        let rgn = CreatePolygonRgn(&mut points, ALTERNATE);
        let _ = SetWindowRgn(hwnd, Some(rgn), true);
        // SetWindowRgn takes ownership of the region, do not delete it
    }
}

#[cfg(not(windows))]
fn set_window_region(_window: &tauri::WebviewWindow) {}

#[tauri::command]
fn get_alarm(state: tauri::State<'_, AppState>) -> AlarmConfig {
    state.alarm.lock().unwrap().clone()
}

#[tauri::command]
fn set_alarm(state: tauri::State<'_, AppState>, config: AlarmConfig) -> Result<(), String> {
    save_alarm(&state.config_path, &config)?;
    {
        let mut alarm = state.alarm.lock().unwrap();
        *alarm = config.clone();
    }

    // Register or remove the wake task
    schedule_wake_task(&config);

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
    state.alarm_active.load(Ordering::Relaxed)
}

#[tauri::command]
fn dismiss_alarm(state: tauri::State<'_, AppState>) {
    state.dismissed.store(true, Ordering::Relaxed);
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
            let alarm = Arc::new(Mutex::new(alarm));
            let alarm_active = Arc::new(AtomicBool::new(false));
            let dismissed = Arc::new(AtomicBool::new(false));

            // Start polling thread (handles sound playback, including after wake)
            start_alarm_thread(
                Arc::clone(&alarm),
                Arc::clone(&alarm_active),
                Arc::clone(&dismissed),
            );

            // Register wake task if alarm is enabled at startup
            {
                let config = alarm.lock().unwrap().clone();
                if config.enabled {
                    schedule_wake_task(&config);
                }
            }

            if let Some(character) = app.get_webview_window("character") {
                #[cfg(windows)]
                set_window_region(&character);
            }

            app.manage(AppState {
                alarm,
                config_path,
                alarm_active,
                dismissed,
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
            dismiss_alarm,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
