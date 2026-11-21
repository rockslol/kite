//! Scrobbling to Last.fm and ListenBrainz, both opt-in with the user's own account.
//!
//! Rules (same as other players): "now playing" when a song starts; a scrobble once the song is
//! longer than 30 s and was played for half its length or 4 minutes, whichever comes first.
//! Scrobbles that fail (offline) wait in the library and are sent with the next one.

use crate::library::now;
use crate::ytdlp::Track;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::State;

const LASTFM_API: &str = "https://ws.audioscrobbler.com/2.0/";
const LASTFM_KEY: &str = "6d828520289ef3dfe60ef2dcae1094a3";
/// Signs Last.fm requests; supplied at build time (see build.rs), never in the repository.
const LASTFM_SECRET: Option<&str> = option_env!("KITE_LASTFM_SECRET");
const LISTENBRAINZ_API: &str = "https://api.listenbrainz.org/1";
/// Unsent scrobbles older than this are dropped (Last.fm refuses ones older than 14 days).
const KEEP_PENDING: i64 = 13 * 24 * 60 * 60;

/// The connected accounts, kept apart from the other settings in `accounts.json`.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Accounts {
    lastfm_user: String,
    lastfm_session: String,
    listenbrainz_user: String,
    listenbrainz_token: String,
}

pub struct Scrobbler {
    file: PathBuf,
    accounts: Mutex<Accounts>,
}

impl Scrobbler {
    pub fn load(data_dir: &std::path::Path) -> Self {
        let file = data_dir.join("accounts.json");
        let accounts = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { file, accounts: Mutex::new(accounts) }
    }

    fn get(&self) -> Accounts {
        self.accounts.lock().unwrap().clone()
    }

    fn update(&self, f: impl FnOnce(&mut Accounts)) -> Result<(), String> {
        let mut a = self.accounts.lock().unwrap();
        f(&mut a);
        let text = serde_json::to_string_pretty(&*a).map_err(|e| e.to_string())?;
        std::fs::write(&self.file, text).map_err(|e| e.to_string())
    }
}

#[derive(Serialize)]
pub struct Status {
    lastfm_available: bool,
    lastfm_user: String,
    listenbrainz_user: String,
}

#[tauri::command]
pub fn scrobble_status(s: State<'_, Scrobbler>) -> Status {
    let a = s.get();
    Status { lastfm_available: LASTFM_SECRET.is_some(), lastfm_user: a.lastfm_user, listenbrainz_user: a.listenbrainz_user }
}

// ---- Last.fm --------------------------------------------------------------------------

type Params = BTreeMap<String, String>;

fn params<const N: usize>(pairs: [(&str, String); N]) -> Params {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Last.fm's signature: md5 of the sorted `keyvalue` pairs plus the shared secret.
fn sign(params: &Params, secret: &str) -> String {
    let mut s = String::new();
    for (k, v) in params {
        if k != "format" {
            s.push_str(k);
            s.push_str(v);
        }
    }
    s.push_str(secret);
    format!("{:x}", md5::compute(s.as_bytes()))
}

async fn lastfm(http: &reqwest::Client, mut params: Params, post: bool) -> Result<Value, String> {
    let secret = LASTFM_SECRET.ok_or("This build of Kite can't connect to Last.fm.")?;
    params.insert("api_key".into(), LASTFM_KEY.into());
    let sig = sign(&params, secret);
    params.insert("api_sig".into(), sig);
    params.insert("format".into(), "json".into());
    let req = if post { http.post(LASTFM_API).form(&params) } else { http.get(LASTFM_API).query(&params) };
    let resp = req.timeout(Duration::from_secs(15)).send().await.map_err(|e| format!("Couldn't reach Last.fm: {e}"))?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(code) = v.get("error") {
        return Err(format!("Last.fm: {} ({code})", v["message"].as_str().unwrap_or("error")));
    }
    Ok(v)
}

/// Desktop login: get a token, let the user approve Kite in the browser, then trade the token
/// for a session (checked every 2 s for up to 3 minutes).
#[tauri::command]
pub async fn lastfm_connect(state: State<'_, AppState>, s: State<'_, Scrobbler>) -> Result<String, String> {
    let http = &state.http;
    let token = lastfm(http, params([("method", "auth.getToken".into())]), false).await?["token"]
        .as_str()
        .ok_or("Last.fm sent no token")?
        .to_string();
    open_browser(&format!("https://www.last.fm/api/auth/?api_key={LASTFM_KEY}&token={token}"));
    for _ in 0..90 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        match lastfm(http, params([("method", "auth.getSession".into()), ("token", token.clone())]), false).await {
            Ok(v) => {
                let user = v["session"]["name"].as_str().unwrap_or("").to_string();
                let key = v["session"]["key"].as_str().ok_or("Last.fm sent no session")?.to_string();
                s.update(|a| {
                    a.lastfm_user = user.clone();
                    a.lastfm_session = key;
                })?;
                return Ok(user);
            }
            // 14: not approved yet. Anything else is a real problem.
            Err(e) if e.contains("(14)") => continue,
            Err(e) => return Err(e),
        }
    }
    Err("Last.fm wasn't approved in time. Try again.".into())
}

