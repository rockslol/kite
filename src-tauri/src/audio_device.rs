//! Which speakers or headphones Windows is playing through, so Kite can keep a volume per
//! device (headphones at 30 %, speakers at 70 %). Checked every 2 seconds; when the default
//! output changes the interface gets an `audio-device` event.
//!
//! Windows only: elsewhere there's no device and Kite keeps one volume.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct Device {
    id: String,
    name: String,
}

#[derive(Default)]
pub struct Current(Mutex<Option<Device>>);

#[tauri::command]
pub fn audio_device(current: tauri::State<'_, Current>) -> Option<Device> {
    current.0.lock().unwrap().clone()
}

pub fn start(app: &AppHandle) {
    app.manage(Current::default());
    #[cfg(windows)]
    {
        let app = app.clone();
        std::thread::spawn(move || win::watch(app));
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// Lets the page see the names of the speakers and headphones, so Settings can offer them.
/// Chromium only names devices for pages allowed to use the microphone, so Kite's own page
/// gets that permission. Kite never opens the microphone (and plugins run in workers, which
/// can't), so nothing is recorded and Windows shows no microphone-in-use icon.
/// Called by the page once it has loaded (before that the window has no origin yet). Answers
/// when the permission is in place, so the page can list the devices right after.
#[tauri::command]
pub async fn allow_device_names(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        let url = window.url().map_err(|e| e.to_string())?;
        let origin = url.origin().ascii_serialization();
        // Answered once, from whichever comes first: an error here or WebView2's callback.
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let reply: Reply = std::sync::Arc::new(Mutex::new(Some(tx)));
        let o = origin.clone();
        window
            .with_webview(move |wv| unsafe {
                use webview2_com::Microsoft::Web::WebView2::Win32::{
                    ICoreWebView2Profile4, ICoreWebView2_13, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
                    COREWEBVIEW2_PERMISSION_STATE_ALLOW,
                };
                use windows::core::{Interface, HSTRING};
                let profile = wv
                    .controller()
                    .CoreWebView2()
                    .and_then(|c| c.cast::<ICoreWebView2_13>())
                    .and_then(|c| c.Profile())
                    .and_then(|p| p.cast::<ICoreWebView2Profile4>());
                let profile = match profile {
                    Ok(p) => p,
                    Err(e) => return answer(&reply, Err(format!("this WebView2 can't do it: {e}"))),
                };
                let on_done = reply.clone();
                let done = webview2_com::SetPermissionStateCompletedHandler::create(Box::new(move |r| {
                    answer(&on_done, r.map_err(|e| e.to_string()));
                    Ok(())
                }));
                if let Err(e) = profile.SetPermissionState(
                    COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
                    &HSTRING::from(o.as_str()),
                    COREWEBVIEW2_PERMISSION_STATE_ALLOW,
                    &done,
                ) {
                    answer(&reply, Err(e.to_string()));
                }
            })
            .map_err(|e| e.to_string())?;
        let r = rx.await.unwrap_or_else(|_| Err("no answer".into()));
        if let Err(e) = &r {
            crate::klog!("couldn't get device names for {origin}: {e}");
        }
        r
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Ok(())
    }
}

#[cfg(windows)]
type Reply = std::sync::Arc<Mutex<Option<tokio::sync::oneshot::Sender<Result<(), String>>>>>;

#[cfg(windows)]
fn answer(reply: &Reply, r: Result<(), String>) {
    if let Some(tx) = reply.lock().unwrap().take() {
        let _ = tx.send(r);
    }
}

fn changed(app: &AppHandle, device: Option<Device>) {
    let current = app.state::<Current>();
    let mut cur = current.0.lock().unwrap();
    if *cur != device {
        *cur = device.clone();
        drop(cur);
        let _ = app.emit("audio-device", device);
    }
}

#[cfg(windows)]
mod win {
    use super::{changed, Device};
    use std::time::Duration;
    use tauri::AppHandle;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

    pub fn watch(app: AppHandle) {
        // SAFETY: this thread owns its COM apartment for its whole life.
        if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
            return;
        }
        let Ok(enumerator) = (unsafe { CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) }) else {
            crate::klog!("audio devices unavailable");
            return;
        };
        loop {
            changed(&app, default_output(&enumerator));
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    pub(super) fn default_output(e: &IMMDeviceEnumerator) -> Option<Device> {
        // SAFETY: plain Core Audio calls; the ID string is freed with CoTaskMemFree as documented.
        unsafe {
            let dev = e.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let raw = dev.GetId().ok()?;
            let id = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0 as _));
            let name = dev
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                .map(|v| v.to_string())
                .unwrap_or_default();
            Some(Device { id: id?, name })
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    /// Needs a PC with sound output, so it only runs on request: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn finds_the_default_output() {
        use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator};
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
        let d = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).unwrap();
            super::win::default_output(&e)
        };
        let d = d.expect("a default output device");
        println!("default output: {} ({} chars of id)", d.name, d.id.len());
        assert!(!d.id.is_empty());
    }
}
