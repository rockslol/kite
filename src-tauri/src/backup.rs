//! Export the library to one JSON file and import it again: for backups and moving to a new PC.
//!
//! Importing merges into the library that's already there, so importing the same file twice
//! changes nothing the second time.

use crate::library::{now, row_track, Library};
use crate::ytdlp::Track;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// Bumped when the file layout changes in a way older versions can't read.
const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct Backup {
    kite_library: u32,
    exported_at: i64,
    tracks: Vec<SavedTrack>,
    likes: Vec<Like>,
    playlists: Vec<Playlist>,
    plays: Vec<Play>,
    folders: Vec<String>,
    #[serde(default)]
    matches: Vec<Match>,
}

#[derive(Serialize, Deserialize)]
struct SavedTrack {
    #[serde(flatten)]
    track: Track,
    /// Local files only: where the file is, and which music folder it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    folder: Option<String>,
    added_at: i64,
}

#[derive(Serialize, Deserialize)]
struct Like {
    url: String,
    liked_at: i64,
}

#[derive(Serialize, Deserialize)]
struct Playlist {
    name: String,
    #[serde(default)]
    source_url: Option<String>,
    created_at: i64,
    updated_at: i64,
    items: Vec<Item>,
}

#[derive(Serialize, Deserialize)]
struct Item {
    url: String,
    added_at: i64,
    #[serde(default)]
    origin: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Play {
    url: String,
    played_at: i64,
}

#[derive(Serialize, Deserialize)]
struct Match {
    key: String,
    track: String,
    updated_at: i64,
}

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Summary {
    songs: usize,
    likes: usize,
    playlists: usize,
    playlist_songs: usize,
    plays: usize,
    folders: usize,
}

fn rows<T>(c: &Connection, sql: &str, f: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>) -> rusqlite::Result<Vec<T>> {
    let mut st = c.prepare(sql)?;
    let out = st.query_map([], f)?.collect();
    out
}

/// Everything the library holds that the user made: likes, playlists, history, folders, and
/// the songs they point to.
pub fn export(c: &Connection) -> rusqlite::Result<Backup> {
    let tracks = rows(
        c,
        "SELECT t.id, t.source, t.url, t.title, t.artist, t.album, t.duration, t.artwork, t.preview, t.path, t.folder, t.added_at
         FROM tracks t
         WHERE t.url IN (SELECT url FROM likes UNION SELECT url FROM playlist_items UNION SELECT url FROM plays)
         ORDER BY t.added_at, t.url",
        |r| Ok(SavedTrack { track: row_track(r)?, path: r.get(9)?, folder: r.get(10)?, added_at: r.get(11)? }),
    )?;
    let likes = rows(c, "SELECT url, liked_at FROM likes ORDER BY liked_at", |r| {
        Ok(Like { url: r.get(0)?, liked_at: r.get(1)? })
    })?;
    let mut playlists = rows(
        c,
        "SELECT id, name, source_url, created_at, updated_at FROM playlists ORDER BY created_at, id",
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                Playlist { name: r.get(1)?, source_url: r.get(2)?, created_at: r.get(3)?, updated_at: r.get(4)?, items: vec![] },
            ))
        },
    )?;
    {
        let mut st = c.prepare("SELECT url, added_at, origin FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id")?;
        for (id, p) in &mut playlists {
            p.items = st
                .query_map([*id], |r| Ok(Item { url: r.get(0)?, added_at: r.get(1)?, origin: r.get(2)? }))?
                .collect::<rusqlite::Result<_>>()?;
        }
    }
    let plays = rows(c, "SELECT url, played_at FROM plays ORDER BY played_at", |r| {
        Ok(Play { url: r.get(0)?, played_at: r.get(1)? })
    })?;
    let folders = rows(c, "SELECT path FROM folders ORDER BY added_at", |r| r.get(0))?;
    let matches = rows(c, "SELECT key, track, updated_at FROM matches ORDER BY key", |r| {
        Ok(Match { key: r.get(0)?, track: r.get(1)?, updated_at: r.get(2)? })
    })?;
    Ok(Backup {
        kite_library: FORMAT,
        exported_at: now(),
        tracks,
        likes,
        playlists: playlists.into_iter().map(|(_, p)| p).collect(),
        plays,
        folders,
        matches,
    })
}

