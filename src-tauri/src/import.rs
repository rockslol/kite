//! Importing playlists and opening links.
//!
//! YouTube, YouTube Music and SoundCloud lists are read with yt-dlp and are playable as is.
//! Spotify (public embed, first 100 songs) and Deezer (public API) lists are matched to
//! YouTube Music songs; matches are remembered so a re-import or sync is fast and keeps any
//! version the user picked by hand.

use crate::ytdlp::{source_of, Track};
use crate::ytmusic::{self, Wanted, GOOD_ENOUGH};
use crate::AppState;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";
/// The Spotify embed lists at most this many songs.
const SPOTIFY_EMBED_LIMIT: usize = 100;

enum Items {
    Ready(Vec<Track>),
    Match(Vec<Wanted>),
}

struct Fetched {
    name: String,
    items: Items,
    truncated: bool,
}

#[derive(Debug, PartialEq)]
enum Link {
    Spotify { kind: String, id: String },
    Deezer { kind: String, id: String },
    YtList,
    YtSingle,
    ScList,
    ScSingle,
}

fn classify(url: &str) -> Option<Link> {
    let u = url.trim();
    let lower = u.to_lowercase();
    let path_parts = |host_end: &str| -> Vec<String> {
        let after = lower.split_once(host_end).map(|(_, p)| p).unwrap_or("");
        let path = after.split(['?', '#']).next().unwrap_or("");
        path.split('/').filter(|s| !s.is_empty()).map(String::from).collect()
    };
    let kind_id = |parts: &[String]| -> Option<(String, String)> {
        let i = parts.iter().position(|p| matches!(p.as_str(), "playlist" | "album" | "track"))?;
        // Keep the id's original case (Spotify ids are case sensitive).
        let id_lower = parts.get(i + 1)?;
        let start = lower.find(id_lower.as_str())?;
        Some((parts[i].clone(), u[start..start + id_lower.len()].to_string()))
    };

    if lower.contains("open.spotify.com/") {
        let (kind, id) = kind_id(&path_parts("open.spotify.com"))?;
        return Some(Link::Spotify { kind, id });
    }
    if lower.contains("deezer.com/") {
        let (kind, id) = kind_id(&path_parts("deezer.com"))?;
        return Some(Link::Deezer { kind, id });
    }
    if lower.contains("youtube.com/") || lower.contains("youtu.be/") {
        let list = lower.contains("list=") && !lower.contains("list=rd");
        let is_list = (list && !lower.contains("watch?")) || lower.contains("/playlist?");
        return Some(if is_list { Link::YtList } else { Link::YtSingle });
    }
    if lower.contains("soundcloud.com/") {
        let parts = path_parts("soundcloud.com");
        let list = parts.get(1).is_some_and(|p| matches!(p.as_str(), "sets" | "likes" | "tracks" | "albums"));
        return Some(if list { Link::ScList } else { Link::ScSingle });
    }
    None
}

/// Short links (spotify.link, link.deezer.com, deezer.page.link) redirect to the real page.
async fn expand(http: &reqwest::Client, url: &str) -> String {
    let lower = url.to_lowercase();
    if ["spotify.link", "spotify.app.link", "link.deezer.com", "deezer.page.link"].iter().any(|h| lower.contains(h)) {
        if let Ok(r) = http.get(url).header("User-Agent", UA).send().await {
            return r.url().to_string();
        }
    }
    url.to_string()
}

async fn fetch(state: &AppState, url: &str) -> Result<Fetched, String> {
    let url = expand(&state.http, url).await;
    match classify(&url).ok_or("That link isn't a Spotify, Deezer, YouTube or SoundCloud link.")? {
        Link::Spotify { kind, id } => spotify(&state.http, &kind, &id).await,
        Link::Deezer { kind, id } => deezer(&state.http, &kind, &id).await,
        Link::YtList | Link::ScList => {
            let (name, tracks) = state.yt.playlist(&url).await?;
            Ok(Fetched { name, items: Items::Ready(tracks), truncated: false })
        }
        Link::YtSingle | Link::ScSingle => {
            let t = state.yt.lookup(&url).await?;
            Ok(Fetched { name: t.title.clone(), items: Items::Ready(vec![t]), truncated: false })
        }
    }
}

