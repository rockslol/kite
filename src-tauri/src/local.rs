//! Scans music folders into the library. Incremental: files whose modified time hasn't changed
//! are skipped, so a rescan at startup is cheap.

use crate::library::{now, Library};
use lofty::picture::MimeType;
use lofty::prelude::*;
use rusqlite::{params, Connection};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Emitter};

const AUDIO: &[&str] = &["mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wav"];
const MAX_ART_BYTES: usize = 8 * 1024 * 1024;

/// Rescans all folders in the background. Does nothing if a scan is already running.
pub fn rescan(app: AppHandle, lib: Arc<Library>) {
    if lib.scanning.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = app.emit("scan", json!({ "state": "started" }));
    tauri::async_runtime::spawn_blocking(move || {
        let result = scan(&app, &lib);
        lib.scanning.store(false, Ordering::SeqCst);
        let payload = match result {
            Ok((total, updated)) => json!({ "state": "done", "total": total, "updated": updated }),
            Err(e) => json!({ "state": "error", "message": e }),
        };
        let _ = app.emit("scan", payload);
        let _ = app.emit("library-changed", "local");
    });
}

fn scan(app: &AppHandle, lib: &Library) -> Result<(usize, usize), String> {
    // A separate connection, so the UI's queries aren't blocked during a long scan (WAL mode).
    let mut conn = Connection::open(&lib.db_path).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;

    let folders: Vec<String> = {
        let mut st = conn.prepare("SELECT path FROM folders").map_err(|e| e.to_string())?;
        let rows = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<_>>().map_err(|e| e.to_string())?
    };
    let known: HashMap<String, i64> = {
        let mut st = conn
            .prepare("SELECT path, mtime FROM tracks WHERE source = 'local'")
            .map_err(|e| e.to_string())?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0))))
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<_>>().map_err(|e| e.to_string())?
    };

    let mut files = Vec::new();
    for folder in &folders {
        walk(Path::new(folder), &mut files, 0);
    }

    let mut seen = HashSet::new();
    let mut updated = 0;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for (i, file) in files.iter().enumerate() {
        let path = file.to_string_lossy().to_string();
        seen.insert(path.clone());
        let mtime = std::fs::metadata(file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        if known.get(&path) == Some(&mtime) {
            continue;
        }
        let folder = folders.iter().find(|f| path.starts_with(f.as_str())).cloned().unwrap_or_default();
        let info = read_tags(file, &lib.art_dir);
        tx.execute(
            "INSERT INTO tracks (url, id, source, title, artist, album, duration, artwork, preview, path, folder, mtime, added_at)
             VALUES (?1, ?2, 'local', ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, ?11)
             ON CONFLICT(url) DO UPDATE SET
                title = excluded.title, artist = excluded.artist, album = excluded.album,
                duration = excluded.duration, artwork = excluded.artwork, folder = excluded.folder,
                mtime = excluded.mtime",
            params![
                format!("local:{path}"),
                short_hash(path.as_bytes()),
                info.title,
                info.artist,
                info.album,
                info.duration,
                info.artwork,
                path,
                folder,
                mtime,
                now()
            ],
        )
        .map_err(|e| e.to_string())?;
        updated += 1;
        if updated % 100 == 0 {
            let _ = app.emit("scan", json!({ "state": "progress", "done": i + 1, "total": files.len() }));
        }
    }

    // Files that disappeared from disk (or from the folder list) leave the library.
    let gone: Vec<&String> = known.keys().filter(|p| !seen.contains(*p)).collect();
    for path in gone {
        tx.execute("DELETE FROM tracks WHERE source = 'local' AND path = ?1", [path]).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok((files.len(), updated))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 24 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, out, depth + 1);
        } else if kind.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if AUDIO.contains(&ext.as_str()) {
                out.push(path);
            }
        }
    }
}

struct TagInfo {
    title: String,
    artist: String,
    album: Option<String>,
    duration: Option<f64>,
    artwork: Option<String>,
}

fn read_tags(file: &Path, art_dir: &Path) -> TagInfo {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    // Untagged files are often named "Artist - Title".
    let (guess_artist, guess_title) = match stem.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => (String::new(), stem.clone()),
    };
    let mut info = TagInfo { title: guess_title, artist: guess_artist, album: None, duration: None, artwork: None };

    let Ok(tagged) = lofty::read_from_path(file) else { return info };
    let secs = tagged.properties().duration().as_secs_f64();
    if secs > 0.0 {
        info.duration = Some(secs);
    }
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        if let Some(t) = tag.title().filter(|t| !t.trim().is_empty()) {
            info.title = t.trim().to_string();
        }
        if let Some(a) = tag.artist().filter(|a| !a.trim().is_empty()) {
            info.artist = a.trim().to_string();
        }
        info.album = tag.album().map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
        if let Some(pic) = tag.pictures().first().filter(|p| !p.data().is_empty() && p.data().len() <= MAX_ART_BYTES) {
            let ext = match pic.mime_type() {
                Some(MimeType::Png) => "png",
                _ => "jpg",
            };
            let name = format!("{}.{ext}", short_hash(pic.data()));
            let dest = art_dir.join(&name);
            if dest.exists() || std::fs::write(&dest, pic.data()).is_ok() {
                info.artwork = Some(format!("art:{name}"));
            }
        }
    }
    info
}

fn short_hash(data: &[u8]) -> String {
    Sha256::digest(data).iter().take(12).map(|b| format!("{b:02x}")).collect()
}