/// Merges a backup into the library. Songs, likes, plays and folders that are already there
/// are skipped. A playlist with the same name (and the same import source) gets the songs it
/// is missing added at the end; other playlists are created.
pub fn import(c: &mut Connection, b: &Backup) -> rusqlite::Result<Summary> {
    let tx = c.transaction()?;
    let mut sum = Summary::default();

    for f in &b.folders {
        sum.folders += tx.execute("INSERT OR IGNORE INTO folders (path, added_at) VALUES (?1, ?2)", params![f, now()])?;
    }
    for s in &b.tracks {
        let t = &s.track;
        // Local files come in with no mtime, so the next scan reads them again (or drops
        // them if the file isn't on this PC).
        sum.songs += tx.execute(
            "INSERT OR IGNORE INTO tracks (url, id, source, title, artist, album, duration, artwork, preview, path, folder, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                t.url, t.id, t.source, t.title, t.artist, t.album, t.duration, t.artwork, t.preview as i64, s.path, s.folder,
                s.added_at
            ],
        )?;
    }
    let known = |tx: &Connection, url: &str| -> rusqlite::Result<bool> {
        tx.query_row("SELECT EXISTS(SELECT 1 FROM tracks WHERE url = ?1)", [url], |r| r.get(0))
    };
    for l in &b.likes {
        if known(&tx, &l.url)? {
            sum.likes += tx.execute("INSERT OR IGNORE INTO likes (url, liked_at) VALUES (?1, ?2)", params![l.url, l.liked_at])?;
        }
    }
    for p in &b.plays {
        if known(&tx, &p.url)? {
            sum.plays += tx.execute(
                "INSERT INTO plays (url, played_at) SELECT ?1, ?2
                 WHERE NOT EXISTS (SELECT 1 FROM plays WHERE url = ?1 AND played_at = ?2)",
                params![p.url, p.played_at],
            )?;
        }
    }
    for p in &b.playlists {
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM playlists WHERE name = ?1 AND source_url IS ?2 ORDER BY id LIMIT 1",
                params![p.name, p.source_url],
                |r| r.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                tx.execute(
                    "INSERT INTO playlists (name, source_url, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                    params![p.name, p.source_url, p.created_at, p.updated_at],
                )?;
                sum.playlists += 1;
                tx.last_insert_rowid()
            }
        };
        let mut pos: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) FROM playlist_items WHERE playlist_id = ?1",
            [id],
            |r| r.get(0),
        )?;
        for it in &p.items {
            let have: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM playlist_items WHERE playlist_id = ?1 AND url = ?2)",
                params![id, it.url],
                |r| r.get(0),
            )?;
            if have || !known(&tx, &it.url)? {
                continue;
            }
            pos += 1;
            tx.execute(
                "INSERT INTO playlist_items (playlist_id, url, position, added_at, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, it.url, pos, it.added_at, it.origin],
            )?;
            sum.playlist_songs += 1;
        }
    }
    for m in &b.matches {
        tx.execute(
            "INSERT OR IGNORE INTO matches (key, track, updated_at) VALUES (?1, ?2, ?3)",
            params![m.key, m.track, m.updated_at],
        )?;
    }
    tx.commit()?;
    Ok(sum)
}

fn parse(text: &str) -> Result<Backup, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|_| "This isn't a Kite library file.".to_string())?;
    match v.get("kite_library").and_then(|f| f.as_u64()) {
        None => return Err("This isn't a Kite library file.".into()),
        Some(f) if f > FORMAT as u64 => {
            return Err("This library file was made by a newer version of Kite. Update Kite and try again.".into())
        }
        _ => {}
    }
    serde_json::from_value(v).map_err(|e| format!("This library file is damaged: {e}"))
}

type Lib<'a> = State<'a, Arc<Library>>;

