//! Song search on YouTube Music, through the same endpoint its website uses. Search only:
//! playback still goes through yt-dlp. Also the matcher that picks the right song for an
//! imported Spotify/Deezer track.

use crate::ytdlp::Track;
use serde_json::{json, Value};

const ENDPOINT: &str = "https://music.youtube.com/youtubei/v1/search?prettyPrint=false";
const CLIENT_VERSION: &str = "1.20251110.01.00";
/// "Songs" filter.
const SONGS: &str = "EgWKAQIIAWoQEAMQBBAJEAoQBRAREBAQFQ%3D%3D";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";

pub async fn search_songs(http: &reqwest::Client, query: &str) -> Result<Vec<Track>, String> {
    let body = json!({
        "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": CLIENT_VERSION, "hl": "en", "gl": "US" } },
        "query": query,
        "params": percent_encoding::percent_decode_str(SONGS).decode_utf8_lossy(),
    });
    let v: Value = http
        .post(ENDPOINT)
        .header("Origin", "https://music.youtube.com")
        .header("User-Agent", UA)
        .json(&body)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("YouTube Music search failed: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let sections = &v["contents"]["tabbedSearchResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]
        ["sectionListRenderer"]["contents"];
    let mut out = Vec::new();
    for section in sections.as_array().into_iter().flatten() {
        for item in section["musicShelfRenderer"]["contents"].as_array().into_iter().flatten() {
            if let Some(t) = parse_item(&item["musicResponsiveListItemRenderer"]) {
                out.push(t);
            }
        }
    }
    Ok(out)
}

