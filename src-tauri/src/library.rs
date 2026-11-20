//! The library: liked songs, playlists, play history and local files, in one SQLite file.
//!
//! Every track the library knows about lives in `tracks`, keyed by its URL. Online tracks use
//! their page URL; local files use `local:<absolute path>`.

use crate::stream::{mime_for, serve_path, text};
use crate::ytdlp::Track;
use percent_encoding::percent_decode_str;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Emitter, State};

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS tracks (
    url       TEXT PRIMARY KEY,
    id        TEXT NOT NULL,
    source    TEXT NOT NULL,
    title     TEXT NOT NULL,
    artist    TEXT NOT NULL DEFAULT '',
    album     TEXT,
    duration  REAL,
    artwork   TEXT,
    preview   INTEGER NOT NULL DEFAULT 0,
    path      TEXT,
    folder    TEXT,
    mtime     INTEGER,
    added_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS tracks_source ON tracks(source);
CREATE TABLE IF NOT EXISTS likes (
    url       TEXT PRIMARY KEY REFERENCES tracks(url) ON DELETE CASCADE,
    liked_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS playlists (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS playlist_items (
    id           INTEGER PRIMARY KEY,
    playlist_id  INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    url          TEXT NOT NULL REFERENCES tracks(url) ON DELETE CASCADE,
    position     INTEGER NOT NULL,
    added_at     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS playlist_items_order ON playlist_items(playlist_id, position);
CREATE TABLE IF NOT EXISTS plays (
    url        TEXT NOT NULL REFERENCES tracks(url) ON DELETE CASCADE,
    played_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS plays_url ON plays(url);
CREATE INDEX IF NOT EXISTS plays_at ON plays(played_at);
CREATE TABLE IF NOT EXISTS folders (
    path      TEXT PRIMARY KEY,
    added_at  INTEGER NOT NULL
);
";

const COLS: &str = "t.id, t.source, t.url, t.title, t.artist, t.album, t.duration, t.artwork, t.preview";

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn row_track(r: &Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        source: r.get(1)?,
        url: r.get(2)?,
        title: r.get(3)?,
        artist: r.get(4)?,
        album: r.get(5)?,
        duration: r.get(6)?,
        artwork: r.get(7)?,
        preview: r.get::<_, i64>(8)? != 0,
    })
}

/// Saves an online track's metadata. Local tracks are owned by the scanner and never
/// overwritten from the UI.
pub fn upsert(conn: &Connection, t: &Track) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tracks (url, id, source, title, artist, album, duration, artwork, preview, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(url) DO UPDATE SET
            title = excluded.title,
            artist = excluded.artist,
            album = COALESCE(excluded.album, tracks.album),
            duration = COALESCE(excluded.duration, tracks.duration),
            artwork = COALESCE(excluded.artwork, tracks.artwork),
            preview = excluded.preview
         WHERE tracks.source <> 'local'",
        params![t.url, t.id, t.source, t.title, t.artist, t.album, t.duration, t.artwork, t.preview as i64, now()],
    )?;
    Ok(())
}

pub struct Library {
    conn: Mutex<Connection>,
    pub db_path: PathBuf,
    pub art_dir: PathBuf,
    pub scanning: AtomicBool,
}

impl Library {
    pub fn open(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(err)?;
        let db_path = data_dir.join("library.db");
        let conn = Connection::open(&db_path).map_err(err)?;
        conn.execute_batch(SCHEMA).map_err(err)?;
        let art_dir = data_dir.join("artwork");
        std::fs::create_dir_all(&art_dir).map_err(err)?;
        Ok(Self { conn: Mutex::new(conn), db_path, art_dir, scanning: AtomicBool::new(false) })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T, String> {
        let mut conn = self.conn.lock().map_err(err)?;
        f(&mut conn).map_err(err)
    }

    fn tracks(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Track>, String> {
        self.with(|c| {
            let mut st = c.prepare(sql)?;
            let rows = st.query_map(p, row_track)?;
            rows.collect()
        })
    }

    /// Serves `/file?u=<local url>` (audio) and `/art/<name>` (artwork) to the webview.
    pub async fn serve(&self, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
        let path = req.uri().path();
        let range = req
            .headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(crate::stream::parse_range);

        if let Some(name) = path.strip_prefix("/art/") {
            let safe = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
            if !safe {
                return text(StatusCode::BAD_REQUEST, "bad name");
            }
            let file = self.art_dir.join(name);
            let mime = if name.ends_with(".png") { "image/png" } else { "image/jpeg" };
            return serve_path(&file, mime, range).await;
        }

        let url = req
            .uri()
            .query()
            .and_then(|q| q.split('&').find_map(|p| p.strip_prefix("u=")))
            .map(|v| percent_decode_str(v).decode_utf8_lossy().into_owned());
        let Some(url) = url else { return text(StatusCode::BAD_REQUEST, "missing track") };
        // Only files the scanner put in the library can be read.
        let file: Option<String> = self
            .with(|c| {
                c.query_row("SELECT path FROM tracks WHERE url = ?1 AND source = 'local'", [&url], |r| r.get(0))
                    .optional()
            })
            .ok()
            .flatten();
        let Some(file) = file else { return text(StatusCode::NOT_FOUND, "not in library") };
        let ext = Path::new(&file).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        serve_path(Path::new(&file), mime_for(&ext), range).await
    }
}

type Lib<'a> = State<'a, Arc<Library>>;

fn changed(app: &AppHandle, what: &str) {
    let _ = app.emit("library-changed", what);
}

// ---- likes ----------------------------------------------------------------

#[tauri::command]
pub fn lib_like(app: AppHandle, lib: Lib<'_>, track: Track, liked: bool) -> Result<(), String> {
    lib.with(|c| {
        if liked {
            upsert(c, &track)?;
            c.execute("INSERT OR IGNORE INTO likes (url, liked_at) VALUES (?1, ?2)", params![track.url, now()])?;
        } else {
            c.execute("DELETE FROM likes WHERE url = ?1", [&track.url])?;
        }
        Ok(())
    })?;
    changed(&app, "likes");
    Ok(())
}

#[tauri::command]
pub fn lib_liked(lib: Lib<'_>) -> Result<Vec<Track>, String> {
    lib.tracks(&format!("SELECT {COLS} FROM likes l JOIN tracks t ON t.url = l.url ORDER BY l.liked_at DESC"), [])
}

#[tauri::command]
pub fn lib_liked_urls(lib: Lib<'_>) -> Result<Vec<String>, String> {
    lib.with(|c| {
        let mut st = c.prepare("SELECT url FROM likes")?;
        let rows = st.query_map([], |r| r.get(0))?;
        rows.collect()
    })
}

// ---- playlists ------------------------------------------------------------

#[derive(Serialize)]
pub struct PlaylistInfo {
    id: i64,
    name: String,
    count: i64,
    covers: Vec<String>,
}

#[derive(Serialize)]
pub struct PlaylistItem {
    item_id: i64,
    #[serde(flatten)]
    track: Track,
}

#[tauri::command]
pub fn lib_playlists(lib: Lib<'_>) -> Result<Vec<PlaylistInfo>, String> {
    lib.with(|c| {
        let mut st = c.prepare(
            "SELECT p.id, p.name, COUNT(i.id) FROM playlists p
             LEFT JOIN playlist_items i ON i.playlist_id = p.id
             GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
        )?;
        let mut list: Vec<PlaylistInfo> = st
            .query_map([], |r| Ok(PlaylistInfo { id: r.get(0)?, name: r.get(1)?, count: r.get(2)?, covers: vec![] }))?
            .collect::<rusqlite::Result<_>>()?;
        let mut covers = c.prepare(
            "SELECT DISTINCT t.artwork FROM playlist_items i JOIN tracks t ON t.url = i.url
             WHERE i.playlist_id = ?1 AND t.artwork IS NOT NULL ORDER BY i.position LIMIT 4",
        )?;
        for p in &mut list {
            p.covers = covers.query_map([p.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        }
        Ok(list)
    })
}

#[tauri::command]
pub fn lib_playlist_create(app: AppHandle, lib: Lib<'_>, name: String) -> Result<i64, String> {
    let name = name.trim();
    let name = if name.is_empty() { "New playlist" } else { name };
    let id = lib.with(|c| {
        c.execute("INSERT INTO playlists (name, created_at, updated_at) VALUES (?1, ?2, ?2)", params![name, now()])?;
        Ok(c.last_insert_rowid())
    })?;
    changed(&app, "playlists");
    Ok(id)
}

#[tauri::command]
pub fn lib_playlist_rename(app: AppHandle, lib: Lib<'_>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("A playlist needs a name.".into());
    }
    lib.with(|c| c.execute("UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3", params![name, now(), id]))?;
    changed(&app, "playlists");
    Ok(())
}

#[tauri::command]
pub fn lib_playlist_delete(app: AppHandle, lib: Lib<'_>, id: i64) -> Result<(), String> {
    lib.with(|c| c.execute("DELETE FROM playlists WHERE id = ?1", [id]))?;
    changed(&app, "playlists");
    Ok(())
}

#[tauri::command]
pub fn lib_playlist_items(lib: Lib<'_>, id: i64) -> Result<Vec<PlaylistItem>, String> {
    lib.with(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {COLS}, i.id FROM playlist_items i JOIN tracks t ON t.url = i.url
             WHERE i.playlist_id = ?1 ORDER BY i.position"
        ))?;
        let rows = st.query_map([id], |r| Ok(PlaylistItem { track: row_track(r)?, item_id: r.get(9)? }))?;
        rows.collect()
    })
}

/// Adds tracks to the end of a playlist, skipping ones already in it. Returns how many were added.
#[tauri::command]
pub fn lib_playlist_add(app: AppHandle, lib: Lib<'_>, id: i64, tracks: Vec<Track>) -> Result<usize, String> {
    let added = lib.with(|c| {
        let tx = c.transaction()?;
        let mut pos: i64 =
            tx.query_row("SELECT COALESCE(MAX(position), -1) FROM playlist_items WHERE playlist_id = ?1", [id], |r| {
                r.get(0)
            })?;
        let mut added = 0;
        for t in &tracks {
            upsert(&tx, t)?;
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM playlist_items WHERE playlist_id = ?1 AND url = ?2)",
                params![id, t.url],
                |r| r.get(0),
            )?;
            if exists {
                continue;
            }
            pos += 1;
            tx.execute(
                "INSERT INTO playlist_items (playlist_id, url, position, added_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, t.url, pos, now()],
            )?;
            added += 1;
        }
        tx.execute("UPDATE playlists SET updated_at = ?1 WHERE id = ?2", params![now(), id])?;
        tx.commit()?;
        Ok(added)
    })?;
    changed(&app, "playlists");
    Ok(added)
}