#[derive(Serialize)]
pub struct Exported {
    songs: usize,
    playlists: usize,
}

#[tauri::command]
pub fn lib_export(lib: Lib<'_>, path: String) -> Result<Exported, String> {
    let b = lib.with(|c| export(c))?;
    let json = serde_json::to_vec_pretty(&b).map_err(|e| e.to_string())?;
    // Write next to the target first, so a failed write never leaves half a file behind.
    let tmp = format!("{path}.part");
    std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Couldn't save the file: {e}")
    })?;
    Ok(Exported { songs: b.tracks.len(), playlists: b.playlists.len() })
}

#[tauri::command]
pub async fn lib_import(app: AppHandle, lib: Lib<'_>, path: String) -> Result<Summary, String> {
    let text = tokio::fs::read_to_string(&path).await.map_err(|e| format!("Couldn't read the file: {e}"))?;
    let b = parse(&text)?;
    let sum = lib.with(|c| import(c, &b))?;
    for what in ["likes", "playlists", "history", "local"] {
        let _ = app.emit("library-changed", what);
    }
    if sum.folders > 0 || b.tracks.iter().any(|t| t.path.is_some()) {
        crate::local::rescan(app, lib.inner().clone());
    }
    Ok(sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: u32) -> Track {
        Track {
            id: n.to_string(),
            source: "youtube".into(),
            url: format!("https://www.youtube.com/watch?v={n}"),
            title: format!("Song {n}"),
            artist: "Artist".into(),
            album: None,
            duration: Some(200.0),
            artwork: None,
            preview: false,
        }
    }

    fn temp(name: &str) -> (Library, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("kite-backup-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Library::open(&dir).unwrap(), dir)
    }

    fn count(lib: &Library, table: &str) -> i64 {
        lib.with(|c| c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))).unwrap()
    }

    #[test]
    fn round_trip_and_second_import_changes_nothing() {
        let (a, dir_a) = temp("a");
        let p = a.create_playlist("Road trip", None).unwrap();
        a.add_items(p, &(1..=4).map(|n| (track(n), None)).collect::<Vec<_>>()).unwrap();
        let q = a.create_playlist("Imported", Some("https://www.deezer.com/playlist/9")).unwrap();
        a.add_items(q, &[(track(5), Some("{\"title\":\"x\"}".into()))]).unwrap();
        a.with(|c| {
            c.execute("INSERT INTO likes (url, liked_at) VALUES (?1, 100)", [track(2).url])?;
            c.execute("INSERT INTO plays (url, played_at) VALUES (?1, 200)", [track(3).url])?;
            c.execute("INSERT INTO plays (url, played_at) VALUES (?1, 300)", [track(3).url])
        })
        .unwrap();
        let text = serde_json::to_string(&a.with(|c| export(c)).unwrap()).unwrap();

        let (b, dir_b) = temp("b");
        // B already has one of the playlists, with one song of it.
        let existing = b.create_playlist("Road trip", None).unwrap();
        b.add_items(existing, &[(track(1), None)]).unwrap();

        let backup = parse(&text).unwrap();
        let first = b.with(|c| import(c, &backup)).unwrap();
        assert_eq!(first.playlists, 1, "only the Deezer one is new");
        assert_eq!(first.playlist_songs, 3 + 1, "Road trip gets the 3 it was missing");
        assert_eq!((first.likes, first.plays), (1, 2));
        assert_eq!(count(&b, "playlists"), 2);
        assert_eq!(count(&b, "playlist_items"), 5);

        let second = b.with(|c| import(c, &backup)).unwrap();
        assert_eq!(second, Summary::default());
        assert_eq!(count(&b, "playlist_items"), 5);
        assert_eq!(count(&b, "plays"), 2);

        drop((a, b));
        let _ = std::fs::remove_dir_all(dir_a);
        let _ = std::fs::remove_dir_all(dir_b);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse("not json").is_err());
        assert!(parse("{\"hello\": 1}").is_err());
        assert!(parse("{\"kite_library\": 99}").err().unwrap().contains("newer version"));
    }
}
