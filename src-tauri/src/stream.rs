//! Serves audio to the webview through the `kite://` protocol.
//!
//! Progressive streams are proxied chunk by chunk with HTTP range requests, so playback starts
//! right away and seeking works. Streams that can't be proxied that way (HLS) are downloaded to
//! the cache first and served from disk.

use crate::ytdlp::YtDlp;
use percent_encoding::percent_decode_str;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri::http::{header, Request, Response, StatusCode};
use tokio::sync::OnceCell;

/// Largest piece fetched from the upstream server per request. The audio element asks for the
/// next piece as it plays.
const CHUNK: u64 = 2 * 1024 * 1024;
/// How long a stream link is trusted when it doesn't say when it expires.
const TTL: u64 = 60 * 60;
/// Links that do say (YouTube's `expire=`, about 6 hours out) are resolved again this long before.
const EXPIRY_MARGIN: u64 = 30 * 60;
/// A fresh YouTube link is refused for a moment (measured: working 1.8-2.9 s after yt-dlp
/// returns). One still refused this long after is one that never works: get another.
const LINK_DEADLINE: Duration = Duration::from_millis(4500);

enum Source {
    Remote {
        url: String,
        headers: Vec<(String, String)>,
        mime: &'static str,
        available_at: Option<u64>,
        /// Fresh links only: when to stop waiting for the link to start working.
        give_up: Option<Instant>,
    },
    File { path: PathBuf, mime: &'static str },
}

struct Entry {
    source: Source,
    /// Unix seconds after which a remote link is resolved again.
    fresh_until: u64,
}

/// A resolved link kept on disk, so songs played or prepared earlier start right away after
/// a restart too (until the link expires).
#[derive(Serialize, Deserialize, Clone)]
struct SavedLink {
    /// Found with Data saver on or off. Links saved before 0.4.1 don't say, and aren't used.
    #[serde(default)]
    saver: Option<bool>,
    url: String,
    headers: Vec<(String, String)>,
    ext: String,
    available_at: Option<u64>,
    fresh_until: u64,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// When a link stops being trusted: from its own `expire=` (YouTube), else an hour from now.
/// The second value says whether the link stated its expiry (only those are saved to disk).
fn fresh_until(url: &str) -> (u64, bool) {
    let expire = url
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("expire="))
        .and_then(|v| v.parse::<u64>().ok());
    match expire {
        Some(e) => (e.saturating_sub(EXPIRY_MARGIN), true),
        None => (unix_now() + TTL, false),
    }
}

type Slot = Arc<OnceCell<Result<Arc<Entry>, String>>>;

/// Each song can have a link per quality: `slot_key(page, data_saver)`.
fn slot_key(page_url: &str, saver: bool) -> String {
    if saver {
        format!("saver {page_url}")
    } else {
        page_url.to_string()
    }
}

pub struct Streams {
    yt: Arc<YtDlp>,
    http: reqwest::Client,
    cache_dir: PathBuf,
    slots: Mutex<HashMap<String, Slot>>,
    /// Caps parallel yt-dlp runs, since the UI resolves songs ahead on hover.
    resolving: tokio::sync::Semaphore,
    links_file: PathBuf,
    saved: Mutex<HashMap<String, SavedLink>>,
    /// Settings → Streaming → Data saver.
    saver: std::sync::atomic::AtomicBool,
    /// The quality each song started streaming at. A song keeps it to the end, even if Data
    /// saver is switched meanwhile: the rest of it must come from the same file.
    serving: Mutex<HashMap<String, bool>>,
}

impl Streams {
    pub fn new(yt: Arc<YtDlp>, http: reqwest::Client, cache_dir: PathBuf) -> Self {
        let links_file = cache_dir.join("links.json");
        // Links that still have a while to live, from earlier runs.
        let soon = unix_now() + 60;
        let saved: HashMap<String, SavedLink> = std::fs::read(&links_file)
            .ok()
            .and_then(|b| serde_json::from_slice::<HashMap<String, SavedLink>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, l)| l.fresh_until > soon && l.saver.is_some())
            .collect();
        let slots = saved
            .iter()
            .map(|(page, l)| {
                let entry = Entry {
                    source: Source::Remote {
                        url: l.url.clone(),
                        headers: l.headers.clone(),
                        mime: mime_for(&l.ext),
                        available_at: l.available_at,
                        give_up: None,
                    },
                    fresh_until: l.fresh_until,
                };
                (page.clone(), Arc::new(OnceCell::new_with(Some(Ok(Arc::new(entry))))))
            })
            .collect();
        Self {
            yt,
            http,
            cache_dir: cache_dir.join("audio"),
            slots: Mutex::new(slots),
            resolving: tokio::sync::Semaphore::new(4),
            links_file,
            saved: Mutex::new(saved),
            saver: Default::default(),
            serving: Mutex::new(HashMap::new()),
        }
    }