#[tauri::command]
pub fn lib_playlist_remove(app: AppHandle, lib: Lib<'_>, id: i64, item_id: i64) -> Result<(), String> {
    lib.with(|c| c.execute("DELETE FROM playlist_items WHERE id = ?1 AND playlist_id = ?2", params![item_id, id]))?;
    changed(&app, "playlists");
    Ok(())
}

/// Moves an item to index `to` and renumbers the playlist.
#[tauri::command]
pub fn lib_playlist_move(app: AppHandle, lib: Lib<'_>, id: i64, item_id: i64, to: usize) -> Result<(), String> {
    lib.with(|c| {
        let tx = c.transaction()?;
        let mut ids: Vec<i64> = {
            let mut st = tx.prepare("SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position")?;
            let rows = st.query_map([id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        if let Some(from) = ids.iter().position(|&i| i == item_id) {
            let moved = ids.remove(from);
            ids.insert(to.min(ids.len()), moved);
            for (pos, item) in ids.iter().enumerate() {
                tx.execute("UPDATE playlist_items SET position = ?1 WHERE id = ?2", params![pos as i64, item])?;
            }
        }
        tx.commit()
    })?;
    changed(&app, "playlists");
    Ok(())
}

// ---- history --------------------------------------------------------------

#[derive(Serialize)]
pub struct Played {
    #[serde(flatten)]
    track: Track,
    plays: i64,
    last_played: i64,
}

#[tauri::command]
pub fn lib_played(app: AppHandle, lib: Lib<'_>, track: Track) -> Result<(), String> {
    lib.with(|c| {
        upsert(c, &track)?;
        c.execute("INSERT INTO plays (url, played_at) VALUES (?1, ?2)", params![track.url, now()])
    })?;
    changed(&app, "history");
    Ok(())
}

/// `order` is "recent" or "top". `since` limits "top" to plays after a unix time.
#[tauri::command]
pub fn lib_history(lib: Lib<'_>, order: String, since: Option<i64>, limit: Option<i64>) -> Result<Vec<Played>, String> {
    let order_by = if order == "top" { "plays DESC, last DESC" } else { "last DESC" };
    lib.with(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {COLS}, COUNT(*) AS plays, MAX(p.played_at) AS last
             FROM plays p JOIN tracks t ON t.url = p.url
             WHERE p.played_at >= ?1
             GROUP BY p.url ORDER BY {order_by} LIMIT ?2"
        ))?;
        let rows = st.query_map(params![since.unwrap_or(0), limit.unwrap_or(200)], |r| {
            Ok(Played { track: row_track(r)?, plays: r.get(9)?, last_played: r.get(10)? })
        })?;
        rows.collect()
    })
}

