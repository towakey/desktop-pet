use chrono::{Timelike, TimeZone};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::{fs, path::PathBuf};
use tauri::{Manager, WindowEvent};

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
#[cfg(windows)]
use windows::Win32::Media::Audio::{
    PlaySoundW, SND_ALIAS, SND_FILENAME, SND_LOOP, SND_ASYNC, SND_PURGE,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, SetWaitableTimer, WaitForSingleObject,
    CREATE_WAITABLE_TIMER_MANUAL_RESET, TIMER_ALL_ACCESS,
};

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
    pub timer_generation: Arc<AtomicU64>,
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

/// Wake timer thread: uses Windows Waitable Timer to wake from sleep.
/// Each call gets a generation number; if a newer generation exists, the old thread exits.
#[cfg(windows)]
fn start_wake_timer_thread(
    config: AlarmConfig,
    alarm_active: Arc<AtomicBool>,
    timer_generation: Arc<AtomicU64>,
    my_generation: u64,
) {
    thread::spawn(move || {
        if !config.enabled {
            return;
        }

        // Calculate next alarm time in local, then convert to UTC
        let now = chrono::Local::now();
        let today = now.date_naive();
        let alarm_naive = match today.and_hms_opt(config.hour, config.minute, 0) {
            Some(t) => t,
            None => return,
        };
        let alarm_local = if alarm_naive <= now.naive_local() {
            alarm_naive + chrono::Duration::days(1)
        } else {
            alarm_naive
        };

        // Convert local NaiveDateTime to UTC timestamp
        let local_dt = match chrono::Local.from_local_datetime(&alarm_local) {
            chrono::MappedLocalTime::Single(dt) => dt,
            _ => return,
        };
        let utc_dt = local_dt.with_timezone(&chrono::Utc);
        let unix_timestamp = utc_dt.timestamp() as i64;

        // Windows FILETIME: 100ns intervals since 1601-01-01 UTC
        let filetime_100ns = unix_timestamp * 10_000_000 + 116_444_736_000_000_000;

        unsafe {
            let timer = match CreateWaitableTimerExW(
                None,
                PCWSTR::null(),
                CREATE_WAITABLE_TIMER_MANUAL_RESET,
                TIMER_ALL_ACCESS.0,
            ) {
                Ok(t) => t,
                Err(_) => return,
            };

            let due_time: i64 = filetime_100ns;
            // fResume = true: wake from sleep
            if SetWaitableTimer(timer, &due_time, 0, None, None, true).is_err() {
                let _ = CloseHandle(timer);
                return;
            }

            // Wait for the timer to signal, checking generation every second
            loop {
                if timer_generation.load(Ordering::SeqCst) != my_generation {
                    let _ = CloseHandle(timer);
                    return;
                }
                let result = WaitForSingleObject(timer, 1000);
                if result == WAIT_OBJECT_0 {
                    // Timer fired (possibly woke from sleep)
                    // The polling thread will handle the actual sound playback,
                    // but we trigger it immediately here too for responsiveness.
                    let now = chrono::Local::now();
                    let current = (now.hour(), now.minute());
                    if current == (config.hour, config.minute)
                        && !alarm_active.load(Ordering::Relaxed)
                    {
                        play_alarm_sound();
                        alarm_active.store(true, Ordering::Relaxed);
                    }
                    let _ = CloseHandle(timer);
                    return;
                }
            }
        }
    });
}

#[cfg(not(windows))]
fn start_wake_timer_thread(
    _config: AlarmConfig,
    _alarm_active: Arc<AtomicBool>,
    _timer_generation: Arc<AtomicU64>,
    _my_generation: u64,
) {
}

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

    // Start a new wake timer thread with an incremented generation.
    // Old threads will see the generation mismatch and exit.
    let my_generation = state.timer_generation.fetch_add(1, Ordering::SeqCst) + 1;

    if config.enabled {
        start_wake_timer_thread(
            config,
            Arc::clone(&state.alarm_active),
            Arc::clone(&state.timer_generation),
            my_generation,
        );
    }

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
            let timer_generation = Arc::new(AtomicU64::new(0));

            // Start polling thread (backup for when not sleeping)
            start_alarm_thread(
                Arc::clone(&alarm),
                Arc::clone(&alarm_active),
                Arc::clone(&dismissed),
            );

            // Set initial wake timer if alarm is enabled
            {
                let config = alarm.lock().unwrap().clone();
                if config.enabled {
                    let my_generation =
                        timer_generation.fetch_add(1, Ordering::SeqCst) + 1;
                    start_wake_timer_thread(
                        config,
                        Arc::clone(&alarm_active),
                        Arc::clone(&timer_generation),
                        my_generation,
                    );
                }
            }

            app.manage(AppState {
                alarm,
                config_path,
                alarm_active,
                dismissed,
                timer_generation,
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
