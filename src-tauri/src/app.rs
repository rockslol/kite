//! App-level things: settings, the tray icon, start with Windows, close to tray, updates,
//! streaming tool info and the cache.

use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_updater::UpdaterExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Closing the window hides it to the tray, so music keeps playing.
    pub close_to_tray: bool,
    /// Check for a new version at startup.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { close_to_tray: true, check_updates: true }
    }
}

pub struct Prefs {
    file: PathBuf,
    current: RwLock<Settings>,
}

impl Prefs {
    pub fn load(data_dir: &Path) -> Self {
        let file = data_dir.join("settings.json");
        let current = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { file, current: RwLock::new(current) }
    }

    pub fn get(&self) -> Settings {
        self.current.read().map(|s| s.clone()).unwrap_or_default()
    }

    fn set(&self, s: Settings) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
        std::fs::write(&self.file, text).map_err(|e| e.to_string())?;
        if let Ok(mut c) = self.current.write() {
            *c = s;
        }
        Ok(())
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        set_shown(&w, true);
    }
}

/// Shows or hides the window, and tells WebView2 whether the page is visible. Hiding the
/// window alone keeps the page drawing frames nobody sees (~3 % CPU while playing); with the
/// page marked hidden the browser stops rendering. Audio keeps playing either way.
pub fn set_shown(window: &tauri::WebviewWindow, shown: bool) {
    if shown {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    #[cfg(windows)]
    let _ = window.with_webview(move |w| unsafe {
        let _ = w.controller().SetIsVisible(shown);
    });
    if !shown {
        let _ = window.hide();
    }
}

/// Tray icon with playback controls. Left click opens the window.
pub fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "toggle", "Play or pause", true, None::<&str>)?,
            &MenuItem::with_id(app, "next", "Next", true, None::<&str>)?,
            &MenuItem::with_id(app, "previous", "Previous", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "show", "Show Kite", true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Kite")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            action @ ("toggle" | "next" | "previous") => {
                let _ = app.emit("media-control", json!({ "action": action }));
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// The tray tooltip follows the current song.
#[tauri::command]
pub fn tray_tooltip(app: AppHandle, text: String) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(if text.is_empty() { "Kite".to_string() } else { format!("Kite: {text}") }));
    }
}

// ---- settings -----------------------------------------------------------------

#[derive(Serialize)]
pub struct SettingsView {
    #[serde(flatten)]
    settings: Settings,
    autostart: bool,
    version: String,
}

#[tauri::command]
pub fn settings_get(app: AppHandle, prefs: State<'_, Prefs>) -> SettingsView {
    SettingsView {
        settings: prefs.get(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        version: app.package_info().version.to_string(),
    }
}

#[tauri::command]
pub fn settings_set(prefs: State<'_, Prefs>, settings: Settings) -> Result<(), String> {
    prefs.set(settings)
}

#[tauri::command]
pub fn autostart_set(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    al.is_enabled().map_err(|e| e.to_string())
}

// ---- streaming tools ------------------------------------------------------------

#[tauri::command]
pub fn tools_info(state: State<'_, AppState>) -> crate::ytdlp::ToolInfo {
    state.yt.info()
}

#[tauri::command]
pub async fn tools_update(state: State<'_, AppState>) -> Result<String, String> {
    state.yt.update_now(&state.http).await
}

// ---- cache ------------------------------------------------------------------------

pub struct CacheDirs(pub Vec<PathBuf>);

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => dir_size(&e.path()),
                    Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

#[tauri::command]
pub fn cache_size(dirs: State<'_, CacheDirs>) -> u64 {
    dirs.0.iter().map(|d| dir_size(d)).sum()
}

/// Deletes cached audio and cached covers. Only files directly inside Kite's own cache folders.
#[tauri::command]
pub fn cache_clear(dirs: State<'_, CacheDirs>) -> u64 {
    for dir in &dirs.0 {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if e.file_type().is_ok_and(|t| t.is_file()) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    dirs.0.iter().map(|d| dir_size(d)).sum()
}

// ---- updates ----------------------------------------------------------------------

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<String>, String> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| u.version))
}

#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Kite is up to date.")?;
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart();
}

/// Quiet check at startup; tells the UI if there's a new version.
pub fn check_updates_later(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        if let Ok(Some(version)) = update_check(app.clone()).await {
            let _ = app.emit("update-available", version);
        }
    });
}