async fn spotify(http: &reqwest::Client, kind: &str, id: &str) -> Result<Fetched, String> {
    let html = http
        .get(format!("https://open.spotify.com/embed/{kind}/{id}"))
        .header("User-Agent", UA)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Spotify didn't answer: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let start_tag = r#"<script id="__NEXT_DATA__" type="application/json">"#;
    let start = html.find(start_tag).ok_or("Couldn't read that Spotify page. Is the playlist public?")? + start_tag.len();
    let end = html[start..].find("</script>").ok_or("Couldn't read that Spotify page.")? + start;
    let v: Value = serde_json::from_str(&html[start..end]).map_err(|e| e.to_string())?;
    let entity = &v["props"]["pageProps"]["state"]["data"]["entity"];
    let name = entity["name"].as_str().or_else(|| entity["title"].as_str()).unwrap_or("Spotify playlist").to_string();

    let song = |t: &Value, artist_fallback: Option<&str>| -> Option<Wanted> {
        let title = t["title"].as_str().or_else(|| t["name"].as_str())?.to_string();
        let artist = t["subtitle"]
            .as_str()
            .map(|s| s.replace('\u{a0}', " "))
            .or_else(|| {
                t["artists"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
            })
            .or_else(|| artist_fallback.map(String::from))
            .unwrap_or_default();
        let duration = t["duration"].as_f64().map(|ms| ms / 1000.0);
        Some(Wanted { title, artist, album: None, duration })
    };

    let wanted: Vec<Wanted> = match entity["trackList"].as_array() {
        Some(list) => list.iter().filter_map(|t| song(t, None)).collect(),
        None => song(entity, None).into_iter().collect(),
    };
    if wanted.is_empty() {
        return Err("That Spotify link has no songs, or it's private.".into());
    }
    let truncated = kind == "playlist" && wanted.len() >= SPOTIFY_EMBED_LIMIT;
    Ok(Fetched { name, items: Items::Match(wanted), truncated })
}

async fn deezer(http: &reqwest::Client, kind: &str, id: &str) -> Result<Fetched, String> {
    let get = |url: String| async move {
        let v: Value = http
            .get(url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Deezer didn't answer: {e}"))?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(msg) = v["error"]["message"].as_str() {
            return Err(format!("Deezer: {msg}"));
        }
        Ok::<Value, String>(v)
    };
    let song = |t: &Value| -> Option<Wanted> {
        Some(Wanted {
            title: t["title"].as_str()?.to_string(),
            artist: t["artist"]["name"].as_str().unwrap_or("").to_string(),
            album: t["album"]["title"].as_str().map(String::from),
            duration: t["duration"].as_f64(),
        })
    };

    let head = get(format!("https://api.deezer.com/{kind}/{id}")).await?;
    if kind == "track" {
        let w = song(&head).ok_or("Couldn't read that Deezer track.")?;
        return Ok(Fetched { name: w.title.clone(), items: Items::Match(vec![w]), truncated: false });
    }
    let name = head["title"].as_str().unwrap_or("Deezer playlist").to_string();
    let first = head["tracks"]["data"].as_array().cloned().unwrap_or_default();
    // Page by how many entries Deezer returned, not how many parsed, so none are fetched twice.
    let mut fetched = first.len();
    let mut wanted: Vec<Wanted> = first.iter().filter_map(song).collect();
    let total = head["nb_tracks"].as_u64().unwrap_or(0) as usize;
    while kind == "playlist" && fetched < total && fetched < 5000 {
        let page = get(format!("https://api.deezer.com/playlist/{id}/tracks?index={fetched}&limit=100")).await?;
        let data = page["data"].as_array().cloned().unwrap_or_default();
        if data.is_empty() {
            break;
        }
        fetched += data.len();
        wanted.extend(data.iter().filter_map(song));
    }
    Ok(Fetched { name, items: Items::Match(wanted), truncated: false })
}

/// Matches songs to YouTube Music, 4 at a time, reporting progress. Keeps the input order;
/// `None` means no good match.
async fn match_all(app: &AppHandle, state: &AppState, job: &str, wanted: &[Wanted]) -> Vec<Option<Track>> {
    let mut out: Vec<Option<Track>> = vec![None; wanted.len()];
    let sem = Arc::new(Semaphore::new(4));
    let mut set = JoinSet::new();
    for (i, w) in wanted.iter().enumerate() {
        if let Some(t) = state.library.cached_match(&w.key()) {
            out[i] = Some(t);
            continue;
        }
        let (http, w, sem) = (state.http.clone(), w.clone(), sem.clone());
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            let best = ytmusic::candidates(&http, &w)
                .await
                .ok()
                .and_then(|c| c.into_iter().next())
                .filter(|(score, _)| *score >= GOOD_ENOUGH)
                .map(|(_, t)| t);
            (i, best)
        });
    }
    let total = wanted.len();
    let mut done = total - set.len();
    let _ = app.emit("import", json!({ "job": job, "state": "matching", "done": done, "total": total }));
    while let Some(Ok((i, best))) = set.join_next().await {
        if let Some(t) = &best {
            state.library.save_match(&wanted[i].key(), t);
        }
        out[i] = best;
        done += 1;
        if done % 5 == 0 || done == total {
            let _ = app.emit("import", json!({ "job": job, "state": "matching", "done": done, "total": total }));
        }
    }
    out
}