#[tauri::command]
pub fn lastfm_disconnect(s: State<'_, Scrobbler>) -> Result<(), String> {
    s.update(|a| {
        a.lastfm_user.clear();
        a.lastfm_session.clear();
    })
}

/// A track's fields, with `[i]` after each name when sending several scrobbles at once.
fn lastfm_track(t: &Track, i: Option<usize>) -> Params {
    let name = |k: &str| i.map_or(k.to_string(), |i| format!("{k}[{i}]"));
    let mut p = Params::from([(name("artist"), t.artist.clone()), (name("track"), t.title.clone())]);
    if let Some(album) = t.album.as_ref().filter(|a| !a.is_empty()) {
        p.insert(name("album"), album.clone());
    }
    if let Some(d) = t.duration {
        p.insert(name("duration"), format!("{}", d.round() as i64));
    }
    p
}

// ---- ListenBrainz ---------------------------------------------------------------------

/// Checks a user token (from listenbrainz.org/settings) and saves it.
#[tauri::command]
pub async fn listenbrainz_connect(state: State<'_, AppState>, s: State<'_, Scrobbler>, token: String) -> Result<String, String> {
    let token = token.trim().to_string();
    let v: Value = state
        .http
        .get(format!("{LISTENBRAINZ_API}/validate-token"))
        .header("Authorization", format!("Token {token}"))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach ListenBrainz: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if v["valid"].as_bool() != Some(true) {
        return Err("ListenBrainz doesn't know that token. Copy it again from listenbrainz.org/settings.".into());
    }
    let user = v["user_name"].as_str().unwrap_or("").to_string();
    s.update(|a| {
        a.listenbrainz_user = user.clone();
        a.listenbrainz_token = token;
    })?;
    Ok(user)
}

#[tauri::command]
pub fn listenbrainz_disconnect(s: State<'_, Scrobbler>) -> Result<(), String> {
    s.update(|a| {
        a.listenbrainz_user.clear();
        a.listenbrainz_token.clear();
    })
}

fn listenbrainz_listen(t: &Track, listened_at: Option<i64>) -> Value {
    let mut info = json!({
        "media_player": "Kite",
        "submission_client": "Kite",
        "submission_client_version": env!("CARGO_PKG_VERSION"),
    });
    if let Some(d) = t.duration {
        info["duration_ms"] = json!((d * 1000.0) as i64);
    }
    if t.url.starts_with("https://") {
        info["origin_url"] = json!(t.url);
    }
    let mut meta = json!({ "artist_name": t.artist, "track_name": t.title, "additional_info": info });
    if let Some(album) = t.album.as_ref().filter(|a| !a.is_empty()) {
        meta["release_name"] = json!(album);
    }
    let mut listen = json!({ "track_metadata": meta });
    if let Some(at) = listened_at {
        listen["listened_at"] = json!(at);
    }
    listen
}

async fn listenbrainz(http: &reqwest::Client, token: &str, kind: &str, listens: Vec<Value>) -> Result<(), String> {
    let resp = http
        .post(format!("{LISTENBRAINZ_API}/submit-listens"))
        .header("Authorization", format!("Token {token}"))
        .json(&json!({ "listen_type": kind, "payload": listens }))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach ListenBrainz: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ListenBrainz answered {}", resp.status()));
    }
    Ok(())
}

// ---- sending ----------------------------------------------------------------------------

/// Skip songs with no real artist (some YouTube uploads); services can't place them.
fn scrobblable(t: &Track) -> bool {
    !t.artist.trim().is_empty() && !t.title.trim().is_empty() && !t.preview
}

