//! The library: liked songs, playlists, play history and local files, in one SQLite file.
//!
//! Every track the library knows about lives in `tracks`, keyed by its URL. Online tracks use
//! their page URL; local files use `local:<absolute path>`.

use crate::stream::{mime_for, serve_path, text};
use crate::ytdlp::Track;
use percent_encoding::percent_decode_str;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
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
CREATE TABLE IF NOT EXISTS matches (
    key         TEXT PRIMARY KEY,
    track       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS lyrics (
    url           TEXT PRIMARY KEY,
    synced        TEXT,
    plain         TEXT,
    instrumental  INTEGER NOT NULL DEFAULT 0,
    offset_ms     INTEGER NOT NULL DEFAULT 0,
    fetched_at    INTEGER
);
CREATE TABLE IF NOT EXISTS scrobble_queue (
    id          INTEGER PRIMARY KEY,
    track       TEXT NOT NULL,
    started_at  INTEGER NOT NULL
);
";

/// Columns added after the first release. `ALTER TABLE ADD COLUMN` fails if the column
/// exists, which is fine.
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE playlists ADD COLUMN source_url TEXT",
    "ALTER TABLE playlist_items ADD COLUMN origin TEXT",
];

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
        ..Default::default()
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

/// A playlist row as it was before a delete, for undo.
#[derive(Serialize, Deserialize)]
pub struct DeletedPlaylist {
    id: i64,
    name: String,
    source_url: Option<String>,
    created_at: i64,
    updated_at: i64,
    items: Vec<SavedItem>,
}

/// A playlist item as it was before it was removed, for undo.
#[derive(Serialize, Deserialize)]
pub struct SavedItem {
    url: String,
    position: i64,
    added_at: i64,
    origin: Option<String>,
}

fn saved_items(c: &Connection, playlist_id: i64, only: Option<&[i64]>) -> rusqlite::Result<Vec<SavedItem>> {
    let mut st = c.prepare(
        "SELECT id, url, position, added_at, origin FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id",
    )?;
    let rows = st.query_map([playlist_id], |r| {
        Ok((r.get::<_, i64>(0)?, SavedItem { url: r.get(1)?, position: r.get(2)?, added_at: r.get(3)?, origin: r.get(4)? }))
    })?;
    let mut out = vec![];
    for row in rows {
        let (id, item) = row?;
        if only.is_none_or(|ids| ids.contains(&id)) {
            out.push(item);
        }
    }
    Ok(out)
}

