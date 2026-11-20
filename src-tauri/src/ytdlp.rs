//! Manages the yt-dlp binary (download, verify, self-update) and runs it for search and stream resolution.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::RwLock;
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

/// QuickJS, the JavaScript runtime yt-dlp uses for YouTube when neither Deno nor Node is
/// installed. Pinned to one release and checked against its published SHA-256.
const QJS_RELEASE: &str = "https://github.com/quickjs-ng/quickjs/releases/download/v0.18.0";
#[cfg(all(windows, target_arch = "x86_64"))]
const QJS: Option<(&str, &str, &str)> =
    Some(("qjs-windows-x86_64.exe", "qjs.exe", "ff03398d74ea6e6bd3a8e58058f7003225abcac229d7d57775b7aaae812a49c7"));
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const QJS: Option<(&str, &str, &str)> =
    Some(("qjs-darwin-arm64", "qjs", "91fe89916d3b0a6c01c4dc8382a99c3c1fd91ecfc214aa81ca051902bcce590d"));
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const QJS: Option<(&str, &str, &str)> =
    Some(("qjs-darwin-x86_64", "qjs", "fa2c5dc78095daff566b32fab995ced12927875253b84216b91b834af2ef2b74"));
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const QJS: Option<(&str, &str, &str)> =
    Some(("qjs-linux-x86_64", "qjs", "85ded23b9fabe8c1644fc40a8d2bbdd71157affcbd807359c618cc717ee24d7d"));
