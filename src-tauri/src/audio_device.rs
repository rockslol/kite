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