#[derive(Serialize)]
pub struct ImportResult {
    playlist_id: i64,
    name: String,
    total: usize,
    added: usize,
    missing: Vec<String>,
    truncated: bool,
}

/// Imports a playlist link into a new playlist, or with `playlist_id` syncs an imported
/// playlist (adds songs that are new at the source).
#[tauri::command]
pub async fn import_link(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    job: String,
    playlist_id: Option<i64>,
) -> Result<ImportResult, String> {
    let url = match playlist_id {
        Some(id) => state.library.playlist_source(id)?.ok_or("This playlist wasn't imported from a link.")?,
        None => url.trim().to_string(),
    };
    let _ = app.emit("import", json!({ "job": job, "state": "reading" }));
    let fetched = fetch(&state, &url).await?;

    let (items, missing): (Vec<(Track, Option<String>)>, Vec<String>) = match fetched.items {
        Items::Ready(tracks) => (tracks.into_iter().map(|t| (t, None)).collect(), vec![]),
        Items::Match(wanted) => {
            let matched = match_all(&app, &state, &job, &wanted).await;
            let mut items = Vec::new();
            let mut missing = Vec::new();
            for (w, m) in wanted.iter().zip(matched) {
                match m {
                    Some(t) => items.push((t, serde_json::to_string(w).ok())),
                    None => missing.push(if w.artist.is_empty() { w.title.clone() } else { format!("{} - {}", w.artist, w.title) }),
                }
            }
            (items, missing)
        }
    };
    let total = items.len() + missing.len();
    if items.is_empty() {
        return Err("None of the songs could be found.".into());
    }

    let id = match playlist_id {
        Some(id) => id,
        None => state.library.create_playlist(&fetched.name, Some(&url))?,
    };
    let added = state.library.add_items(id, &items)?;
    let _ = app.emit("library-changed", "playlists");
    let _ = app.emit("import", json!({ "job": job, "state": "done" }));
    Ok(ImportResult { playlist_id: id, name: fetched.name, total, added, missing, truncated: fetched.truncated })
}