fn parse_item(r: &Value) -> Option<Track> {
    let id = r["playlistItemData"]["videoId"].as_str()?.to_string();
    let col = |i: usize| r["flexColumns"][i]["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array();
    let title: String = col(0)?.iter().filter_map(|x| x["text"].as_str()).collect();

    let mut artists = Vec::new();
    let mut album = None;
    let mut duration = None;
    for run in col(1).into_iter().flatten() {
        let text = run["text"].as_str().unwrap_or("");
        let page = run["navigationEndpoint"]["browseEndpoint"]["browseEndpointContextSupportedConfigs"]
            ["browseEndpointContextMusicConfig"]["pageType"]
            .as_str();
        match page {
            Some("MUSIC_PAGE_TYPE_ARTIST") => artists.push(text.to_string()),
            Some("MUSIC_PAGE_TYPE_ALBUM") => album = Some(text.to_string()),
            _ => {
                if let Some(d) = parse_duration(text.trim()) {
                    duration = Some(d);
                }
            }
        }
    }
    // Some songs list their artist as plain text (no link): take the first run then.
    if artists.is_empty() {
        if let Some(first) = col(1).and_then(|c| c.first()).and_then(|x| x["text"].as_str()) {
            artists.push(first.to_string());
        }
    }

    let artwork = r["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]
        .as_array()
        .and_then(|t| t.last())
        .and_then(|t| t["url"].as_str())
        .map(resize_art);

    Some(Track {
        url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        source: "youtube".into(),
        title,
        artist: artists.join(", "),
        album,
        duration,
        artwork,
        preview: false,
    })
}

/// Google image URLs end in size parameters like `=w120-h120-l90-rj`; ask for 300 px.
fn resize_art(url: &str) -> String {
    match url.rfind("=w") {
        Some(i) => {
            let rest = &url[i + 2..];
            let after_h = rest.find("-h").map(|h| {
                let tail = &rest[h + 2..];
                let digits = tail.chars().take_while(|c| c.is_ascii_digit()).count();
                &tail[digits..]
            });
            match after_h {
                Some(tail) => format!("{}=w300-h300{tail}", &url[..i]),
                None => url.to_string(),
            }
        }
        None => url.to_string(),
    }
}

fn parse_duration(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(parts.iter().fold(0.0, |acc, p| acc * 60.0 + p.parse::<f64>().unwrap_or(0.0)))
}

// ---- matching ---------------------------------------------------------------

/// A song from a Spotify/Deezer list that needs a playable match.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Wanted {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
}

impl Wanted {
    pub fn query(&self) -> String {
        let first_artist = split_artists(&self.artist).into_iter().next().unwrap_or_default();
        format!("{first_artist} {}", strip_feat(&self.title))
    }

    /// Key for remembering matches across imports.
    pub fn key(&self) -> String {
        format!("{}|{}", normalize(&self.artist), normalize(&strip_feat(&self.title)))
    }
}

const VERSION_WORDS: &[&str] = &[
    "live", "cover", "karaoke", "instrumental", "remix", "sped", "slowed", "nightcore", "8d", "acoustic",
    "reverb", "tribute", "lofi", "lo-fi", "mashup", "bass boosted",
];

pub fn normalize(s: &str) -> String {
    let lower = s.to_lowercase();
    let cleaned: String = lower.chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' }).collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_feat(title: &str) -> String {
    let lower = title.to_lowercase();
    let mut cut = title.len();
    for marker in ["(feat", "[feat", "(ft.", "[ft.", " feat.", " ft. ", "(with ", " - feat"] {
        if let Some(i) = lower.find(marker) {
            cut = cut.min(i);
        }
    }
    title[..cut].trim().to_string()
}

fn split_artists(s: &str) -> Vec<String> {
    s.split([',', '&'])
        .flat_map(|p| p.split(" feat. ").flat_map(|q| q.split(" x ")))
        .map(|p| normalize(p))
        .filter(|p| !p.is_empty())
        .collect()
}

fn words(s: &str) -> std::collections::HashSet<String> {
    normalize(s).split(' ').filter(|w| !w.is_empty()).map(String::from).collect()
}

/// 0..1, higher is a better match.
pub fn score(want: &Wanted, cand: &Track) -> f64 {
    let wt = words(&strip_feat(&want.title));
    let ct = words(&strip_feat(&cand.title));
    let title = if wt.is_empty() || ct.is_empty() {
        0.0
    } else {
        let common = wt.intersection(&ct).count() as f64;
        // Weighted toward the wanted title being fully present.
        0.7 * common / wt.len() as f64 + 0.3 * common / ct.len() as f64
    };

    let cand_artists = normalize(&cand.artist);
    let want_artists = split_artists(&want.artist);
    let artist = if want_artists.iter().any(|a| cand_artists.contains(a.as_str())) {
        1.0
    } else if want_artists.iter().any(|a| normalize(&cand.title).contains(a.as_str())) {
        0.5
    } else {
        0.0
    };

    let duration = match (want.duration, cand.duration) {
        (Some(a), Some(b)) => {
            let d = (a - b).abs();
            if d <= 3.0 {
                1.0
            } else if d <= 10.0 {
                0.6
            } else if d <= 30.0 {
                0.2
            } else {
                -0.5
            }
        }
        _ => 0.3,
    };

    // A live or sped-up version is wrong unless the original asked for it.
    let want_lower = want.title.to_lowercase();
    let cand_lower = cand.title.to_lowercase();
    let wrong_version = VERSION_WORDS.iter().any(|w| cand_lower.contains(w) && !want_lower.contains(w));

    let mut s = 0.45 * title + 0.35 * artist + 0.2 * duration;
    if wrong_version {
        s -= 0.35;
    }
    s
}

pub const GOOD_ENOUGH: f64 = 0.55;

/// Candidates for `want`, best first, with their scores.
pub async fn candidates(http: &reqwest::Client, want: &Wanted) -> Result<Vec<(f64, Track)>, String> {
    let mut found: Vec<(f64, Track)> = search_songs(http, &want.query())
        .await?
        .into_iter()
        .map(|t| (score(want, &t), t))
        .collect();
    found.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(title: &str, artist: &str, duration: f64) -> Track {
        Track {
            id: "x".into(),
            source: "youtube".into(),
            url: "u".into(),
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration: Some(duration),
            artwork: None,
            preview: false,
        }
    }
    fn want(title: &str, artist: &str, duration: f64) -> Wanted {
        Wanted { title: title.into(), artist: artist.into(), album: None, duration: Some(duration) }
    }

    #[test]
    fn prefers_the_original() {
        let w = want("Get Lucky (feat. Pharrell Williams & Nile Rodgers)", "Daft Punk, Pharrell Williams, Nile Rodgers", 369.0);
        let orig = cand("Get Lucky (feat. Pharrell Williams and Nile Rodgers)", "Daft Punk, Pharrell Williams, Nile Rodgers", 370.0);
        let live = cand("Get Lucky (Live)", "Daft Punk", 400.0);
        let cover = cand("Get Lucky", "Funk Punk", 250.0);
        assert!(score(&w, &orig) > GOOD_ENOUGH);
        assert!(score(&w, &orig) > score(&w, &live));
        assert!(score(&w, &orig) > score(&w, &cover));
        assert!(score(&w, &cover) < GOOD_ENOUGH);
    }

    #[test]
    fn sped_up_only_when_asked() {
        let w = want("Snooze", "SZA", 201.0);
        let sped = cand("Snooze (Sped Up)", "SZA", 160.0);
        assert!(score(&w, &sped) < GOOD_ENOUGH);
        let w2 = want("Snooze (Sped Up)", "SZA", 160.0);
        assert!(score(&w2, &sped) > GOOD_ENOUGH);
    }

    #[test]
    fn parses_durations_and_art() {
        assert_eq!(parse_duration("6:10"), Some(370.0));
        assert_eq!(parse_duration("1:02:03"), Some(3723.0));
        assert_eq!(parse_duration("1.9B plays"), None);
        assert_eq!(resize_art("https://x/abc=w120-h120-l90-rj"), "https://x/abc=w300-h300-l90-rj");
    }
}