#[tauri::command]
pub async fn scrobble_now_playing(state: State<'_, AppState>, s: State<'_, Scrobbler>, track: Track) -> Result<(), String> {
    if !scrobblable(&track) {
        return Ok(());
    }
    let a = s.get();
    if !a.lastfm_session.is_empty() {
        let mut p = lastfm_track(&track, None);
        p.insert("method".into(), "track.updateNowPlaying".into());
        p.insert("sk".into(), a.lastfm_session.clone());
        if let Err(e) = lastfm(&state.http, p, true).await {
            crate::klog!("Last.fm now playing: {e}");
        }
    }
    if !a.listenbrainz_token.is_empty() {
        if let Err(e) = listenbrainz(&state.http, &a.listenbrainz_token, "playing_now", vec![listenbrainz_listen(&track, None)]).await {
            crate::klog!("ListenBrainz now playing: {e}");
        }
    }
    Ok(())
}

/// A finished listen: send it, plus any that failed earlier.
#[tauri::command]
pub async fn scrobble(state: State<'_, AppState>, s: State<'_, Scrobbler>, track: Track, started_at: i64) -> Result<(), String> {
    if !scrobblable(&track) {
        return Ok(());
    }
    let a = s.get();
    if a.lastfm_session.is_empty() && a.listenbrainz_token.is_empty() {
        return Ok(());
    }
    let lib = &state.library;
    let pending: Vec<(i64, Track, i64)> = lib.with(|c| {
        c.execute("DELETE FROM scrobble_queue WHERE started_at < ?1", [now() - KEEP_PENDING])?;
        let mut st = c.prepare("SELECT id, track, started_at FROM scrobble_queue ORDER BY started_at LIMIT 49")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(id, t, at)| Some((id, serde_json::from_str(&t).ok()?, at)))
            .collect())
    })?;
    let mut all: Vec<(Track, i64)> = pending.iter().map(|(_, t, at)| (t.clone(), *at)).collect();
    all.push((track.clone(), started_at));

    let mut ok = true;
    if !a.lastfm_session.is_empty() {
        // Up to 50 per request, as artist[i] / track[i] / timestamp[i].
        let mut p = params([("method", "track.scrobble".into()), ("sk", a.lastfm_session.clone())]);
        for (i, (t, at)) in all.iter().enumerate() {
            p.extend(lastfm_track(t, Some(i)));
            p.insert(format!("timestamp[{i}]"), at.to_string());
        }
        if let Err(e) = lastfm(&state.http, p, true).await {
            crate::klog!("Last.fm scrobble: {e}");
            ok = false;
        }
    }
    if !a.listenbrainz_token.is_empty() {
        let listens = all.iter().map(|(t, at)| listenbrainz_listen(t, Some(*at))).collect();
        let kind = if all.len() == 1 { "single" } else { "import" };
        if let Err(e) = listenbrainz(&state.http, &a.listenbrainz_token, kind, listens).await {
            crate::klog!("ListenBrainz scrobble: {e}");
            ok = false;
        }
    }
    lib.with(|c| {
        if ok {
            for (id, _, _) in &pending {
                c.execute("DELETE FROM scrobble_queue WHERE id = ?1", [id])?;
            }
        } else {
            let json = serde_json::to_string(&track).unwrap_or_default();
            c.execute("INSERT INTO scrobble_queue (track, started_at) VALUES (?1, ?2)", rusqlite::params![json, started_at])?;
        }
        Ok(())
    })?;
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_like_last_fm_documents() {
        // api_sig = md5("api_key" + key + "method" + method + "token" + token + secret); "format" is left out.
        let p = params([
            ("method", "auth.getSession".into()),
            ("api_key", "abc".into()),
            ("token", "t0k".into()),
            ("format", "json".into()),
        ]);
        let expected = format!("{:x}", md5::compute(b"api_keyabcmethodauth.getSessiontokent0ksecret"));
        assert_eq!(sign(&p, "secret"), expected);
    }

    #[test]
    fn listens_carry_what_listenbrainz_needs() {
        let t = Track {
            id: "x".into(),
            source: "youtube".into(),
            url: "https://www.youtube.com/watch?v=x".into(),
            title: "sol".into(),
            artist: "Che".into(),
            album: None,
            duration: Some(135.0),
            artwork: None,
            preview: false,
            ..Default::default()
        };
        let l = listenbrainz_listen(&t, Some(1_800_000_000));
        assert_eq!(l["listened_at"], 1_800_000_000);
        assert_eq!(l["track_metadata"]["artist_name"], "Che");
        assert_eq!(l["track_metadata"]["additional_info"]["duration_ms"], 135_000);
        assert!(l["track_metadata"].get("release_name").is_none());
    }
}