    /// Writes the saved links (dropping expired ones). Small file, written whole.
    fn save_links(&self, change: impl FnOnce(&mut HashMap<String, SavedLink>)) {
        let mut saved = self.saved.lock().unwrap();
        change(&mut saved);
        let now = unix_now();
        saved.retain(|_, l| l.fresh_until > now);
        let Ok(json) = serde_json::to_vec(&*saved) else { return };
        let tmp = self.links_file.with_extension("json.part");
        if std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &self.links_file)).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Resolves a track page URL to something playable, at the current quality. Concurrent
    /// callers share one lookup.
    pub async fn prepare(&self, page_url: &str) -> Result<(), String> {
        self.entry(page_url, self.saver.load(std::sync::atomic::Ordering::Relaxed)).await.map(|_| ())
    }

    async fn entry(&self, page_url: &str, saver: bool) -> Result<Arc<Entry>, String> {
        let key = slot_key(page_url, saver);
        let slot = {
            let mut slots = self.slots.lock().unwrap();
            // A downloaded (HLS) song plays from its file, whichever quality it was saved at.
            if let Some(Ok(e)) = slots.get(&slot_key(page_url, !saver)).and_then(|s| s.get()) {
                if matches!(&e.source, Source::File { path, .. } if path.exists()) {
                    return Ok(e.clone());
                }
            }
            let fresh = |s: &Slot| match s.get() {
                // Cached files stay valid until the file is gone (Settings → Clear cache).
                Some(Ok(e)) => match &e.source {
                    Source::File { path, .. } => path.exists(),
                    Source::Remote { .. } => unix_now() < e.fresh_until,
                },
                Some(Err(_)) => false,
                None => true,
            };
            match slots.get(&key) {
                Some(s) if fresh(s) => s.clone(),
                _ => {
                    let s: Slot = Arc::new(OnceCell::new());
                    slots.insert(key, s.clone());
                    s
                }
            }
        };
        slot.get_or_init(|| self.load(page_url, saver)).await.clone()
    }

    /// The quality to serve a request at. A media element starts every stream with a request
    /// from byte 0: that one takes the current setting, and the rest of the song follows it.
    fn quality_for(&self, page_url: &str, start: u64) -> bool {
        let now = self.saver.load(std::sync::atomic::Ordering::Relaxed);
        let mut serving = self.serving.lock().unwrap();
        if start == 0 {
            serving.insert(page_url.to_string(), now);
            now
        } else {
            serving.get(page_url).copied().unwrap_or(now)
        }
    }

    /// Debug builds only, for testing: make every cached stream link invalid, as if it had
    /// expired upstream.
    #[cfg(debug_assertions)]
    pub fn break_links(&self) -> usize {
        let mut n = 0;
        for slot in self.slots.lock().unwrap().values_mut() {
            let Some(Ok(e)) = slot.get() else { continue };
            let Source::Remote { url, headers, mime, available_at, give_up } = &e.source else { continue };
            // `expire` is covered by the signature, so changing it makes the server refuse the link.
            let broken = Entry {
                source: Source::Remote {
                    url: url.replacen("expire=", "expire=1", 1),
                    headers: headers.clone(),
                    mime,
                    available_at: *available_at,
                    give_up: *give_up,
                },
                fresh_until: e.fresh_until,
            };
            *slot = Arc::new(OnceCell::new_with(Some(Ok(Arc::new(broken)))));
            n += 1;
        }
        n
    }

    /// Switches between best quality and data saver. Songs started from now on use the new
    /// one; a song already playing keeps its own to the end. Links found at either quality are
    /// kept, so switching back is instant.
    pub fn set_saver(&self, on: bool) {
        self.saver.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// How many resolved links are saved for later runs.
    pub fn saved_count(&self) -> usize {
        self.saved.lock().unwrap().len()
    }

    fn forget(&self, page_url: &str, saver: bool) {
        let key = slot_key(page_url, saver);
        self.slots.lock().unwrap().remove(&key);
        if self.saved.lock().unwrap().contains_key(&key) {
            self.save_links(|s| {
                s.remove(&key);
            });
        }
    }

    async fn load(&self, page_url: &str, saver: bool) -> Result<Arc<Entry>, String> {
        let mut tries = 0;
        loop {
            tries += 1;
            let (entry, link) = self.resolve_once(page_url, saver).await?;
            let Source::Remote { url, headers, mime, available_at, give_up } = &entry.source else { return Ok(entry) };
            // Check the link with a 1-byte read, so a song that was prepared ahead really starts
            // right away, and a link that never works is replaced before anyone presses play.
            if self.proxy(url, headers, mime, *available_at, *give_up, Some((0, Some(0)))).await.is_ok() {
                if let Some(link) = link {
                    self.save_links(|s| {
                        s.insert(slot_key(page_url, saver), link);
                    });
                }
                return Ok(entry);
            }
            crate::klog!("stream link for {page_url} never started working (try {tries})");
            if tries == 2 {
                // Hand it over anyway: playing it retries once more.
                return Ok(entry);
            }
        }
    }

    /// One yt-dlp lookup. Returns the entry, and the link to save for later runs if it says
    /// when it expires.
    async fn resolve_once(&self, page_url: &str, saver: bool) -> Result<(Arc<Entry>, Option<SavedLink>), String> {
        let started = Instant::now();
        let r = {
            let _permit = self.resolving.acquire().await.map_err(|e| e.to_string())?;
            self.yt.resolve(page_url, saver).await.map_err(|e| {
                crate::klog!("couldn't find a stream for {page_url}: {e}");
                e
            })?
        };
        crate::klog!("resolved {page_url} in {:.1} s ({})", started.elapsed().as_secs_f32(), r.ext);
        if r.hls {
            tokio::fs::create_dir_all(&self.cache_dir).await.map_err(|e| e.to_string())?;
            let stem = self.cache_dir.join(cache_key(&slot_key(page_url, saver)));
            let path = self.yt.download(page_url, &stem).await?;
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
            return Ok((Arc::new(Entry { source: Source::File { path, mime: mime_for(&ext) }, fresh_until: u64::MAX }), None));
        }
        let (fresh, stated) = fresh_until(&r.url);
        let link = stated.then(|| SavedLink {
            saver: Some(saver),
            url: r.url.clone(),
            headers: r.headers.clone(),
            ext: r.ext.clone(),
            available_at: r.available_at,
            fresh_until: fresh,
        });
        let source = Source::Remote {
            url: r.url,
            headers: r.headers,
            mime: mime_for(&r.ext),
            available_at: r.available_at,
            give_up: Some(Instant::now() + LINK_DEADLINE),
        };
        Ok((Arc::new(Entry { source, fresh_until: fresh }), link))
    }

    pub async fn handle(&self, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
        let resp = self.handle_inner(&req).await;
        let status = resp.status();
        if !status.is_success() {
            crate::klog!(
                "stream {} {} -> {status}: {}",
                req.uri(),
                req.headers().get(header::RANGE).and_then(|v| v.to_str().ok()).unwrap_or("-"),
                String::from_utf8_lossy(resp.body())
            );
        }
        resp
    }

    async fn handle_inner(&self, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
        let Some(page_url) = req
            .uri()
            .query()
            .and_then(|q| q.split('&').find_map(|p| p.strip_prefix("u=")))
            .map(|v| percent_decode_str(v).decode_utf8_lossy().into_owned())
        else {
            return text(StatusCode::BAD_REQUEST, "missing track");
        };
        let range = req
            .headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_range);

        let saver = self.quality_for(&page_url, range.map_or(0, |r| r.0));
        for attempt in 0..2 {
            let entry = match self.entry(&page_url, saver).await {
                Ok(e) => e,
                Err(e) => return text(StatusCode::BAD_GATEWAY, &e),
            };
            let result = match &entry.source {
                Source::File { path, mime } => return serve_path(path, mime, range).await,
                Source::Remote { url, headers, mime, available_at, give_up } => {
                    self.proxy(url, headers, mime, *available_at, *give_up, range).await
                }
            };
            match result {
                Ok(resp) => return resp,
                // The stream URL expired or was refused: resolve once more and retry.
                Err(_) if attempt == 0 => self.forget(&page_url, saver),
                Err(e) => return text(StatusCode::BAD_GATEWAY, &e),
            }
        }
        text(StatusCode::BAD_GATEWAY, "stream unavailable")
    }

    async fn proxy(
        &self,
        url: &str,
        headers: &[(String, String)],
        mime: &str,
        available_at: Option<u64>,
        give_up: Option<Instant>,
        range: Option<(u64, Option<u64>)>,
    ) -> Result<Response<Vec<u8>>, String> {
        let (start, end) = range.unwrap_or((0, None));
        let end = end.map_or(start + CHUNK - 1, |e| e.min(start + CHUNK - 1));

        // YouTube refuses fresh URLs with 403 for a few seconds. `available_at` is the official
        // time, but URLs measured working ~2 s after resolving while `available_at` said ~5 s.
        // So try right away and retry 403s quickly until a little past `available_at`.
        let now = || Duration::from_secs(unix_now());
        let ready = available_at.map(Duration::from_secs);

        let mut tries = 0;
        let resp = loop {
            let mut up = self.http.get(url);
            for (k, v) in headers {
                up = up.header(k.as_str(), v.as_str());
            }
            let resp = up
                .header(header::RANGE, format!("bytes={start}-{end}"))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            // Some URLs never become usable; past this point a fresh URL is the faster way out.
            let in_grace = ready.is_some_and(|r| now() < r + Duration::from_secs(2))
                && give_up.is_none_or(|d| Instant::now() < d);
            if resp.status() == reqwest::StatusCode::FORBIDDEN && in_grace && tries < 60 {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(300)).await;
                continue;
            }
            break resp;
        };

        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::GONE {
            return Err(format!("upstream refused the stream ({status})"));
        }
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(Response::builder().status(416).body(Vec::new()).unwrap());
        }
        if !status.is_success() {
            return Err(format!("upstream error ({status})"));
        }
        let content_range = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();

        let mut b = Response::builder()
            .status(status.as_u16())
            .header(header::CONTENT_TYPE, mime)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::CONTENT_LENGTH, body.len());
        if let Some(cr) = content_range {
            b = b.header(header::CONTENT_RANGE, cr);
        }
        Ok(b.body(body).unwrap())
    }
}