/// What a pasted link is: `{ kind: "list" }` for playlists/albums (import them), or
/// `{ kind: "track", track }` for a single song (play it).
#[tauri::command]
pub async fn open_link(state: State<'_, AppState>, url: String) -> Result<Value, String> {
    let url = expand(&state.http, url.trim()).await;
    let link = classify(&url).ok_or("That link isn't a Spotify, Deezer, YouTube or SoundCloud link.")?;
    let single = match &link {
        Link::Spotify { kind, .. } | Link::Deezer { kind, .. } => kind == "track",
        Link::YtSingle | Link::ScSingle => true,
        _ => false,
    };
    if !single {
        return Ok(json!({ "kind": "list" }));
    }
    let fetched = fetch(&state, &url).await?;
    let track = match fetched.items {
        Items::Ready(mut t) => t.pop(),
        Items::Match(w) => match w.first() {
            Some(w) => match state.library.cached_match(&w.key()) {
                Some(t) => Some(t),
                None => ytmusic::candidates(&state.http, w).await?.into_iter().next().map(|(_, t)| t),
            },
            None => None,
        },
    };
    let track = track.ok_or("Couldn't find that song.")?;
    Ok(json!({ "kind": "track", "track": track, "source": source_of(&track.url) }))
}

/// Other versions for a playlist song, best first, for "Wrong song? Pick another".
#[tauri::command]
pub async fn match_alternatives(state: State<'_, AppState>, playlist_id: i64, item_id: i64) -> Result<Vec<Track>, String> {
    let (current, origin) = state.library.item(playlist_id, item_id)?;
    let want = origin
        .and_then(|o| serde_json::from_str::<Wanted>(&o).ok())
        .unwrap_or(Wanted { title: current.title.clone(), artist: current.artist.clone(), album: None, duration: current.duration });
    let found = ytmusic::candidates(&state.http, &want).await?;
    Ok(found.into_iter().map(|(_, t)| t).filter(|t| t.url != current.url).take(8).collect())
}

/// Swaps a playlist song for another version and remembers the choice for future imports.
#[tauri::command]
pub fn match_choose(app: AppHandle, state: State<'_, AppState>, playlist_id: i64, item_id: i64, track: Track) -> Result<(), String> {
    let (_, origin) = state.library.item(playlist_id, item_id)?;
    state.library.replace_item(playlist_id, item_id, &track)?;
    if let Some(w) = origin.and_then(|o| serde_json::from_str::<Wanted>(&o).ok()) {
        state.library.save_match(&w.key(), &track);
    }
    let _ = app.emit("library-changed", "playlists");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_links() {
        assert_eq!(
            classify("https://open.spotify.com/playlist/37i9dQZF1DX4UtSsGT1Sbe?si=abc"),
            Some(Link::Spotify { kind: "playlist".into(), id: "37i9dQZF1DX4UtSsGT1Sbe".into() })
        );
        assert_eq!(
            classify("https://open.spotify.com/intl-de/track/2LlQb7Uoj1kKyGhlkBf9aC"),
            Some(Link::Spotify { kind: "track".into(), id: "2LlQb7Uoj1kKyGhlkBf9aC".into() })
        );
        assert_eq!(
            classify("https://www.deezer.com/en/playlist/1313621735"),
            Some(Link::Deezer { kind: "playlist".into(), id: "1313621735".into() })
        );
        assert_eq!(classify("https://www.youtube.com/playlist?list=PLx"), Some(Link::YtList));
        assert_eq!(classify("https://music.youtube.com/playlist?list=OLAK5uy_x"), Some(Link::YtList));
        assert_eq!(classify("https://www.youtube.com/watch?v=abc&list=PLx"), Some(Link::YtSingle));
        assert_eq!(classify("https://youtu.be/abc"), Some(Link::YtSingle));
        assert_eq!(classify("https://soundcloud.com/che/sets/rest-in-bass"), Some(Link::ScList));
        assert_eq!(classify("https://soundcloud.com/che/2-saska-you-made-it"), Some(Link::ScSingle));
        assert_eq!(classify("https://example.com"), None);
    }
}