#[tauri::command]
pub fn lib_clear_history(app: AppHandle, lib: Lib<'_>) -> Result<(), String> {
    lib.with(|c| c.execute("DELETE FROM plays", []))?;
    changed(&app, "history");
    Ok(())
}

// ---- search ---------------------------------------------------------------

/// Library tracks (liked, in a playlist, or local) matching the query.
#[tauri::command]
pub fn lib_search(lib: Lib<'_>, query: String) -> Result<Vec<Track>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let pattern = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    lib.tracks(
        &format!(
            "SELECT {COLS} FROM tracks t
             WHERE (t.title LIKE ?1 ESCAPE '\\' OR t.artist LIKE ?1 ESCAPE '\\' OR t.album LIKE ?1 ESCAPE '\\')
               AND (t.source = 'local'
                    OR t.url IN (SELECT url FROM likes)
                    OR t.url IN (SELECT url FROM playlist_items))
             ORDER BY (t.url IN (SELECT url FROM likes)) DESC, t.title COLLATE NOCASE
             LIMIT 8"
        ),
        [pattern],
    )
}

// ---- local files ----------------------------------------------------------

#[tauri::command]
pub fn lib_local_tracks(lib: Lib<'_>) -> Result<Vec<Track>, String> {
    lib.tracks(
        &format!(
            "SELECT {COLS} FROM tracks t WHERE t.source = 'local'
             ORDER BY t.artist COLLATE NOCASE, t.album COLLATE NOCASE, t.title COLLATE NOCASE"
        ),
        [],
    )
}

