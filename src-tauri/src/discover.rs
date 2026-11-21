//! Discovery from YouTube Music: artist pages, album pages and radio ("up next"), through the
//! same web endpoints its website uses. Playback still goes through yt-dlp.

use crate::ytdlp::Track;
use crate::ytmusic::{parse_duration, resize_art_to};
use crate::AppState;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

const BASE: &str = "https://music.youtube.com/youtubei/v1";
const CLIENT_VERSION: &str = "1.20251110.01.00";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";
const FILTER_ARTISTS: &str = "EgWKAQIgAWoQEAMQBBAJEAoQBRAREBAQFQ==";
const FILTER_ALBUMS: &str = "EgWKAQIYAWoQEAMQBBAJEAoQBRAREBAQFQ==";

async fn call(http: &reqwest::Client, endpoint: &str, body: Value) -> Result<Value, String> {
    let mut body = body;
    body["context"] = json!({ "client": { "clientName": "WEB_REMIX", "clientVersion": CLIENT_VERSION, "hl": "en", "gl": "US" } });
    http.post(format!("{BASE}/{endpoint}?prettyPrint=false"))
        .header("Origin", "https://music.youtube.com")
        .header("User-Agent", UA)
        .json(&body)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("YouTube Music didn't answer: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())
}

// ---- small readers ---------------------------------------------------------------------

fn text(runs: &Value) -> String {
    runs["runs"].as_array().into_iter().flatten().filter_map(|r| r["text"].as_str()).collect()
}

fn browse_id(v: &Value) -> Option<String> {
    v["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str().map(String::from)
}

/// The largest thumbnail, asked for at `size` px.
fn thumb(v: &Value, size: u32) -> Option<String> {
    v["thumbnails"].as_array()?.last()?["url"].as_str().map(|u| resize_art_to(u, size))
}

fn row_thumb(r: &Value, size: u32) -> Option<String> {
    thumb(&r["thumbnail"]["musicThumbnailRenderer"]["thumbnail"], size)
}

const SEP: &str = " \u{2022} ";

/// What a list row doesn't say itself (album pages leave out the artist and cover).
#[derive(Default)]
struct Fallback<'a> {
    artist: Option<(&'a str, Option<&'a str>)>,
    album: Option<(&'a str, &'a str)>,
    artwork: Option<&'a str>,
}

/// A song row (search results, top songs, album tracks).
fn parse_row(r: &Value, fb: &Fallback) -> Option<Track> {
    let id = r["playlistItemData"]["videoId"]
        .as_str()
        .or_else(|| r["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]["musicPlayButtonRenderer"]["playNavigationEndpoint"]["watchEndpoint"]["videoId"].as_str())?
        .to_string();
    let cols = r["flexColumns"].as_array()?;
    let col = |i: usize| cols.get(i).map(|c| &c["musicResponsiveListItemFlexColumnRenderer"]["text"]);
    let title = text(col(0)?);

    let mut artists: Vec<String> = vec![];
    let mut artist_id = None;
    let mut album = None;
    let mut album_id = None;
    let mut duration = None;
    for c in cols.iter().skip(1) {
        for run in c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array().into_iter().flatten() {
            let t = run["text"].as_str().unwrap_or("");
            match browse_id(run) {
                Some(b) if b.starts_with("UC") => {
                    artist_id.get_or_insert(b);
                    artists.push(t.to_string());
                }
                Some(b) if b.starts_with("MPREb") => {
                    album = Some(t.to_string());
                    album_id = Some(b);
                }
                _ => {
                    if let Some(d) = parse_duration(t.trim()) {
                        duration = Some(d);
                    }
                }
            }
        }
    }
    for c in r["fixedColumns"].as_array().into_iter().flatten() {
        if let Some(d) = parse_duration(text(&c["musicResponsiveListItemFixedColumnRenderer"]["text"]).trim()) {
            duration = Some(d);
        }
    }
    // Plain-text artists (no link): the first run of the second column, like the search did.
    if artists.is_empty() {
        if let Some((name, id)) = fb.artist {
            artists.push(name.to_string());
            artist_id = id.map(String::from);
        } else if let Some(first) = col(1).and_then(|c| c["runs"][0]["text"].as_str()).filter(|t| parse_duration(t).is_none()) {
            artists.push(first.to_string());
        }
    }
    if album.is_none() {
        if let Some((name, id)) = fb.album {
            album = Some(name.to_string());
            album_id = Some(id.to_string());
        }
    }
    Some(Track {
        url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        source: "youtube".into(),
        title,
        artist: artists.join(", "),
        album,
        duration,
        artwork: row_thumb(r, 300).or_else(|| fb.artwork.map(String::from)),
        preview: false,
        artist_id,
        album_id,
    })
}

/// Search rows, used by the normal song search too (so its results carry artist/album ids).
pub fn parse_search_row(r: &Value) -> Option<Track> {
    parse_row(r, &Fallback::default())
}

// ---- artist ------------------------------------------------------------------------------

/// An album, single, playlist or artist shown as a card.
#[derive(Serialize, Debug)]
pub struct Card {
    id: String,
    title: String,
    subtitle: String,
    artwork: Option<String>,
}

fn card(item: &Value) -> Option<Card> {
    let r = &item["musicTwoRowItemRenderer"];
    Some(Card {
        id: browse_id(r)?,
        title: text(&r["title"]),
        subtitle: text(&r["subtitle"]),
        artwork: thumb(&r["thumbnailRenderer"]["musicThumbnailRenderer"]["thumbnail"], 300),
    })
}

#[derive(Serialize, Default, Debug)]
pub struct Artist {
    id: String,
    name: String,
    artwork: Option<String>,
    /// "810K monthly audience" or "254K subscribers".
    audience: String,
    about: String,
    songs: Vec<Track>,
    /// The full song list, as a playlist to open.
    all_songs: Option<String>,
    albums: Vec<Card>,
    singles: Vec<Card>,
    similar: Vec<Card>,
    /// Playlist id for the artist's radio.
    radio: Option<String>,
}

pub fn parse_artist(id: &str, v: &Value) -> Artist {
    let h = &v["header"]["musicImmersiveHeaderRenderer"];
    let h = if h.is_null() { &v["header"]["musicVisualHeaderRenderer"] } else { h };
    let mut a = Artist {
        id: id.to_string(),
        name: text(&h["title"]),
        artwork: thumb(&h["thumbnail"]["musicThumbnailRenderer"]["thumbnail"], 600),
        radio: h["startRadioButton"]["buttonRenderer"]["navigationEndpoint"]["watchEndpoint"]["playlistId"].as_str().map(String::from),
        ..Default::default()
    };
    let monthly = text(&h["monthlyListenerCount"]);
    let subs = text(&h["subscriptionButton"]["subscribeButtonRenderer"]["subscriberCountText"]);
    a.audience = if !monthly.is_empty() { monthly } else if !subs.is_empty() { format!("{subs} subscribers") } else { String::new() };

    let name = a.name.clone();
    let sections = &v["contents"]["singleColumnBrowseResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]["sectionListRenderer"]["contents"];
    for s in sections.as_array().into_iter().flatten() {
        if let Some(shelf) = s.get("musicShelfRenderer") {
            let fb = Fallback { artist: Some((&name, Some(id))), ..Default::default() };
            a.songs = shelf["contents"].as_array().into_iter().flatten().filter_map(|i| parse_row(&i["musicResponsiveListItemRenderer"], &fb)).collect();
            a.all_songs = shelf["bottomEndpoint"]["browseEndpoint"]["browseId"]
                .as_str()
                .or_else(|| shelf["title"]["runs"][0]["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str())
                .map(String::from);
        } else if let Some(c) = s.get("musicCarouselShelfRenderer") {
            let title = text(&c["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]).to_lowercase();
            let cards: Vec<Card> = c["contents"].as_array().into_iter().flatten().filter_map(card).collect();
            if title == "albums" {
                a.albums = cards;
            } else if title.starts_with("singles") {
                a.singles = cards;
            } else if title.contains("might also like") || title.contains("similar") {
                a.similar = cards.into_iter().filter(|c| c.id.starts_with("UC")).collect();
            }
        } else if let Some(d) = s.get("musicDescriptionShelfRenderer") {
            a.about = text(&d["description"]);
        }
    }
    a
}

#[tauri::command]
pub async fn ytm_artist(state: State<'_, AppState>, id: String) -> Result<Artist, String> {
    let v = call(&state.http, "browse", json!({ "browseId": id })).await?;
    let a = parse_artist(&id, &v);
    if a.name.is_empty() {
        return Err("YouTube Music has no page for this artist.".into());
    }
    Ok(a)
}

/// All of an artist's songs (the playlist behind "Top songs → See all").
#[tauri::command]
pub async fn ytm_playlist_songs(state: State<'_, AppState>, id: String) -> Result<Vec<Track>, String> {
    let v = call(&state.http, "browse", json!({ "browseId": id })).await?;
    let shelf = &v["contents"]["twoColumnBrowseResultsRenderer"]["secondaryContents"]["sectionListRenderer"]["contents"][0];
    let items = shelf["musicPlaylistShelfRenderer"]["contents"].as_array().or_else(|| shelf["musicShelfRenderer"]["contents"].as_array());
    Ok(items.into_iter().flatten().filter_map(|i| parse_row(&i["musicResponsiveListItemRenderer"], &Fallback::default())).collect())
}

// ---- album -------------------------------------------------------------------------------

#[derive(Serialize, Default, Debug)]
pub struct Album {
    id: String,
    title: String,
    /// "Album", "Single" or "EP".
    kind: String,
    year: String,
    artist: String,
    artist_id: Option<String>,
    artwork: Option<String>,
    tracks: Vec<Track>,
}

pub fn parse_album(id: &str, v: &Value) -> Album {
    let tc = &v["contents"]["twoColumnBrowseResultsRenderer"];
    let h = &tc["tabs"][0]["tabRenderer"]["content"]["sectionListRenderer"]["contents"][0]["musicResponsiveHeaderRenderer"];
    let sub: Vec<String> = h["subtitle"]["runs"].as_array().into_iter().flatten().filter_map(|r| r["text"].as_str()).map(String::from).collect();
    let strap = &h["straplineTextOne"]["runs"];
    let mut al = Album {
        id: id.to_string(),
        title: text(&h["title"]),
        kind: sub.first().cloned().unwrap_or_default(),
        year: sub.iter().rev().find(|s| s.len() == 4 && s.chars().all(|c| c.is_ascii_digit())).cloned().unwrap_or_default(),
        artist: text(&h["straplineTextOne"]),
        artist_id: strap.as_array().into_iter().flatten().find_map(browse_id),
        artwork: thumb(&h["thumbnail"]["musicThumbnailRenderer"]["thumbnail"], 600),
        ..Default::default()
    };
    let small_art = al.artwork.as_deref().map(|u| resize_art_to(u, 300));
    let (artist, artist_id, title) = (al.artist.clone(), al.artist_id.clone(), al.title.clone());
    let fb = Fallback { artist: Some((&artist, artist_id.as_deref())), album: Some((&title, id)), artwork: small_art.as_deref() };
    let shelf = &tc["secondaryContents"]["sectionListRenderer"]["contents"][0]["musicShelfRenderer"]["contents"];
    al.tracks = shelf.as_array().into_iter().flatten().filter_map(|i| parse_row(&i["musicResponsiveListItemRenderer"], &fb)).collect();
    al
}

#[tauri::command]
pub async fn ytm_album(state: State<'_, AppState>, id: String) -> Result<Album, String> {
    let v = call(&state.http, "browse", json!({ "browseId": id })).await?;
    let a = parse_album(&id, &v);
    if a.title.is_empty() {
        return Err("YouTube Music has no page for this album.".into());
    }
    Ok(a)
}

// ---- radio -------------------------------------------------------------------------------

#[derive(Serialize, Default, Debug)]
pub struct Radio {
    tracks: Vec<Track>,
    /// Pass back to get more (radio never ends).
    more: Option<String>,
}

/// A queue row: "Artist, Artist • Album • Year" in one line.
fn parse_queue_item(r: &Value) -> Option<Track> {
    let id = r["videoId"].as_str()?.to_string();
    let runs = r["longBylineText"]["runs"].as_array()?;
    let groups: Vec<&[Value]> = runs.split(|x| x["text"].as_str() == Some(SEP)).collect();
    let artist: String = groups.first().map(|g| g.iter().filter_map(|x| x["text"].as_str()).collect()).unwrap_or_default();
    let artist_id = groups.first().and_then(|g| g.iter().find_map(browse_id)).filter(|b| b.starts_with("UC"));
    let album_run = runs.iter().find(|x| browse_id(x).is_some_and(|b| b.starts_with("MPREb")));
    Some(Track {
        url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        source: "youtube".into(),
        title: text(&r["title"]),
        artist,
        album: album_run.and_then(|x| x["text"].as_str()).map(String::from),
        duration: parse_duration(&text(&r["lengthText"])),
        artwork: thumb(&r["thumbnail"], 300),
        preview: false,
        artist_id,
        album_id: album_run.and_then(browse_id),
    })
}

pub fn parse_radio(v: &Value) -> Radio {
    let panel = &v["contents"]["singleColumnMusicWatchNextResultsRenderer"]["tabbedRenderer"]["watchNextTabbedResultsRenderer"]["tabs"][0]
        ["tabRenderer"]["content"]["musicQueueRenderer"]["content"]["playlistPanelRenderer"];
    let panel = if panel.is_null() { &v["continuationContents"]["playlistPanelContinuation"] } else { panel };
    Radio {
        tracks: panel["contents"].as_array().into_iter().flatten().filter_map(|i| parse_queue_item(&i["playlistPanelVideoRenderer"])).collect(),
        more: panel["continuations"][0]["nextRadioContinuationData"]["continuation"].as_str().map(String::from),
    }
}

/// Radio from a song (`video_id`) or from an artist's radio playlist (`playlist_id`).
#[tauri::command]
pub async fn ytm_radio(state: State<'_, AppState>, video_id: Option<String>, playlist_id: Option<String>, more: Option<String>) -> Result<Radio, String> {
    let playlist = playlist_id.or_else(|| video_id.as_ref().map(|v| format!("RDAMVM{v}")));
    let mut body = json!({ "playlistId": playlist, "isAudioOnly": true });
    if let Some(v) = video_id {
        body["videoId"] = json!(v);
    }
    if let Some(c) = more {
        body["continuation"] = json!(c);
    }
    Ok(parse_radio(&call(&state.http, "next", body).await?))
}

// ---- finding pages by name (for songs that came without ids) ------------------------------

async fn search_rows(http: &reqwest::Client, query: &str, filter: &str) -> Result<Vec<Value>, String> {
    let v = call(http, "search", json!({ "query": query, "params": filter })).await?;
    let sections = &v["contents"]["tabbedSearchResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]["sectionListRenderer"]["contents"];
    Ok(sections
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["musicShelfRenderer"]["contents"].as_array().cloned().unwrap_or_default())
        .map(|i| i["musicResponsiveListItemRenderer"].clone())
        .collect())
}

fn row_browse(r: &Value) -> Option<(String, String)> {
    let id = r["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()?.to_string();
    let name = text(&r["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"]);
    Some((id, name))
}

/// The artist page for a name: an exact (case-insensitive) match first, else the top result.
#[tauri::command]
pub async fn ytm_find_artist(state: State<'_, AppState>, name: String) -> Result<String, String> {
    let first = name.split([',', '&']).next().unwrap_or(&name).trim().to_string();
    let rows: Vec<(String, String)> = search_rows(&state.http, &first, FILTER_ARTISTS).await?.iter().filter_map(row_browse).collect();
    rows.iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(&first))
        .or(rows.first())
        .map(|(id, _)| id.clone())
        .ok_or_else(|| format!("Couldn't find {first} on YouTube Music."))
}

/// The album page for an artist + album name.
#[tauri::command]
pub async fn ytm_find_album(state: State<'_, AppState>, artist: String, album: String) -> Result<String, String> {
    let rows: Vec<(String, String)> = search_rows(&state.http, &format!("{artist} {album}"), FILTER_ALBUMS).await?.iter().filter_map(row_browse).collect();
    rows.iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(&album))
        .or(rows.first())
        .map(|(id, _)| id.clone())
        .ok_or_else(|| format!("Couldn't find {album} on YouTube Music."))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses saved YouTube Music responses: `KITE_YTM_SAMPLES=<dir> cargo test -- --ignored`
    /// with artist.json, album.json, radio.json and search.json in that folder.
    #[test]
    #[ignore]
    fn parses_saved_pages() {
        let dir = std::path::PathBuf::from(std::env::var("KITE_YTM_SAMPLES").expect("KITE_YTM_SAMPLES"));
        let read = |f: &str| -> Value { serde_json::from_str(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap() };
        let a = parse_artist("UCh_YNYvD3VX1wepTg1ydUaA", &read("artist.json"));
        println!("artist: {} | {} | songs {} | albums {} | singles {} | similar {} | radio {:?} | all {:?}", a.name, a.audience, a.songs.len(), a.albums.len(), a.singles.len(), a.similar.len(), a.radio, a.all_songs);
        println!("  song 1: {:?}", a.songs.first().map(|t| (&t.title, &t.artist, &t.album, t.duration, &t.album_id, t.artwork.is_some())));
        println!("  album 1: {:?}", a.albums.first());
        println!("  about: {}…", a.about.chars().take(60).collect::<String>());
        assert!(!a.name.is_empty() && !a.songs.is_empty() && !a.albums.is_empty() && !a.similar.is_empty());
        let al = parse_album("MPREb_I5szecfn2Nj", &read("album.json"));
        println!("album: {} | {} {} | {} ({:?}) | tracks {} | {:?}", al.title, al.kind, al.year, al.artist, al.artist_id, al.tracks.len(), al.tracks.first().map(|t| (&t.title, &t.artist, t.duration, t.artwork.is_some())));
        assert!(!al.tracks.is_empty());
        let r = parse_radio(&read("radio.json"));
        println!("radio: {} songs, more: {} | {:?}", r.tracks.len(), r.more.is_some(), r.tracks.get(2).map(|t| (&t.title, &t.artist, &t.album, t.duration, &t.artist_id)));
        assert!(r.tracks.len() > 40 && r.more.is_some());
        // Parsed copies, for rendering screenshots with a mocked backend.
        let out = |f: &str, v: String| std::fs::write(dir.join(f), v).unwrap();
        out("parsed-artist.json", serde_json::to_string(&a).unwrap());
        out("parsed-album.json", serde_json::to_string(&al).unwrap());
        out("parsed-radio.json", serde_json::to_string(&r).unwrap());
    }
}