/// Puts saved items back. Songs whose track was removed from the library meanwhile
/// (a local folder that was taken out) are skipped.
fn insert_saved(c: &Connection, playlist_id: i64, items: &[SavedItem]) -> rusqlite::Result<()> {
    for it in items {
        c.execute(
            "INSERT INTO playlist_items (playlist_id, url, position, added_at, origin)
             SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM tracks WHERE url = ?2)",
            params![playlist_id, it.url, it.position, it.added_at, it.origin],
        )?;
    }
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
        for m in MIGRATIONS {
            let _ = conn.execute(m, []);
        }
        // Rows left behind by older scans that ran without foreign keys.
        let _ = conn.execute_batch(
            "DELETE FROM likes WHERE url NOT IN (SELECT url FROM tracks);
             DELETE FROM playlist_items WHERE url NOT IN (SELECT url FROM tracks);
             DELETE FROM plays WHERE url NOT IN (SELECT url FROM tracks);",
        );
        let art_dir = data_dir.join("artwork");
        std::fs::create_dir_all(&art_dir).map_err(err)?;
        Ok(Self { conn: Mutex::new(conn), db_path, art_dir, scanning: AtomicBool::new(false) })
    }

    pub fn create_playlist(&self, name: &str, source_url: Option<&str>) -> Result<i64, String> {
        self.with(|c| {
            c.execute(
                "INSERT INTO playlists (name, source_url, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                params![name, source_url, now()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Appends tracks (with the imported song they stand for, as JSON) to a playlist, skipping
    /// ones already in it. Returns how many were added.
    pub fn add_items(&self, id: i64, items: &[(Track, Option<String>)]) -> Result<usize, String> {
        self.with(|c| {
            let tx = c.transaction()?;
            let mut pos: i64 = tx.query_row(
                "SELECT COALESCE(MAX(position), -1) FROM playlist_items WHERE playlist_id = ?1",
                [id],
                |r| r.get(0),
            )?;
            let mut added = 0;
            for (t, origin) in items {
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
                    "INSERT INTO playlist_items (playlist_id, url, position, added_at, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, t.url, pos, now(), origin],
                )?;
                added += 1;
            }
            tx.execute("UPDATE playlists SET updated_at = ?1 WHERE id = ?2", params![now(), id])?;
            tx.commit()?;
            Ok(added)
        })
    }

    pub fn delete_playlist(&self, id: i64) -> Result<DeletedPlaylist, String> {
        self.with(|c| {
            let tx = c.transaction()?;
            let (name, source_url, created_at, updated_at) = tx.query_row(
                "SELECT name, source_url, created_at, updated_at FROM playlists WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            let items = saved_items(&tx, id, None)?;
            tx.execute("DELETE FROM playlists WHERE id = ?1", [id])?;
            tx.commit()?;
            Ok(DeletedPlaylist { id, name, source_url, created_at, updated_at, items })
        })
    }

    pub fn restore_playlist(&self, p: &DeletedPlaylist) -> Result<i64, String> {
        self.with(|c| {
            let tx = c.transaction()?;
            // Keep the old id unless a playlist made since has taken it.
            let free: bool = tx.query_row("SELECT NOT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)", [p.id], |r| r.get(0))?;
            tx.execute(
                "INSERT INTO playlists (id, name, source_url, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![free.then_some(p.id), p.name, p.source_url, p.created_at, p.updated_at],
            )?;
            let id = tx.last_insert_rowid();
            insert_saved(&tx, id, &p.items)?;
            tx.commit()?;
            Ok(id)
        })
    }

    pub fn remove_items(&self, playlist_id: i64, item_ids: &[i64]) -> Result<Vec<SavedItem>, String> {
        self.with(|c| {
            let tx = c.transaction()?;
            let removed = saved_items(&tx, playlist_id, Some(item_ids))?;
            for id in item_ids {
                tx.execute("DELETE FROM playlist_items WHERE id = ?1 AND playlist_id = ?2", params![id, playlist_id])?;
            }
            tx.execute("UPDATE playlists SET updated_at = ?1 WHERE id = ?2", params![now(), playlist_id])?;
            tx.commit()?;
            Ok(removed)
        })
    }

    /// Removing leaves gaps in `position`, so putting items back at their old position puts
    /// them back in order (unless the playlist was reordered in between, which renumbers it).
    pub fn restore_items(&self, playlist_id: i64, items: &[SavedItem]) -> Result<(), String> {
        self.with(|c| {
            let tx = c.transaction()?;
            insert_saved(&tx, playlist_id, items)?;
            tx.commit()
        })
    }

    pub fn playlist_source(&self, id: i64) -> Result<Option<String>, String> {
        self.with(|c| {
            c.query_row("SELECT source_url FROM playlists WHERE id = ?1", [id], |r| r.get(0))
                .optional()
                .map(Option::flatten)
        })
    }

    /// The playlist item's track and the imported song it stands for (if it was imported).
    pub fn item(&self, playlist_id: i64, item_id: i64) -> Result<(Track, Option<String>), String> {
        self.with(|c| {
            c.query_row(
                &format!(
                    "SELECT {COLS}, i.origin FROM playlist_items i JOIN tracks t ON t.url = i.url
                     WHERE i.id = ?1 AND i.playlist_id = ?2"
                ),
                params![item_id, playlist_id],
                |r| Ok((row_track(r)?, r.get(9)?)),
            )
        })
    }

    pub fn replace_item(&self, playlist_id: i64, item_id: i64, track: &Track) -> Result<(), String> {
        self.with(|c| {
            upsert(c, track)?;
            let elsewhere: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM playlist_items WHERE playlist_id = ?1 AND url = ?2 AND id <> ?3)",
                params![playlist_id, track.url, item_id],
                |r| r.get(0),
            )?;
            if elsewhere {
                // The chosen version is already in the playlist: drop this entry instead of doubling it.
                c.execute("DELETE FROM playlist_items WHERE id = ?1 AND playlist_id = ?2", params![item_id, playlist_id])?;
            } else {
                c.execute(
                    "UPDATE playlist_items SET url = ?1 WHERE id = ?2 AND playlist_id = ?3",
                    params![track.url, item_id, playlist_id],
                )?;
            }
            Ok(())
        })
    }

    pub fn cached_match(&self, key: &str) -> Option<Track> {
        let json: Option<String> = self
            .with(|c| c.query_row("SELECT track FROM matches WHERE key = ?1", [key], |r| r.get(0)).optional())
            .ok()
            .flatten();
        json.and_then(|j| serde_json::from_str(&j).ok())
    }

    pub fn save_match(&self, key: &str, track: &Track) {
        let Ok(json) = serde_json::to_string(track) else { return };
        let _ = self.with(|c| {
            c.execute(
                "INSERT INTO matches (key, track, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET track = excluded.track, updated_at = excluded.updated_at",
                params![key, json, now()],
            )
        });
    }

    pub(crate) fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T, String> {
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

    /// Serves `/file?u=<local url>` (audio), `/art/<name>` (artwork from local files) and
    /// `/img?u=<https url>` (remote artwork, fetched once and cached on disk) to the webview.
    pub async fn serve(&self, req: &Request<Vec<u8>>, http: &reqwest::Client) -> Response<Vec<u8>> {
        let path = req.uri().path();
        if path == "/img" {
            return self.remote_image(req, http).await;
        }
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

    /// Remote artwork goes through here instead of loading in the webview directly: WebView2's
    /// tracking prevention blocks some image hosts (yt3.googleusercontent.com), and a disk cache
    /// keeps covers showing offline.
    async fn remote_image(&self, req: &Request<Vec<u8>>, http: &reqwest::Client) -> Response<Vec<u8>> {
        let url = req
            .uri()
            .query()
            .and_then(|q| q.split('&').find_map(|p| p.strip_prefix("u=")))
            .map(|v| percent_decode_str(v).decode_utf8_lossy().into_owned())
            .unwrap_or_default();
        if !image_url_allowed(&url) {
            return text(StatusCode::FORBIDDEN, "image host not allowed");
        }
        let dir = self.art_dir.join("web");
        let name: String = {
            use sha2::{Digest, Sha256};
            Sha256::digest(url.as_bytes()).iter().take(12).map(|b| format!("{b:02x}")).collect()
        };
        let file = dir.join(&name);
        let bytes = match tokio::fs::read(&file).await {
            Ok(b) => b,
            Err(_) => {
                let fetched = http.get(&url).send().await.and_then(|r| r.error_for_status());
                let body = match fetched {
                    Ok(r) => r.bytes().await.map(|b| b.to_vec()),
                    Err(e) => Err(e),
                };
                match body {
                    Ok(b) if !b.is_empty() && b.len() < 4 * 1024 * 1024 => {
                        let _ = tokio::fs::create_dir_all(&dir).await;
                        let _ = tokio::fs::write(&file, &b).await;
                        b
                    }
                    Ok(_) => return text(StatusCode::BAD_GATEWAY, "bad image"),
                    Err(e) => return text(StatusCode::BAD_GATEWAY, &e.to_string()),
                }
            }
        };
        let mime = match bytes.get(..4) {
            Some([0x89, b'P', b'N', b'G']) => "image/png",
            Some([b'R', b'I', b'F', b'F']) => "image/webp",
            Some([b'G', b'I', b'F', _]) => "image/gif",
            _ => "image/jpeg",
        };
        Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(bytes)
            .unwrap()
    }
}

/// Only https images from the music services' image hosts go through the cover proxy. The URL is
/// parsed properly: a hand-split host could be fooled by `?`, `#` or `\` in the URL.
fn image_url_allowed(url: &str) -> bool {
    const HOSTS: &[&str] =
        &["googleusercontent.com", "ytimg.com", "ggpht.com", "sndcdn.com", "dzcdn.net", "scdn.co", "spotifycdn.com"];
    reqwest::Url::parse(url).ok().is_some_and(|u| {
        u.scheme() == "https"
            && u.host_str().is_some_and(|host| HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))))
    })
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
    source_url: Option<String>,
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
            "SELECT p.id, p.name, COUNT(i.id), p.source_url FROM playlists p
             LEFT JOIN playlist_items i ON i.playlist_id = p.id
             GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
        )?;
        let mut list: Vec<PlaylistInfo> = st
            .query_map([], |r| {
                Ok(PlaylistInfo { id: r.get(0)?, name: r.get(1)?, count: r.get(2)?, covers: vec![], source_url: r.get(3)? })
            })?
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

