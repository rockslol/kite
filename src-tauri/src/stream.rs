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
use std::time::{Duration, Instant};
use tauri::http::{header, Request, Response, StatusCode};
use tokio::sync::OnceCell;

/// Largest piece fetched from the upstream server per request. The audio element asks for the
/// next piece as it plays.
const CHUNK: u64 = 2 * 1024 * 1024;
/// Stream URLs expire upstream (YouTube's after about 6 hours); resolve again well before that.
const TTL: Duration = Duration::from_secs(60 * 60);

enum Source {
    Remote { url: String, headers: Vec<(String, String)>, mime: &'static str, available_at: Option<u64> },
    File { path: PathBuf, mime: &'static str },
}

struct Entry {
    source: Source,
    at: Instant,
}

type Slot = Arc<OnceCell<Result<Arc<Entry>, String>>>;

pub struct Streams {
    yt: Arc<YtDlp>,
    http: reqwest::Client,
    cache_dir: PathBuf,
    slots: Mutex<HashMap<String, Slot>>,
    /// Caps parallel yt-dlp runs, since the UI resolves songs ahead on hover.
    resolving: tokio::sync::Semaphore,
}

impl Streams {
    pub fn new(yt: Arc<YtDlp>, http: reqwest::Client, cache_dir: PathBuf) -> Self {
        Self {
            yt,
            http,
            cache_dir: cache_dir.join("audio"),
            slots: Mutex::new(HashMap::new()),
            resolving: tokio::sync::Semaphore::new(3),
        }
    }

    /// Resolves a track page URL to something playable. Concurrent callers share one lookup.
    pub async fn prepare(&self, page_url: &str) -> Result<(), String> {
        self.entry(page_url).await.map(|_| ())
    }

    async fn entry(&self, page_url: &str) -> Result<Arc<Entry>, String> {
        let slot = {
            let mut slots = self.slots.lock().unwrap();
            let fresh = |s: &Slot| match s.get() {
                // Cached files stay valid until the file is gone (Settings → Clear cache).
                Some(Ok(e)) => match &e.source {
                    Source::File { path, .. } => path.exists(),
                    Source::Remote { .. } => e.at.elapsed() < TTL,
                },
                Some(Err(_)) => false,
                None => true,
            };
            match slots.get(page_url) {
                Some(s) if fresh(s) => s.clone(),
                _ => {
                    let s: Slot = Arc::new(OnceCell::new());
                    slots.insert(page_url.to_string(), s.clone());
                    s
                }
            }
        };
        slot.get_or_init(|| self.load(page_url)).await.clone()
    }

    fn forget(&self, page_url: &str) {
        self.slots.lock().unwrap().remove(page_url);
    }

    async fn load(&self, page_url: &str) -> Result<Arc<Entry>, String> {
        let r = {
            let _permit = self.resolving.acquire().await.map_err(|e| e.to_string())?;
            self.yt.resolve(page_url).await?
        };
        let source = if r.hls {
            tokio::fs::create_dir_all(&self.cache_dir).await.map_err(|e| e.to_string())?;
            let stem = self.cache_dir.join(cache_key(page_url));
            let path = self.yt.download(page_url, &stem).await?;
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
            Source::File { path, mime: mime_for(&ext) }
        } else {
            Source::Remote { url: r.url, headers: r.headers, mime: mime_for(&r.ext), available_at: r.available_at }
        };
        Ok(Arc::new(Entry { source, at: Instant::now() }))
    }

    pub async fn handle(&self, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
        let resp = self.handle_inner(&req).await;
        let status = resp.status();
        if !status.is_success() {
            eprintln!(
                "[kite] {} {} -> {status}: {}",
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

        for attempt in 0..2 {
            let entry = match self.entry(&page_url).await {
                Ok(e) => e,
                Err(e) => return text(StatusCode::BAD_GATEWAY, &e),
            };
            let result = match &entry.source {
                Source::File { path, mime } => return serve_path(path, mime, range).await,
                Source::Remote { url, headers, mime, available_at } => {
                    self.proxy(url, headers, mime, *available_at, range).await
                }
            };
            match result {
                Ok(resp) => return resp,
                // The stream URL expired or was refused: resolve once more and retry.
                Err(_) if attempt == 0 => self.forget(&page_url),
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
        range: Option<(u64, Option<u64>)>,
    ) -> Result<Response<Vec<u8>>, String> {
        let (start, end) = range.unwrap_or((0, None));
        let end = end.map_or(start + CHUNK - 1, |e| e.min(start + CHUNK - 1));

        // YouTube refuses fresh URLs with 403 for a few seconds. `available_at` is the official
        // time, but URLs measured working ~2 s after resolving while `available_at` said ~5 s.
        // So try right away and retry 403s quickly until a little past `available_at`.
        let unix_now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let ready = available_at.map(|t| Duration::from_secs(t));

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
            let in_grace = ready.is_some_and(|r| unix_now() < r + Duration::from_secs(2));
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
    use super::parse_range;

    #[test]
    fn parses_ranges_and_rejects_backwards_ones() {
        assert_eq!(parse_range("bytes=0-"), Some((0, None)));
        assert_eq!(parse_range("bytes=100-199"), Some((100, Some(199))));
        assert_eq!(parse_range("bytes=500-100"), None);
        assert_eq!(parse_range("items=0-1"), None);
    }
}
