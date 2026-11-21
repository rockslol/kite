//! Watches the music folders while Kite runs, so added, changed, moved or deleted files show up
//! without a restart. Changes are batched: the (incremental) scan starts once the folders have
//! been quiet for a moment, so copying an album in is one scan, not one per file.

use crate::library::Library;
use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// How long the folders must be quiet before scanning.
const QUIET: Duration = Duration::from_secs(2);

pub struct FolderWatch {
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    watched: Mutex<HashSet<PathBuf>>,
}

/// Starts the watcher and the thread that turns its events into scans.
pub fn start(app: &AppHandle, lib: Arc<Library>) -> Arc<FolderWatch> {
    let (tx, rx) = mpsc::channel::<()>();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| relevant(&e)) {
            let _ = tx.send(());
        }
    })
    .map_err(|e| crate::klog!("folder watching unavailable: {e}"))
    .ok();

    let handle = app.clone();
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // Wait for the burst to end (an album being copied in).
            loop {
                match rx.recv_timeout(QUIET) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            // A scan already running may have walked past these files: let it finish, then scan again.
            while lib.scanning.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(500));
            }
            crate::local::rescan(handle.clone(), lib.clone());
        }
    });

    let w = Arc::new(FolderWatch { watcher: Mutex::new(watcher), watched: Mutex::new(HashSet::new()) });
    app.manage(w.clone());
    refresh(app);
    w
}

/// Watches exactly the folders in the library (call after adding or removing one).
pub fn refresh(app: &AppHandle) {
    let (Some(w), Some(lib)) = (app.try_state::<Arc<FolderWatch>>(), app.try_state::<Arc<Library>>()) else {
        return;
    };
    let Ok(folders) = lib.with(|c| {
        let mut st = c.prepare("SELECT path FROM folders")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }) else {
        return;
    };
    let want: HashSet<PathBuf> = folders.into_iter().map(PathBuf::from).collect();
    let mut guard = w.watcher.lock().unwrap();
    let Some(watcher) = guard.as_mut() else { return };
    let mut watched = w.watched.lock().unwrap();
    for gone in watched.difference(&want).cloned().collect::<Vec<_>>() {
        let _ = watcher.unwatch(&gone);
        watched.remove(&gone);
    }
    for new in want.difference(&watched.clone()) {
        // A folder on a drive that isn't plugged in can't be watched; the startup scan and
        // "Rescan" still cover it.
        match watcher.watch(new, RecursiveMode::Recursive) {
            Ok(()) => {
                watched.insert(new.clone());
            }
            Err(e) => crate::klog!("can't watch {}: {e}", new.display()),
        }
    }
}

/// Only changes that can affect the library: audio files, and folders being created, deleted or
/// renamed (a folder moved in or out carries its songs with it). Reads, covers, text files and
/// "this folder's contents changed" (Windows reports that next to every file event) are ignored.
fn relevant(e: &notify::Event) -> bool {
    let folder_moved = matches!(e.kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)));
    !matches!(e.kind, EventKind::Access(_))
        && e.paths.iter().any(|p| is_audio(p) || (folder_moved && p.extension().is_none()))
}

fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| crate::local::AUDIO.contains(&e.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, RenameMode};

    fn ev(kind: EventKind, path: &str) -> notify::Event {
        notify::Event::new(kind).add_path(PathBuf::from(path))
    }

    /// The real OS watcher sees a song being added, renamed and deleted in a subfolder.
    #[test]
    fn sees_real_file_changes() {
        let dir = std::env::temp_dir().join(format!("kite-watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Album")).unwrap();
        let (tx, rx) = mpsc::channel();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok_and(|e| relevant(&e)) {
                let _ = tx.send(());
            }
        })
        .unwrap();
        w.watch(&dir, RecursiveMode::Recursive).unwrap();
        let seen = |what: &str| {
            assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "no event for {what}");
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
        };
        std::fs::write(dir.join("Album").join("song.mp3"), b"x").unwrap();
        seen("create");
        std::fs::write(dir.join("Album").join("cover.jpg"), b"x").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(800)).is_err(), "a cover alone shouldn't scan");
        std::fs::rename(dir.join("Album").join("song.mp3"), dir.join("Album").join("song2.mp3")).unwrap();
        seen("rename");
        std::fs::remove_file(dir.join("Album").join("song2.mp3")).unwrap();
        seen("delete");
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_audio_and_folders_trigger_a_scan() {
        assert!(relevant(&ev(EventKind::Create(CreateKind::Any), r"C:\Music\song.MP3")));
        assert!(relevant(&ev(EventKind::Remove(notify::event::RemoveKind::Any), r"C:\Music\Album")));
        assert!(relevant(&ev(EventKind::Modify(ModifyKind::Any), r"C:\Music\a.flac")));
        assert!(!relevant(&ev(EventKind::Create(CreateKind::Any), r"C:\Music\cover.jpg")));
        assert!(!relevant(&ev(EventKind::Access(AccessKind::Any), r"C:\Music\song.mp3")));
        assert!(relevant(&ev(EventKind::Modify(ModifyKind::Name(RenameMode::Any)), r"C:\Music\Old album")));
        assert!(!relevant(&ev(EventKind::Modify(ModifyKind::Any), r"C:\Music\Album")));
    }
}