#[cfg(not(any(
    all(windows, target_arch = "x86_64"),
    all(target_os = "macos", any(target_arch = "aarch64", target_arch = "x86_64")),
    all(target_os = "linux", target_arch = "x86_64")
)))]
const QJS: Option<(&str, &str, &str)> = None;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ToolStatus {
    Checking,
    Downloading,
    Ready,
    Error { message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub source: String,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub artwork: Option<String>,
    /// SoundCloud Go+ tracks only play a 30 second preview without a subscription.
    #[serde(default)]
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

#[derive(Clone, Debug, Serialize, Default)]
pub struct ToolInfo {
    pub ytdlp_version: String,
    pub js_runtime: String,
}

pub struct YtDlp {
    bin: PathBuf,
    bin_dir: PathBuf,
    cache_dir: PathBuf,
    js_args: RwLock<Vec<String>>,
    info: RwLock<ToolInfo>,
    status: watch::Sender<ToolStatus>,
}

impl YtDlp {
    pub fn new(bin_dir: &Path, cache_dir: &Path) -> Self {
        let (status, _) = watch::channel(ToolStatus::Checking);
        Self {
            bin: bin_dir.join(ASSET),
            bin_dir: bin_dir.to_path_buf(),
            cache_dir: cache_dir.join("yt-dlp"),
            js_args: RwLock::new(vec![]),
            info: RwLock::new(ToolInfo::default()),
            status,
        }
    }

    pub fn info(&self) -> ToolInfo {
        self.info.read().map(|i| i.clone()).unwrap_or_default()
    }

    /// yt-dlp needs a JavaScript runtime for full YouTube support. Prefer Deno (yt-dlp's
    /// default) or Node if installed; otherwise download QuickJS next to yt-dlp.
    async fn pick_js_runtime(&self, http: &reqwest::Client) {
        let (name, args): (&str, Vec<String>) = if find_on_path("deno").is_some() {
            ("Deno", vec![])
        } else if find_on_path("node").is_some() {
            ("Node.js", vec!["--js-runtimes".into(), "node".into()])
        } else if let Some((asset, file, sha)) = QJS {
            let path = self.bin_dir.join(file);
            let url = format!("{QJS_RELEASE}/{asset}");
            let ok = path.exists() || download_verified(http, &url, sha, &path).await.is_ok();
            if ok {
                let arg = format!("quickjs:{}", path.display());
                ("QuickJS", vec!["--no-js-runtimes".into(), "--js-runtimes".into(), arg])
            } else {
                ("none", vec![])
            }
        } else {
            ("none", vec![])
        };
        if let Ok(mut a) = self.js_args.write() {
            *a = args;
        }
        if let Ok(mut i) = self.info.write() {
            i.js_runtime = name.to_string();
        }
    }

    async fn read_version(&self) {
        if let Ok(out) = self.run(&["--version"]).await {
            if let Ok(mut i) = self.info.write() {
                i.ytdlp_version = String::from_utf8_lossy(&out).trim().to_string();
            }
        }
    }

    /// Updates yt-dlp now (Settings button). Returns the new version.
    pub async fn update_now(&self) -> Result<String, String> {
        self.wait_ready().await?;
        self.run(&["--update"]).await?;
        if let Ok(f) = std::fs::File::options().write(true).open(&self.bin) {
            let _ = f.set_modified(SystemTime::now());
        }
        self.read_version().await;
        Ok(self.info().ytdlp_version)
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
        let fresh_install = !self.bin.exists();
        if fresh_install {
            self.set_status(&app, ToolStatus::Downloading);
            if let Err(e) = self.fetch(&http).await {
                self.set_status(&app, ToolStatus::Error { message: format!("Couldn't download yt-dlp: {e}") });
                return;
            }
        }
        self.pick_js_runtime(&http).await;
        self.set_status(&app, ToolStatus::Ready);
        self.read_version().await;
        if fresh_install {
            return;
        }

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
            self.read_version().await;
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
            .args(self.js_args.read().map(|a| a.clone()).unwrap_or_default())
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

    /// Reads a YouTube/YouTube Music playlist or SoundCloud set/likes. Returns its title and songs.
    pub async fn playlist(&self, url: &str) -> Result<(String, Vec<Track>), String> {
        self.wait_ready().await?;
        let source = source_of(url);
        let out = self.run(&["--flat-playlist", "-J", "--", url]).await?;
        let v: Value = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
        let title = v["title"].as_str().unwrap_or("Imported playlist").to_string();
        let entries = v["entries"].as_array().cloned().unwrap_or_default();
        Ok((title, entries.iter().filter_map(|e| to_track(source, e)).collect()))
    }

    /// Metadata for a single YouTube or SoundCloud link.
    pub async fn lookup(&self, url: &str) -> Result<Track, String> {
        self.wait_ready().await?;
        let out = self.run(&["-J", "--no-playlist", "--skip-download", "--", url]).await?;
        let v: Value = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
        to_track(source_of(url), &v).ok_or_else(|| "Couldn't read that link.".to_string())
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

pub fn source_of(url: &str) -> &'static str {
    if url.contains("soundcloud.com") {
        "soundcloud"
    } else {
        "youtube"
    }
}

fn to_track(source: &str, e: &Value) -> Option<Track> {
    if e["live_status"].as_str() == Some("is_live") {
        return None;
    }
    let id = e["id"].as_str()?.to_string();
    let title = e["title"].as_str()?.to_string();
    // One URL form per YouTube video, so music.youtube.com and youtube.com links are the same song.
    let url = if source == "youtube" {
        format!("https://www.youtube.com/watch?v={id}")
    } else {
        e["webpage_url"].as_str().or_else(|| e["url"].as_str())?.to_string()
    };
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
        album: None,
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

/// Downloads a file and keeps it only if its SHA-256 matches.
async fn download_verified(http: &reqwest::Client, url: &str, sha256: &str, dest: &Path) -> Result<(), String> {
    let bytes = http
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if actual != sha256 {
        return Err("checksum mismatch".into());
    }
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
    }
    let part = dest.with_extension("part");
    tokio::fs::write(&part, &bytes).await.map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
    }
    tokio::fs::rename(&part, dest).await.map_err(|e| e.to_string())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd"] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter().map(|ext| dir.join(format!("{name}{ext}"))).find(|p| p.is_file())
    })
}