#[tauri::command]
pub fn lib_folders(lib: Lib<'_>) -> Result<Vec<String>, String> {
    lib.with(|c| {
        let mut st = c.prepare("SELECT path FROM folders ORDER BY path")?;
        let rows = st.query_map([], |r| r.get(0))?;
        rows.collect()
    })
}

#[tauri::command]
pub fn lib_add_folder(app: AppHandle, lib: Lib<'_>, path: String) -> Result<(), String> {
    lib.with(|c| c.execute("INSERT OR IGNORE INTO folders (path, added_at) VALUES (?1, ?2)", params![path, now()]))?;
    crate::local::rescan(app, lib.inner().clone());
    Ok(())
}

#[tauri::command]
pub fn lib_remove_folder(app: AppHandle, lib: Lib<'_>, path: String) -> Result<(), String> {
    lib.with(|c| {
        c.execute("DELETE FROM folders WHERE path = ?1", [&path])?;
        c.execute("DELETE FROM tracks WHERE source = 'local' AND folder = ?1", [&path])
    })?;
    changed(&app, "local");
    Ok(())
}

#[tauri::command]
pub fn lib_rescan(app: AppHandle, lib: Lib<'_>) {
    crate::local::rescan(app, lib.inner().clone());
}

#[tauri::command]
pub fn lib_scanning(lib: Lib<'_>) -> bool {
    lib.scanning.load(Ordering::Relaxed)
}