/// Deletes a playlist and returns what it held, so the delete can be undone.
#[tauri::command]
pub fn lib_playlist_delete(app: AppHandle, lib: Lib<'_>, id: i64) -> Result<DeletedPlaylist, String> {
    let gone = lib.delete_playlist(id)?;
    changed(&app, "playlists");
    Ok(gone)
}

/// Undo for `lib_playlist_delete`. Returns the playlist's id (a new one if the old id was reused).
#[tauri::command]
pub fn lib_playlist_restore(app: AppHandle, lib: Lib<'_>, playlist: DeletedPlaylist) -> Result<i64, String> {
    let id = lib.restore_playlist(&playlist)?;
    changed(&app, "playlists");
    Ok(id)
}

#[tauri::command]
pub fn lib_playlist_items(lib: Lib<'_>, id: i64) -> Result<Vec<PlaylistItem>, String> {
    lib.with(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {COLS}, i.id FROM playlist_items i JOIN tracks t ON t.url = i.url
             WHERE i.playlist_id = ?1 ORDER BY i.position, i.id"
        ))?;
        let rows = st.query_map([id], |r| Ok(PlaylistItem { track: row_track(r)?, item_id: r.get(9)? }))?;
        rows.collect()
    })
}

/// Adds tracks to the end of a playlist, skipping ones already in it. Returns how many were added.
#[tauri::command]
pub fn lib_playlist_add(app: AppHandle, lib: Lib<'_>, id: i64, tracks: Vec<Track>) -> Result<usize, String> {
    let items: Vec<(Track, Option<String>)> = tracks.into_iter().map(|t| (t, None)).collect();
    let added = lib.add_items(id, &items)?;
    changed(&app, "playlists");
    Ok(added)
}

