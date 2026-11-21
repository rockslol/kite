//! System media controls: Windows SMTC (seen by Notch, media keys, the volume flyout),
//! macOS Now Playing, MPRIS on Linux.
//!
//! The controls live on the main thread. Calls are always marshalled there, and the controls
//! are created on first use, when the main window is sure to exist.

use serde_json::json;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use std::cell::RefCell;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

thread_local! {
    static CONTROLS: RefCell<Option<MediaControls>> = const { RefCell::new(None) };
}

fn with_controls(app: &AppHandle, f: impl FnOnce(&mut MediaControls) + Send + 'static) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        CONTROLS.with(|cell| {
            let mut cell = cell.borrow_mut();
            if cell.is_none() {
                *cell = create(&handle);
            }
            if let Some(controls) = cell.as_mut() {
                f(controls);
            }
        });
    });
}

fn create(app: &AppHandle) -> Option<MediaControls> {
    #[cfg(windows)]
    let hwnd = Some(app.get_webview_window("main")?.hwnd().ok()?.0 as *mut std::ffi::c_void);
    #[cfg(not(windows))]
    let hwnd = None;

    let mut controls = MediaControls::new(PlatformConfig {
        display_name: "Kite",
        dbus_name: "app.kite.player",
        hwnd,
    })
    .map_err(|e| crate::klog!("media controls unavailable: {e:?}"))
    .ok()?;

    let app = app.clone();
    controls
        .attach(move |event| {
            let payload = match event {
                MediaControlEvent::Play => json!({ "action": "play" }),
                MediaControlEvent::Pause | MediaControlEvent::Stop => json!({ "action": "pause" }),
                MediaControlEvent::Toggle => json!({ "action": "toggle" }),
                MediaControlEvent::Next => json!({ "action": "next" }),
                MediaControlEvent::Previous => json!({ "action": "previous" }),
                MediaControlEvent::SetPosition(MediaPosition(at)) => {
                    json!({ "action": "seek", "position": at.as_secs_f64() })
                }
                MediaControlEvent::Seek(dir) => json!({ "action": "seekBy", "delta": signed(dir, 10.0) }),
                MediaControlEvent::SeekBy(dir, by) => {
                    json!({ "action": "seekBy", "delta": signed(dir, by.as_secs_f64()) })
                }
                MediaControlEvent::Raise => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                    return;
                }
                _ => return,
            };
            let _ = app.emit("media-control", payload);
        })
        .map_err(|e| crate::klog!("couldn't attach media controls: {e:?}"))
        .ok()?;
    Some(controls)
}

fn signed(dir: SeekDirection, secs: f64) -> f64 {
    match dir {
        SeekDirection::Forward => secs,
        SeekDirection::Backward => -secs,
    }
}

#[tauri::command]
pub fn media_track(
    app: AppHandle,
    title: String,
    artist: String,
    artwork: Option<String>,
    duration: Option<f64>,
    url: Option<String>,
    album: Option<String>,
) {
    if let Some(d) = app.try_state::<crate::discord::Discord>() {
        d.set_song(crate::discord::Song {
            title: title.clone(),
            artist: artist.clone(),
            album,
            artwork: artwork.clone(),
            url,
            duration,
        });
    }
    // Artwork from local files is stored as `art:<file>`; the OS wants a real file.
    let artwork = match artwork.as_deref().and_then(|a| a.strip_prefix("art:")) {
        Some(name) => app
            .try_state::<std::sync::Arc<crate::library::Library>>()
            .map(|lib| format!("file://{}", lib.art_dir.join(name).display())),
        None => artwork,
    };
    with_controls(&app, move |c| {
        let _ = c.set_metadata(MediaMetadata {
            title: Some(&title),
            artist: Some(&artist),
            album: None,
            cover_url: artwork.as_deref(),
            duration: duration.filter(|d| d.is_finite() && *d > 0.0).map(Duration::from_secs_f64),
        });
    });
}

/// `state` is "playing", "paused" or "stopped".
#[tauri::command]
pub fn media_state(app: AppHandle, state: String, position: Option<f64>) {
    if let Some(d) = app.try_state::<crate::discord::Discord>() {
        d.set_state(state == "playing", position);
    }
    with_controls(&app, move |c| {
        let progress = position
            .filter(|p| p.is_finite() && *p >= 0.0)
            .map(|p| MediaPosition(Duration::from_secs_f64(p)));
        let playback = match state.as_str() {
            "playing" => MediaPlayback::Playing { progress },
            "paused" => MediaPlayback::Paused { progress },
            _ => MediaPlayback::Stopped,
        };
        let _ = c.set_playback(playback);
    });
}