/// Serves part of a file. Open-ended ranges are capped at `CHUNK`, so large files (FLAC) are
/// read piece by piece instead of all at once.
pub(crate) async fn serve_path(path: &Path, mime: &str, range: Option<(u64, Option<u64>)>) -> Response<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => return text(StatusCode::NOT_FOUND, &e.to_string()),
    };
    let total = match file.metadata().await {
        Ok(m) => m.len(),
        Err(e) => return text(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let b = Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    let (start, end, partial) = match range {
        Some((start, _)) if start >= total => {
            return b.status(416).header(header::CONTENT_RANGE, format!("bytes */{total}")).body(Vec::new()).unwrap()
        }
        Some((start, end)) => (start, end.unwrap_or(start + CHUNK - 1).min(start + CHUNK - 1).min(total - 1), true),
        // No range: small files go whole, large ones as a first chunk the browser continues from.
        None if total <= CHUNK => (0, total.saturating_sub(1), false),
        None => (0, CHUNK - 1, true),
    };
    let mut body = vec![0u8; (end + 1 - start) as usize];
    if total > 0 {
        if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
            return text(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
        }
        if let Err(e) = file.read_exact(&mut body).await {
            return text(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
        }
    } else {
        body.clear();
    }
    let b = b.header(header::CONTENT_LENGTH, body.len());
    if partial {
        b.status(206).header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}")).body(body).unwrap()
    } else {
        b.status(200).body(body).unwrap()
    }
}

/// Parses `bytes=START-` or `bytes=START-END`.
pub(crate) fn parse_range(v: &str) -> Option<(u64, Option<u64>)> {
    let (start, end) = v.strip_prefix("bytes=")?.split(',').next()?.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end = end.trim();
    let end: Option<u64> = if end.is_empty() { None } else { Some(end.parse().ok()?) };
    // A backwards range (`bytes=500-100`) is invalid: ignore it rather than underflow.
    if end.is_some_and(|e| e < start) {
        return None;
    }
    Some((start, end))
}

pub(crate) fn mime_for(ext: &str) -> &'static str {
    match ext {
        "webm" | "weba" => "audio/webm",
        "m4a" | "mp4" | "aac" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "opus" | "ogg" | "oga" => "audio/ogg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn cache_key(s: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(s.as_bytes()).iter().take(12).map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn text(status: StatusCode, msg: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(msg.as_bytes().to_vec())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_trusted_until_shortly_before_they_expire() {
        let (t, stated) = fresh_until("https://rr1.googlevideo.com/videoplayback?expire=1800000000&ei=x&ip=1.2.3.4");
        assert_eq!((t, stated), (1_800_000_000 - EXPIRY_MARGIN, true));
        let (t, stated) = fresh_until("https://cf-media.sndcdn.com/abc.mp3?Policy=x");
        assert!(!stated && t > unix_now() + TTL - 5);
    }

    /// Links resolved in one run are there in the next, and expired ones aren't.
    #[test]
    fn saved_links_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("kite-links-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let new = || Streams::new(Arc::new(YtDlp::new(&dir.join("bin"), &dir)), reqwest::Client::new(), dir.clone());
        let link = |until| SavedLink { saver: Some(false), url: "https://x/v?expire=1".into(), headers: vec![], ext: "webm".into(), available_at: None, fresh_until: until };
        let s = new();
        s.save_links(|m| {
            m.insert("https://www.youtube.com/watch?v=live".into(), link(unix_now() + 3600));
            m.insert("https://www.youtube.com/watch?v=old".into(), link(unix_now() + 30));
        });
        let s2 = new();
        let slots = s2.slots.lock().unwrap();
        assert!(slots.contains_key("https://www.youtube.com/watch?v=live"));
        assert!(!slots.contains_key("https://www.youtube.com/watch?v=old"), "less than a minute left: resolve again");
        drop(slots);
        s2.forget("https://www.youtube.com/watch?v=live", false);
        assert!(new().slots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Switching Data saver while a song plays must not move the rest of that song to another
    /// file: the bytes wouldn't line up and the song would fail partway. Needs the internet and
    /// the yt-dlp Kite downloaded: `cargo test -- --ignored saver_switch`.
    #[test]
    #[ignore]
    fn saver_switch_keeps_a_playing_song_on_its_file() {
        tauri::async_runtime::block_on(saver_switch());
    }

    async fn saver_switch() {
        let app_dir = PathBuf::from(std::env::var("APPDATA").unwrap()).join("app.kite.player");
        let dir = std::env::temp_dir().join(format!("kite-saver-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let yt = Arc::new(YtDlp::ready_for_tests(&app_dir.join("bin"), &dir).await);
        let s = Streams::new(yt, reqwest::Client::new(), dir.clone());
        let page = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let get = |start: u64| {
            Request::builder()
                .uri(format!("http://kite.localhost/play?u={}", percent_encoding::utf8_percent_encode(page, percent_encoding::NON_ALPHANUMERIC)))
                .header(header::RANGE, format!("bytes={start}-"))
                .body(Vec::new())
                .unwrap()
        };
        // The file's size, from "bytes 0-2097151/3433041".
        let size = |r: &Response<Vec<u8>>| {
            assert!(r.status().is_success(), "{}: {}", r.status(), String::from_utf8_lossy(r.body()));
            let cr = r.headers()[header::CONTENT_RANGE].to_str().unwrap().to_string();
            cr.rsplit('/').next().unwrap().parse::<u64>().unwrap()
        };
        s.prepare(page).await.unwrap();
        let best = size(&s.handle(get(0)).await);
        s.set_saver(true);
        // The playing song asks for its next piece: still the same file.
        assert_eq!(size(&s.handle(get(CHUNK)).await), best, "the playing song moved to another file");
        // Played again from the start, it uses the smaller stream.
        s.prepare(page).await.unwrap();
        let small = size(&s.handle(get(0)).await);
        assert!(small < best, "data saver should give a smaller file ({small} vs {best})");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A song keeps the quality it started at; the next song started takes the new setting.
    #[test]
    fn a_song_keeps_the_quality_it_started_at() {
        let dir = std::env::temp_dir().join(format!("kite-quality-test-{}", std::process::id()));
        let s = Streams::new(Arc::new(YtDlp::new(&dir.join("bin"), &dir)), reqwest::Client::new(), dir.clone());
        assert!(!s.quality_for("a", 0));
        s.set_saver(true);
        assert!(!s.quality_for("a", CHUNK), "song a started at best quality");
        assert!(s.quality_for("b", 0), "song b starts after the switch");
        assert!(s.quality_for("a", 0), "song a played again from the start");
        assert!(s.quality_for("c", CHUNK), "unknown song: the current setting");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Links saved before 0.4.1 don't say their quality, so they're resolved again.
    #[test]
    fn links_without_a_quality_are_not_used() {
        let dir = std::env::temp_dir().join(format!("kite-oldlinks-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let until = unix_now() + 3600;
        let link = |saver: &str| format!(r#"{{{saver}"url":"u","headers":[],"ext":"webm","available_at":null,"fresh_until":{until}}}"#);
        std::fs::write(dir.join("links.json"), format!(r#"{{"old":{},"saver new":{}}}"#, link(""), link(r#""saver":true,"#))).unwrap();
        let s = Streams::new(Arc::new(YtDlp::new(&dir.join("bin"), &dir)), reqwest::Client::new(), dir.clone());
        let slots = s.slots.lock().unwrap();
        assert!(!slots.contains_key("old") && slots.contains_key("saver new"));
        drop(slots);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_ranges_and_rejects_backwards_ones() {
        assert_eq!(parse_range("bytes=0-"), Some((0, None)));
        assert_eq!(parse_range("bytes=100-199"), Some((100, Some(199))));
        assert_eq!(parse_range("bytes=500-100"), None);
        assert_eq!(parse_range("items=0-1"), None);
    }
}