/// Removes items from a playlist and returns them, so the removal can be undone.
#[tauri::command]
pub fn lib_playlist_remove(app: AppHandle, lib: Lib<'_>, id: i64, item_ids: Vec<i64>) -> Result<Vec<SavedItem>, String> {
    let removed = lib.remove_items(id, &item_ids)?;
    changed(&app, "playlists");
    Ok(removed)
}

/// Undo for `lib_playlist_remove`: puts the items back where they were.
#[tauri::command]
pub fn lib_playlist_unremove(app: AppHandle, lib: Lib<'_>, id: i64, items: Vec<SavedItem>) -> Result<(), String> {
    lib.restore_items(id, &items)?;
    changed(&app, "playlists");
    Ok(())
}

/// Moves an item to index `to` and renumbers the playlist.
#[tauri::command]
pub fn lib_playlist_move(app: AppHandle, lib: Lib<'_>, id: i64, item_id: i64, to: usize) -> Result<(), String> {
    lib.with(|c| {
        let tx = c.transaction()?;
        let mut ids: Vec<i64> = {
            let mut st = tx.prepare("SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id")?;
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
    crate::watch::refresh(&app);
    crate::local::rescan(app, lib.inner().clone());
    Ok(())
}

#[tauri::command]
pub fn lib_remove_folder(app: AppHandle, lib: Lib<'_>, path: String) -> Result<(), String> {
    lib.with(|c| {
        c.execute("DELETE FROM folders WHERE path = ?1", [&path])?;
        c.execute("DELETE FROM tracks WHERE source = 'local' AND folder = ?1", [&path])
    })?;
    crate::watch::refresh(&app);
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

#[cfg(test)]
mod tests {
    use super::image_url_allowed as ok;
    use super::*;

    fn track(n: u32) -> Track {
        Track {
            id: n.to_string(),
            source: "youtube".into(),
            url: format!("https://www.youtube.com/watch?v={n}"),
            title: format!("Song {n}"),
            artist: "Artist".into(),
            album: None,
            duration: Some(180.0),
            artwork: None,
            preview: false,
            ..Default::default()
        }
    }

    fn urls(lib: &Library, id: i64) -> Vec<String> {
        lib.tracks(
            &format!("SELECT {COLS} FROM playlist_items i JOIN tracks t ON t.url = i.url WHERE i.playlist_id = ?1 ORDER BY i.position, i.id"),
            [id],
        )
        .unwrap()
        .into_iter()
        .map(|t| t.url)
        .collect()
    }

    fn temp_library() -> (Library, PathBuf) {
        let dir = std::env::temp_dir().join(format!("kite-test-{}-{}", std::process::id(), now_nanos()));
        (Library::open(&dir).unwrap(), dir)
    }

    fn now_nanos() -> u128 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn removed_items_come_back_in_place() {
        let (lib, dir) = temp_library();
        let id = lib.create_playlist("Mix", None).unwrap();
        let items: Vec<_> = (1..=5).map(|n| (track(n), None)).collect();
        lib.add_items(id, &items).unwrap();
        let before = urls(&lib, id);
        let ids: Vec<i64> = lib.with(|c| {
            let mut st = c.prepare("SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position")?;
            let rows = st.query_map([id], |r| r.get(0))?;
            rows.collect()
        })
        .unwrap();
        let removed = lib.remove_items(id, &[ids[1], ids[3]]).unwrap();
        assert_eq!(removed.len(), 2);
        assert_eq!(urls(&lib, id).len(), 3);
        lib.restore_items(id, &removed).unwrap();
        assert_eq!(urls(&lib, id), before);
        drop(lib);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn deleted_playlist_comes_back() {
        let (lib, dir) = temp_library();
        let id = lib.create_playlist("Gone", Some("https://www.deezer.com/playlist/1")).unwrap();
        let items: Vec<_> = (1..=3).map(|n| (track(n), Some(format!("origin {n}")))).collect();
        lib.add_items(id, &items).unwrap();
        let before = urls(&lib, id);
        let gone = lib.delete_playlist(id).unwrap();
        assert!(urls(&lib, id).is_empty());
        assert_eq!(lib.restore_playlist(&gone).unwrap(), id);
        assert_eq!(urls(&lib, id), before);
        assert_eq!(lib.playlist_source(id).unwrap().as_deref(), Some("https://www.deezer.com/playlist/1"));

        // A playlist made after the delete takes the old id: the restored one gets a new id.
        let gone = lib.delete_playlist(id).unwrap();
        let other = lib.create_playlist("New", None).unwrap();
        assert_eq!(other, id);
        let back = lib.restore_playlist(&gone).unwrap();
        assert_ne!(back, id);
        assert_eq!(urls(&lib, back), before);
        drop(lib);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cover_proxy_only_allows_known_image_hosts() {
        assert!(ok("https://i.ytimg.com/vi/abc/mqdefault.jpg"));
        assert!(ok("https://yt3.googleusercontent.com/abc=w300-h300"));
        assert!(ok("https://cdn-images.dzcdn.net/images/cover/x.jpg"));
        assert!(!ok("http://i.ytimg.com/vi/abc/mqdefault.jpg"));
        assert!(!ok("https://evil.example?.ytimg.com"));
        assert!(!ok("https://evil.example#.ytimg.com"));
        assert!(!ok(r"https://evil.example\.ytimg.com/x"));
        assert!(!ok("https://ytimg.com.evil.example/x"));
    }
}
