//! Lyrics: a `.lrc` (or `.txt`) file next to a local song, else LRCLIB (lrclib.net: free, no
//! key). Answers are cached in the library, misses too, so each song is looked up once.

use crate::library::now;
use crate::ytdlp::Track;
use crate::AppState;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tauri::State;

const API: &str = "https://lrclib.net/api";
/// A song with no lyrics is asked about again after this long (they get added over time).
const RETRY_MISSES_AFTER: i64 = 3 * 24 * 60 * 60;
/// LRCLIB matches by length; a different cut of the song is usually a few seconds off.
const MAX_LENGTH_DIFF: f64 = 3.0;

#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub struct Lyrics {
    /// LRC text with `[mm:ss.xx]` timestamps.
    synced: Option<String>,
    plain: Option<String>,
    instrumental: bool,
    /// "file", "lrclib" or "" when nothing was found.
    source: String,
    /// The user's timing nudge for this song, in milliseconds.
    offset_ms: i64,
}

#[derive(Deserialize, Debug)]
struct Found {
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default, rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(default, rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

#[tauri::command]
pub async fn lyrics(state: State<'_, AppState>, track: Track) -> Result<Lyrics, String> {
    let lib = state.library.clone();
    let url = track.url.clone();
    let offset_ms = lib
        .with(|c| c.query_row("SELECT offset_ms FROM lyrics WHERE url = ?1", [&url], |r| r.get(0)).optional())?
        .unwrap_or(0);

    if let Some(path) = url.strip_prefix("local:") {
        if let Some(mut l) = from_file(Path::new(path)) {
            l.offset_ms = offset_ms;
            return Ok(l);
        }
    }

    let cached = lib.with(|c| {
        c.query_row(
            "SELECT synced, plain, instrumental, fetched_at FROM lyrics WHERE url = ?1 AND fetched_at IS NOT NULL",
            [&url],
            |r| {
                Ok((
                    Lyrics {
                        synced: r.get(0)?,
                        plain: r.get(1)?,
                        instrumental: r.get::<_, i64>(2)? != 0,
                        source: "lrclib".into(),
                        offset_ms,
                    },
                    r.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
    })?;
    if let Some((mut l, at)) = cached {
        let found = l.synced.is_some() || l.plain.is_some() || l.instrumental;
        if found || now() - at < RETRY_MISSES_AFTER {
            if !found {
                l.source.clear();
            }
            return Ok(l);
        }
    }

    let found = fetch(&state.http, &track).await?;
    let mut l = Lyrics { offset_ms, ..Default::default() };
    if let Some(f) = found {
        l.synced = f.synced.filter(|s| !s.trim().is_empty());
        l.plain = f.plain.filter(|s| !s.trim().is_empty());
        l.instrumental = f.instrumental;
        l.source = "lrclib".into();
    }
    lib.with(|c| {
        c.execute(
            "INSERT INTO lyrics (url, synced, plain, instrumental, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(url) DO UPDATE SET synced = excluded.synced, plain = excluded.plain,
                instrumental = excluded.instrumental, fetched_at = excluded.fetched_at",
            params![url, l.synced, l.plain, l.instrumental as i64, now()],
        )
    })?;
    if l.synced.is_none() && l.plain.is_none() && !l.instrumental {
        l.source.clear();
    }
    Ok(l)
}

/// Saves the timing nudge for a song.
#[tauri::command]
pub fn lyrics_offset(state: State<'_, AppState>, url: String, offset_ms: i64) -> Result<(), String> {
    let offset_ms = offset_ms.clamp(-30_000, 30_000);
    state.library.with(|c| {
        c.execute(
            "INSERT INTO lyrics (url, offset_ms) VALUES (?1, ?2) ON CONFLICT(url) DO UPDATE SET offset_ms = excluded.offset_ms",
            params![url, offset_ms],
        )
    })?;
    Ok(())
}

/// `song.lrc` (synced) or `song.txt` (plain) next to `song.mp3`.
fn from_file(audio: &Path) -> Option<Lyrics> {
    let read = |ext: &str| std::fs::read_to_string(audio.with_extension(ext)).ok().filter(|s| !s.trim().is_empty());
    if let Some(text) = read("lrc") {
        let timed = text.lines().any(|l| parse_time(l).is_some());
        return Some(Lyrics {
            synced: timed.then(|| text.clone()),
            plain: (!timed).then_some(text),
            source: "file".into(),
            ..Default::default()
        });
    }
    read("txt").map(|text| Lyrics { plain: Some(text), source: "file".into(), ..Default::default() })
}

/// Whether a line starts with an LRC timestamp like `[01:23.45]`.
fn parse_time(line: &str) -> Option<f64> {
    let inner = line.trim_start().strip_prefix('[')?.split(']').next()?;
    let (m, s) = inner.split_once(':')?;
    Some(m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?)
}

async fn fetch(http: &reqwest::Client, t: &Track) -> Result<Option<Found>, String> {
    let artist = main_artist(&t.artist);
    let title = t.title.trim();
    if artist.is_empty() || title.is_empty() {
        return Ok(None);
    }
    // 1. Exact match by artist, title, album and length.
    if let Some(d) = t.duration {
        let mut q = vec![("artist_name", artist.to_string()), ("track_name", title.to_string()), ("duration", format!("{}", d.round()))];
        if let Some(album) = t.album.as_deref().filter(|a| !a.is_empty()) {
            q.push(("album_name", album.to_string()));
        }
        if let Some(f) = get(http, "get", &q).await? {
            return Ok(Some(serde_json::from_value(f).map_err(|e| e.to_string())?));
        }
    }
    // 2. Search, with the title as it is and then without "(feat. …)" and the like.
    let bare = bare_title(title);
    for name in [title, bare.as_str()] {
        let q = [("artist_name", artist.to_string()), ("track_name", name.to_string())];
        if let Some(v) = get(http, "search", &q).await? {
            let list: Vec<Found> = serde_json::from_value(v).unwrap_or_default();
            if let Some(f) = best(list, t.duration) {
                return Ok(Some(f));
            }
        }
        if bare == title {
            break;
        }
    }
    Ok(None)
}

/// GET `{API}/{path}`; `None` on 404.
async fn get(http: &reqwest::Client, path: &str, query: &[(&str, String)]) -> Result<Option<serde_json::Value>, String> {
    let resp = http
        .get(format!("{API}/{path}"))
        .query(query)
        .header("User-Agent", concat!("Kite/", env!("CARGO_PKG_VERSION"), " (https://github.com/rockslol/kite)"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach LRCLIB: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("LRCLIB answered {}", resp.status()));
    }
    resp.json().await.map(Some).map_err(|e| e.to_string())
}

/// The closest-length result within `MAX_LENGTH_DIFF`, synced lyrics first.
fn best(list: Vec<Found>, duration: Option<f64>) -> Option<Found> {
    let diff = |f: &Found| match (f.duration, duration) {
        (Some(a), Some(b)) => (a - b).abs(),
        _ => 0.0,
    };
    list.into_iter()
        .filter(|f| diff(f) <= MAX_LENGTH_DIFF && (f.synced.is_some() || f.plain.is_some() || f.instrumental))
        .min_by(|a, b| {
            (a.synced.is_none(), diff(a)).partial_cmp(&(b.synced.is_none(), diff(b))).unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// "Che, Chuckyy" → "Che"; "A feat. B" → "A".
fn main_artist(artist: &str) -> &str {
    let lower = artist.to_lowercase();
    let mut end = artist.len();
    for sep in [",", " & ", " x ", " feat.", " feat ", " ft.", " with "] {
        if let Some(i) = lower.find(sep) {
            end = end.min(i);
        }
    }
    artist[..end].trim()
}

/// "Song (feat. X) [Remastered]" → "Song".
fn bare_title(title: &str) -> String {
    let cut = ["(", "[", " - ", " feat.", " ft."]
        .iter()
        .filter_map(|p| title.find(p))
        .filter(|&i| i > 0)
        .min()
        .unwrap_or(title.len());
    title[..cut].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(d: f64, synced: bool) -> Found {
        Found { duration: Some(d), synced: synced.then(|| "[00:01.00]x".into()), plain: Some("x".into()), instrumental: false }
    }

    #[test]
    fn picks_synced_lyrics_of_the_right_length() {
        let pick = best(vec![found(200.0, false), found(201.0, true), found(240.0, true)], Some(200.0)).unwrap();
        assert_eq!((pick.duration, pick.synced.is_some()), (Some(201.0), true));
        assert!(best(vec![found(260.0, true)], Some(200.0)).is_none(), "a different cut of the song");
    }

    #[test]
    fn cleans_artists_and_titles_for_searching() {
        assert_eq!(main_artist("Che, Chuckyy"), "Che");
        assert_eq!(main_artist("Buckshot & fakemink"), "Buckshot");
        assert_eq!(main_artist("OsamaSon"), "OsamaSon");
        assert_eq!(bare_title("CHECK (feat. OsamaSon)"), "CHECK");
        assert_eq!(bare_title("keep me going (BIRDBRAIN)"), "keep me going");
        assert_eq!(bare_title("Song - Remastered 2011"), "Song");
        assert_eq!(bare_title("(untitled)"), "(untitled)");
    }

    #[test]
    fn reads_lrc_timestamps() {
        assert_eq!(parse_time("[01:23.45] hello"), Some(83.45));
        assert_eq!(parse_time("[ar: Someone]"), None);
    }
}
