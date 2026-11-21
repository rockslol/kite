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
        .or_else(|| r["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]["musicPlayButtonRenderer"]["playNavigationEndpoint"]["watchEndpoint"]["videoId"].as_str())
        .or_else(|| r["navigationEndpoint"]["watchEndpoint"]["videoId"].as_str())?
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
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork: Option<String>,
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
    pub id: String,
    pub name: String,
    pub artwork: Option<String>,
    /// "810K monthly audience" or "254K subscribers".
    pub audience: String,
    pub about: String,
    pub songs: Vec<Track>,
    /// The full song list, as a playlist to open.
    pub all_songs: Option<String>,
    pub albums: Vec<Card>,
    pub singles: Vec<Card>,
    pub similar: Vec<Card>,
    /// Playlist id for the artist's radio.
    pub radio: Option<String>,
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

pub async fn fetch_artist(http: &reqwest::Client, id: &str) -> Result<Artist, String> {
    let v = call(http, "browse", json!({ "browseId": id })).await?;
    let a = parse_artist(id, &v);
    if a.name.is_empty() {
        return Err("YouTube Music has no page for this artist.".into());
    }
    Ok(a)
}

#[tauri::command]
pub async fn ytm_artist(state: State<'_, AppState>, id: String) -> Result<Artist, String> {
    fetch_artist(&state.http, &id).await
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

/// An album page, or a playlist page (`VL…` ids, such as charts), which has the same layout.
pub fn parse_album(id: &str, v: &Value) -> Album {
    let playlist = id.starts_with("VL");
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
    // Playlist rows name their own artists and covers; album rows leave them out.
    let fb = if playlist {
        Fallback::default()
    } else {
        Fallback { artist: Some((&artist, artist_id.as_deref())), album: Some((&title, id)), artwork: small_art.as_deref() }
    };
    let shelf = &tc["secondaryContents"]["sectionListRenderer"]["contents"][0];
    let rows = shelf["musicShelfRenderer"]["contents"].as_array().or_else(|| shelf["musicPlaylistShelfRenderer"]["contents"].as_array());
    al.tracks = rows.into_iter().flatten().filter_map(|i| parse_row(&i["musicResponsiveListItemRenderer"], &fb)).collect();
    if playlist {
        // "Chart • 2026": the kind is "Chart"; a playlist's year isn't worth showing.
        if al.kind.is_empty() {
            al.kind = "Playlist".into();
        }
        al.year = String::new();
    }
    al
}

#[tauri::command]
pub async fn ytm_album(state: State<'_, AppState>, id: String) -> Result<Album, String> {
    let v = call(&state.http, "browse", json!({ "browseId": id })).await?;
    let a = parse_album(&id, &v);
    if a.title.is_empty() {
        return Err(format!("YouTube Music has no page for this {}.", if id.starts_with("VL") { "playlist" } else { "album" }));
    }
    Ok(a)
}

// ---- charts ------------------------------------------------------------------------------

#[derive(Serialize, Debug)]
pub struct Country {
    code: String,
    name: String,
}

#[derive(Serialize, Default, Debug)]
pub struct Charts {
    /// The country shown (YouTube Music picks one from the IP when none is asked for).
    country: String,
    countries: Vec<Country>,
    /// Chart playlists: "Top 100 Music Videos", "Daily Top Music Videos", "Trending 20".
    playlists: Vec<Card>,
    artists: Vec<Card>,
}

/// Base64 (standard or URL-safe, padding ignored), for the country codes in the menu's keys.
fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (vec![], 0u32, 0);
    for v in s.bytes().filter_map(val) {
        acc = ((acc << 6) | v as u32) & 0xffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// "EidleHBsb3Jl…%3D%3D" → "HU": the key holds `explore_charts_country_menu_<digits><CODE>`.
fn country_code(key: &str) -> Option<String> {
    let raw = base64(&key.replace("%3D", ""));
    let raw = String::from_utf8_lossy(&raw);
    let rest = &raw[raw.find("country_menu_")? + "country_menu_".len()..];
    let code: String = rest.trim_start_matches(|c: char| c.is_ascii_digit()).chars().take(2).collect();
    (code.len() == 2 && code.chars().all(|c| c.is_ascii_uppercase())).then_some(code)
}

pub fn parse_charts(v: &Value) -> Charts {
    let mut ch = Charts::default();
    let sections = &v["contents"]["singleColumnBrowseResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]["sectionListRenderer"]["contents"];
    for s in sections.as_array().into_iter().flatten() {
        if let Some(shelf) = s.get("musicShelfRenderer") {
            let menu = &shelf["subheaders"][0]["musicSideAlignedItemRenderer"]["startItems"][0]["musicSortFilterButtonRenderer"]["menu"]
                ["musicMultiSelectMenuRenderer"];
            for o in menu["options"].as_array().into_iter().flatten() {
                let o = &o["musicMultiSelectMenuItemRenderer"];
                let Some(code) = o["formItemEntityKey"].as_str().and_then(country_code) else { continue };
                // The country shown is the only one without a command to pick it.
                if o.get("selectedCommand").is_none() && ch.country.is_empty() {
                    ch.country = code.clone();
                }
                ch.countries.push(Country { code, name: text(&o["title"]) });
            }
        } else if let Some(c) = s.get("musicCarouselShelfRenderer") {
            let title = text(&c["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]).to_lowercase();
            let items = c["contents"].as_array().into_iter().flatten();
            if title.contains("artist") {
                ch.artists = items
                    .filter_map(|i| {
                        let r = &i["musicResponsiveListItemRenderer"];
                        let col = |n: usize| text(&r["flexColumns"][n]["musicResponsiveListItemFlexColumnRenderer"]["text"]);
                        Some(Card { id: browse_id(r)?, title: col(0), subtitle: col(1), artwork: row_thumb(r, 300) })
                    })
                    .collect();
            } else {
                ch.playlists.extend(items.filter_map(card).filter(|c| c.id.starts_with("VL")));
            }
        }
    }
    ch
}

/// Charts for a country code ("HU", or "ZZ" for global), or YouTube Music's guess for `None`.
#[tauri::command]
pub async fn ytm_charts(state: State<'_, AppState>, country: Option<String>) -> Result<Charts, String> {
    let mut body = json!({ "browseId": "FEmusic_charts" });
    if let Some(c) = country.filter(|c| !c.is_empty()) {
        body["formData"] = json!({ "selectedValues": [c] });
    }
    let ch = parse_charts(&call(&state.http, "browse", body).await?);
    if ch.playlists.is_empty() && ch.artists.is_empty() {
        return Err("YouTube Music didn't send any charts.".into());
    }
    Ok(ch)
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

// ---- search suggestions ------------------------------------------------------------------

#[derive(Serialize, Default, Debug)]
pub struct Suggestions {
    /// Finished queries for what's typed so far.
    queries: Vec<String>,
    /// Artists and albums that match.
    cards: Vec<Card>,
    songs: Vec<Track>,
}

pub fn parse_suggestions(v: &Value) -> Suggestions {
    let mut out = Suggestions::default();
    for sec in v["contents"].as_array().into_iter().flatten() {
        for item in sec["searchSuggestionsSectionRenderer"]["contents"].as_array().into_iter().flatten() {
            if let Some(s) = item.get("searchSuggestionRenderer") {
                out.queries.push(text(&s["suggestion"]));
                continue;
            }
            let r = &item["musicResponsiveListItemRenderer"];
            if r.is_null() {
                continue;
            }
            match browse_id(r) {
                Some(id) if id.starts_with("UC") || id.starts_with("MPREb") => {
                    let col = |n: usize| text(&r["flexColumns"][n]["musicResponsiveListItemFlexColumnRenderer"]["text"]);
                    out.cards.push(Card { id, title: col(0), subtitle: col(1), artwork: row_thumb(r, 120) });
                }
                Some(_) => {}
                None => out.songs.extend(parse_row(r, &Fallback::default())),
            }
        }
    }
    out
}

#[tauri::command]
pub async fn ytm_suggestions(state: State<'_, AppState>, query: String) -> Result<Suggestions, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Suggestions::default());
    }
    Ok(parse_suggestions(&call(&state.http, "music/get_search_suggestions", json!({ "input": q })).await?))
}

// ---- related songs ("Because you liked") --------------------------------------------------

#[derive(Serialize, Default, Debug)]
pub struct Related {
    songs: Vec<Track>,
    artists: Vec<Card>,
}

/// The "Related" tab of a song's watch page: a `MPTRt_…` page to browse.
fn related_tab(v: &Value) -> Option<String> {
    let tabs = &v["contents"]["singleColumnMusicWatchNextResultsRenderer"]["tabbedRenderer"]["watchNextTabbedResultsRenderer"]["tabs"];
    tabs.as_array()?.iter().find_map(|t| {
        t["tabRenderer"]["endpoint"]["browseEndpoint"]["browseId"].as_str().filter(|b| b.starts_with("MPTRt")).map(String::from)
    })
}

pub fn parse_related(v: &Value) -> Related {
    let mut out = Related::default();
    for s in v["contents"]["sectionListRenderer"]["contents"].as_array().into_iter().flatten() {
        let c = &s["musicCarouselShelfRenderer"];
        let items = c["contents"].as_array().into_iter().flatten();
        let title = text(&c["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]).to_lowercase();
        if title.contains("might also like") {
            out.songs = items.filter_map(|i| parse_row(&i["musicResponsiveListItemRenderer"], &Fallback::default())).collect();
        } else if title.contains("similar artists") {
            out.artists = items.filter_map(card).filter(|c| c.id.starts_with("UC")).collect();
        }
    }
    out
}

/// Songs like this one (YouTube Music's "You might also like"), and similar artists.
#[tauri::command]
pub async fn ytm_related(state: State<'_, AppState>, video_id: String) -> Result<Related, String> {
    let next = call(&state.http, "next", json!({ "videoId": video_id, "isAudioOnly": true })).await?;
    let tab = related_tab(&next).ok_or("YouTube Music has no related songs for this one.")?;
    Ok(parse_related(&call(&state.http, "browse", json!({ "browseId": tab })).await?))
}

// ---- song info ----------------------------------------------------------------------------

#[derive(Serialize, Default, Debug)]
pub struct SongInfo {
    views: Option<u64>,
    /// "2025-05-15".
    published: Option<String>,
    /// "Song" (from an album), "Music video", "Video" (anyone's upload) or "".
    kind: String,
    channel: Option<String>,
    channel_id: Option<String>,
}

pub fn parse_player(v: &Value) -> SongInfo {
    let d = &v["videoDetails"];
    let mf = &v["microformat"]["microformatDataRenderer"];
    SongInfo {
        views: d["viewCount"].as_str().and_then(|n| n.parse().ok()),
        published: mf["uploadDate"].as_str().or(mf["publishDate"].as_str()).map(|s| s.chars().take(10).collect()),
        kind: match d["musicVideoType"].as_str().unwrap_or("") {
            "MUSIC_VIDEO_TYPE_ATV" => "Song",
            "MUSIC_VIDEO_TYPE_OMV" | "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC" => "Music video",
            "MUSIC_VIDEO_TYPE_UGC" => "Video",
            _ => "",
        }
        .into(),
        channel: d["author"].as_str().map(String::from),
        channel_id: d["channelId"].as_str().map(String::from),
    }
}

/// Views, upload date and kind of a YouTube video. Only the details: Kite never streams
/// from this answer (yt-dlp does that).
#[tauri::command]
pub async fn ytm_song_info(state: State<'_, AppState>, video_id: String) -> Result<SongInfo, String> {
    let v = call(&state.http, "player", json!({ "videoId": video_id })).await?;
    let info = parse_player(&v);
    if info.views.is_none() && info.channel.is_none() {
        return Err("YouTube didn't share details for this one.".into());
    }
    Ok(info)
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

/// The artist id for a name, only when YouTube Music has an artist with exactly that name.
pub async fn find_artist_exact(http: &reqwest::Client, name: &str) -> Result<Option<String>, String> {
    let rows: Vec<(String, String)> = search_rows(http, name, FILTER_ARTISTS).await?.iter().filter_map(row_browse).collect();
    Ok(rows.into_iter().find(|(id, n)| id.starts_with("UC") && n.eq_ignore_ascii_case(name)).map(|(id, _)| id))
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

    #[test]
    fn reads_country_codes_from_menu_keys() {
        assert_eq!(country_code("EidleHBsb3JlX2NoYXJ0c19jb3VudHJ5X21lbnVfMzE2NzY2NTY3SFUgkQEoAQ%3D%3D").as_deref(), Some("HU"));
        assert_eq!(country_code("EidleHBsb3JlX2NoYXJ0c19jb3VudHJ5X21lbnVfMzE2NzY2NTY3WlogkQEoAQ%3D%3D").as_deref(), Some("ZZ"));
        assert_eq!(country_code("bm90aGluZw"), None);
    }

    /// Parses saved suggestion, related and player answers: `KITE_YTM_CHARTS=<dir> cargo test -- --ignored`
    /// with sugg.json, next.json, related.json and player.json in that folder.
    #[test]
    #[ignore]
    fn parses_saved_song_pages() {
        let dir = std::path::PathBuf::from(std::env::var("KITE_YTM_CHARTS").expect("KITE_YTM_CHARTS"));
        let read = |f: &str| -> Value { serde_json::from_str(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap() };
        let s = parse_suggestions(&read("sugg.json"));
        println!("suggestions: {:?} | cards {:?} | songs {:?}", s.queries, s.cards.iter().map(|c| (&c.title, &c.subtitle)).collect::<Vec<_>>(), s.songs.iter().map(|t| (&t.title, &t.artist, &t.artist_id, &t.album)).collect::<Vec<_>>());
        assert!(!s.queries.is_empty());
        println!("related tab: {:?}", related_tab(&read("next.json")));
        assert!(related_tab(&read("next.json")).is_some());
        let r = parse_related(&read("related.json"));
        println!("related: {} songs, {} artists | {:?}", r.songs.len(), r.artists.len(), r.songs.iter().take(3).map(|t| (&t.title, &t.artist, &t.album, t.duration, t.artwork.is_some())).collect::<Vec<_>>());
        assert!(r.songs.len() >= 10 && !r.artists.is_empty());
        let p = parse_player(&read("player.json"));
        println!("player: {p:?}");
        assert!(p.views.is_some() && p.published.is_some());
        std::fs::write(dir.join("parsed-related.json"), serde_json::to_string(&r).unwrap()).unwrap();
        std::fs::write(dir.join("parsed-sugg.json"), serde_json::to_string(&s).unwrap()).unwrap();
        std::fs::write(dir.join("parsed-player.json"), serde_json::to_string(&p).unwrap()).unwrap();
    }

    /// Parses saved chart pages: `KITE_YTM_CHARTS=<dir> cargo test -- --ignored` with charts.json
    /// and top100.json (a chart playlist) in that folder.
    #[test]
    #[ignore]
    fn parses_saved_charts() {
        let dir = std::path::PathBuf::from(std::env::var("KITE_YTM_CHARTS").expect("KITE_YTM_CHARTS"));
        let read = |f: &str| -> Value { serde_json::from_str(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap() };
        let ch = parse_charts(&read("charts.json"));
        println!(
            "charts: {} | {} countries | playlists {:?} | artists {} {:?}",
            ch.country,
            ch.countries.len(),
            ch.playlists.iter().map(|c| &c.title).collect::<Vec<_>>(),
            ch.artists.len(),
            ch.artists.first()
        );
        assert!(ch.country.len() == 2 && ch.countries.len() > 50 && !ch.playlists.is_empty() && !ch.artists.is_empty());
        let pl = parse_album("VLPL4fGSI1pDJn6-AkuEzkhgTBJq3Lm0Oolc", &read("top100.json"));
        println!(
            "playlist: {} | {} | {} tracks | {:?}",
            pl.title,
            pl.kind,
            pl.tracks.len(),
            pl.tracks.first().map(|t| (&t.title, &t.artist, t.duration, &t.artist_id, t.artwork.is_some()))
        );
        assert_eq!(pl.tracks.len(), 100);
        let out = |f: &str, v: String| std::fs::write(dir.join(f), v).unwrap();
        out("parsed-charts.json", serde_json::to_string(&ch).unwrap());
        out("parsed-top100.json", serde_json::to_string(&pl).unwrap());
    }

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
