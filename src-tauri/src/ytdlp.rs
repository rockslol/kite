//! Manages the yt-dlp binary (download, verify, self-update) and runs it for search and stream resolution.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use tokio::sync::watch;

const RELEASE_BASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
const RUN_TIMEOUT: Duration = Duration::from_secs(90);
const UPDATE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[cfg(windows)]
const ASSET: &str = "yt-dlp.exe";
#[cfg(target_os = "macos")]
const ASSET: &str = "yt-dlp_macos";
#[cfg(all(unix, not(target_os = "macos")))]
const ASSET: &str = "yt-dlp_linux";

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ToolStatus {
    Checking,
    Downloading,
    Ready,
    Error { message: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct Track {
    pub id: String,
    pub source: String,
    pub url: String,
    pub title: String,
    pub artist: String,
    pub duration: Option<f64>,
    pub artwork: Option<String>,
    /// SoundCloud Go+ tracks only play a 30 second preview without a subscription.
    pub preview: bool,
}

#[derive(Clone, Debug)]
pub struct Resolved {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub ext: String,
    pub hls: bool,
    /// Unix time (seconds) before which YouTube refuses the URL with 403. Fresh URLs from the
    /// TV client become usable a few seconds after they're issued.
    pub available_at: Option<u64>,
}

pub struct YtDlp {
    bin: PathBuf,
    cache_dir: PathBuf,
    js_args: Vec<String>,
    status: watch::Sender<ToolStatus>,
}

impl YtDlp {
    pub fn new(bin_dir: &Path, cache_dir: &Path) -> Self {
        // yt-dlp needs a JavaScript runtime for full YouTube support. It uses Deno by default;
        // if only Node is installed, tell it to use that instead.
        let js_args = if find_on_path("deno").is_none() && find_on_path("node").is_some() {
            vec!["--js-runtimes".into(), "node".into()]
        } else {
            vec![]
        };
        let (status, _) = watch::channel(ToolStatus::Checking);
        Self {
            bin: bin_dir.join(ASSET),
            cache_dir: cache_dir.join("yt-dlp"),
            js_args,
            status,
        }
    }

    pub fn status(&self) -> ToolStatus {
        self.status.borrow().clone()
    }

    fn set_status(&self, app: &AppHandle, s: ToolStatus) {
        self.status.send_replace(s.clone());
        let _ = app.emit("tool-status", s);
    }

    /// Makes sure yt-dlp is installed and recent. Runs once at startup in the background.
    pub async fn ensure(&self, app: AppHandle, http: reqwest::Client) {
        if !self.bin.exists() {
            self.set_status(&app, ToolStatus::Downloading);
            if let Err(e) = self.fetch(&http).await {
                self.set_status(&app, ToolStatus::Error { message: format!("Couldn't download yt-dlp: {e}") });
                return;
            }
            self.set_status(&app, ToolStatus::Ready);
            return;
        }
        self.set_status(&app, ToolStatus::Ready);

        let stale = std::fs::metadata(&self.bin)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > UPDATE_EVERY);
        if stale {
            // yt-dlp updates itself in place and verifies its own signature.
            let _ = self.run(&["--update"]).await;
            if let Ok(f) = std::fs::File::options().write(true).open(&self.bin) {
                let _ = f.set_modified(SystemTime::now());
            }
        }
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<(), String> {
        let sums = http
            .get(format!("{RELEASE_BASE}/SHA2-256SUMS"))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let expected = sums
            .lines()
            .find_map(|l| {
                let (hash, name) = l.split_once(char::is_whitespace)?;
                (name.trim() == ASSET).then(|| hash.to_ascii_lowercase())
            })
            .ok_or("no checksum listed for this platform")?;

        let bytes = http
            .get(format!("{RELEASE_BASE}/{ASSET}"))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;

        let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if actual != expected {
            return Err("checksum mismatch".into());
        }

        if let Some(dir) = self.bin.parent() {
            tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
        }
        let part = self.bin.with_extension("part");
        tokio::fs::write(&part, &bytes).await.map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
        }
        tokio::fs::rename(&part, &self.bin).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn wait_ready(&self) -> Result<(), String> {
        let mut rx = self.status.subscribe();
        loop {
            match rx.borrow_and_update().clone() {
                ToolStatus::Ready => return Ok(()),
                ToolStatus::Error { message } => return Err(message),
                _ => {}
            }
            rx.changed().await.map_err(|e| e.to_string())?;
        }
    }

    async fn run(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(["--ignore-config", "--no-warnings", "--no-progress", "--cache-dir"])
            .arg(&self.cache_dir)
            .args(&self.js_args)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

        let out = tokio::time::timeout(RUN_TIMEOUT, cmd.output())
            .await
            .map_err(|_| "yt-dlp took too long".to_string())?
            .map_err(|e| format!("couldn't start yt-dlp: {e}"))?;

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err
                .lines()
                .rev()
                .find(|l| l.contains("ERROR"))
                .or_else(|| err.lines().last())
                .unwrap_or("yt-dlp failed");
            return Err(line.trim_start_matches("ERROR:").trim().to_string());
        }
        Ok(out.stdout)
    }

    pub async fn search(&self, source: &str, query: &str) -> Result<Vec<Track>, String> {
        self.wait_ready().await?;
        let q = match source {
            "soundcloud" => format!("scsearch20:{query}"),
            _ => format!("ytsearch20:{query}"),
        };
        let out = self.run(&["--flat-playlist", "-J", "--", &q]).await?;
        let v: Value = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
        let entries = v["entries"].as_array().cloned().unwrap_or_default();
        Ok(entries.iter().filter_map(|e| to_track(source, e)).collect())
    }

    pub async fn resolve(&self, url: &str) -> Result<Resolved, String> {
        self.wait_ready().await?;
        // macOS's webview (WebKit) is safest with AAC/MP3, so prefer m4a there. Chromium-based
        // WebView2 plays Opus/WebM fine, which is usually YouTube's best audio.
        #[cfg(target_os = "macos")]
        let format = "bestaudio[ext=m4a][protocol^=http][protocol!*=dash]/bestaudio[ext=mp3]/bestaudio[protocol^=http][protocol!*=dash]/bestaudio/best";
        #[cfg(not(target_os = "macos"))]
        let format = "bestaudio[protocol^=http][protocol!*=dash]/bestaudio[protocol^=http]/bestaudio/best";
        let out = self.run(&["-J", "--no-playlist", "-f", format, "--", url]).await?;
        let v: Value = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
        let stream = v["url"].as_str().ok_or("no playable stream found")?.to_string();
        let headers = v["http_headers"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let protocol = v["protocol"].as_str().unwrap_or("");
        Ok(Resolved {
            url: stream,
            headers,
            ext: v["ext"].as_str().unwrap_or("").to_string(),
            hls: protocol.contains("m3u8") || protocol.contains("dash"),
            available_at: v["available_at"].as_f64().map(|t| t.ceil() as u64),
        })
    }

    /// Downloads a track to `stem.<ext>` and returns the final path. Used for streams that
    /// can't be proxied byte-by-byte (HLS).
    pub async fn download(&self, url: &str, stem: &Path) -> Result<PathBuf, String> {
        self.wait_ready().await?;
        let template = format!("{}.%(ext)s", stem.display());
        let out = self
            .run(&[
                "--no-playlist",
                "-f",
                "bestaudio/best",
                "-o",
                &template,
                "--print",
                "after_move:filepath",
                "--",
                url,
            ])
            .await?;
        let path = String::from_utf8_lossy(&out).lines().last().unwrap_or("").trim().to_string();
        if path.is_empty() {
            return Err("download finished without a file".into());
        }
        Ok(PathBuf::from(path))
    }
}

fn to_track(source: &str, e: &Value) -> Option<Track> {
    if e["live_status"].as_str() == Some("is_live") {
        return None;
    }
    let id = e["id"].as_str()?.to_string();
    let title = e["title"].as_str()?.to_string();
    let url = e["webpage_url"]
        .as_str()
        .or_else(|| e["url"].as_str())?
        .to_string();
    let channel = e["channel"].as_str().or_else(|| e["uploader"].as_str()).unwrap_or("");
    let (title, artist) = match (source, e["artist"].as_str()) {
        (_, Some(artist)) => (title, artist.to_string()),
        ("youtube", None) => crate::titles::clean_youtube(&title, channel),
        _ => (title, channel.to_string()),
    };

    let artwork = if source == "youtube" {
        Some(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"))
    } else {
        pick_thumbnail(&e["thumbnails"]).or_else(|| e["thumbnail"].as_str().map(String::from))
    };

    Some(Track {
        id,
        source: source.to_string(),
        url,
        title,
        artist,
        duration: e["duration"].as_f64(),
        preview: source == "soundcloud" && e["duration"].as_f64().is_some_and(|d| (29.0..=31.0).contains(&d)),
        artwork,
    })
}

fn pick_thumbnail(list: &Value) -> Option<String> {
    let list = list.as_array()?;
    let url = |t: &Value| t["url"].as_str().map(String::from);
    list.iter()
        .find(|t| t["id"].as_str() == Some("t300x300") || t["url"].as_str().is_some_and(|u| u.contains("t300x300")))
        .and_then(url)
        .or_else(|| list.last().and_then(url))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd"] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter().map(|ext| dir.join(format!("{name}{ext}"))).find(|p| p.is_file())
    })
}
