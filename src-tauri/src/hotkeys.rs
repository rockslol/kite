//! Shortcuts that work while Kite is in the background. Off by default: they also fire inside
//! games and other apps, so the user switches them on and picks the keys.

use crate::app::Prefs;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// The keys for each action, as `Ctrl+Alt+Space` (key names are `KeyboardEvent.code`s).
/// Empty means not set.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Hotkeys {
    pub toggle: String,
    pub next: String,
    pub previous: String,
    pub like: String,
    pub show: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        // No Ctrl+Alt+letter: on many keyboards (Hungarian, German, Polish, …) that's AltGr,
        // and taking it would stop people typing characters like ł or @ everywhere.
        Self {
            toggle: "Ctrl+Alt+Space".into(),
            next: "Ctrl+Alt+ArrowRight".into(),
            previous: "Ctrl+Alt+ArrowLeft".into(),
            like: "Ctrl+Alt+ArrowUp".into(),
            show: String::new(),
        }
    }
}

impl Hotkeys {
    fn pairs(&self) -> [(&'static str, &str); 5] {
        [
            ("toggle", &self.toggle),
            ("next", &self.next),
            ("previous", &self.previous),
            ("like", &self.like),
            ("show", &self.show),
        ]
    }
}

/// Which action each registered shortcut stands for.
#[derive(Default)]
pub struct Registered(Mutex<HashMap<u32, &'static str>>);

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let action = app.state::<Registered>().0.lock().unwrap().get(&shortcut.id()).copied();
            match action {
                Some("show") => crate::app::show_main(app),
                Some("like") => {
                    let _ = app.emit("hotkey", json!({ "action": "like" }));
                }
                Some(a) => {
                    let _ = app.emit("media-control", json!({ "action": a }));
                }
                None => {}
            }
        })
        .build()
}

/// Registers the shortcuts from the settings (none when they're switched off). Returns a
/// problem per action that couldn't be set, e.g. keys another app already uses.
pub fn apply(app: &AppHandle) -> HashMap<&'static str, String> {
    let settings = app.state::<Prefs>().get();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let registered = app.state::<Registered>();
    let mut map = registered.0.lock().unwrap();
    map.clear();
    let mut problems = HashMap::new();
    if !settings.hotkeys_on {
        return problems;
    }
    for (action, keys) in settings.hotkeys.pairs() {
        if keys.is_empty() {
            continue;
        }
        let shortcut: Shortcut = match keys.parse() {
            Ok(s) => s,
            Err(_) => {
                problems.insert(action, "Not a key combination Kite can use".to_string());
                continue;
            }
        };
        if map.contains_key(&shortcut.id()) {
            problems.insert(action, "Already used for another action".to_string());
            continue;
        }
        match gs.register(shortcut) {
            Ok(()) => {
                map.insert(shortcut.id(), action);
            }
            Err(e) => {
                crate::klog!("couldn't register {keys} for {action}: {e}");
                problems.insert(action, "Another app already uses these keys".to_string());
            }
        }
    }
    problems
}

/// Re-registers after a settings change; the interface shows the problems next to each key.
#[tauri::command]
pub fn hotkeys_apply(app: AppHandle) -> HashMap<&'static str, String> {
    apply(&app)
}

/// While the user records a new shortcut: let every key reach the window.
#[tauri::command]
pub fn hotkeys_suspend(app: AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keys_parse() {
        for (_, keys) in Hotkeys::default().pairs() {
            if !keys.is_empty() {
                assert!(keys.parse::<Shortcut>().is_ok(), "{keys}");
            }
        }
        assert!("Ctrl+Alt+KeyL".parse::<Shortcut>().is_ok(), "KeyboardEvent.code names");
        assert!("Ctrl+Alt+Nope".parse::<Shortcut>().is_err());
    }
}
