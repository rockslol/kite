//! Discord status ("Listening to Kite"): the song, artist, cover, a progress bar and a button
//! to the song. Talks to the Discord app over its local IPC pipe; nothing goes over the network
//! from Kite. Off by default (it shows friends what you're playing).

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

/// Kite's application in the Discord developer portal.
const CLIENT_ID: &str = "1573498807621132298";
/// Discord allows about 5 status changes per 20 s; changes inside this window are merged.
const MIN_GAP: Duration = Duration::from_millis(2500);
/// How often to look for Discord again when it isn't running.
const RETRY: Duration = Duration::from_secs(20);

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Song {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub artwork: Option<String>,
    pub url: Option<String>,
    pub duration: Option<f64>,
}

#[derive(Clone, Default, PartialEq, Debug)]
struct Now {
    song: Option<Song>,
    playing: bool,
    /// Unix ms when the song would have started, given the current position.
    started_ms: i64,
    enabled: bool,
}

pub struct Discord {
    tx: Mutex<Sender<Now>>,
    now: Mutex<Now>,
}

fn unix_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl Discord {
    pub fn start(enabled: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || worker(rx));
        let d = Self { tx: Mutex::new(tx), now: Mutex::new(Now { enabled, ..Default::default() }) };
        d.send();
        d
    }

    fn send(&self) {
        let now = self.now.lock().unwrap().clone();
        let _ = self.tx.lock().unwrap().send(now);
    }

    pub fn set_enabled(&self, on: bool) {
        self.now.lock().unwrap().enabled = on;
        self.send();
    }

    pub fn set_song(&self, song: Song) {
        {
            let mut n = self.now.lock().unwrap();
            n.song = Some(song);
            n.started_ms = unix_ms();
        }
        self.send();
    }

    /// From the player's state reports (also every few seconds while playing): only a real
    /// change (play/pause, or a seek that moves the progress bar) reaches Discord.
    pub fn set_state(&self, playing: bool, position: Option<f64>) {
        let changed = {
            let mut n = self.now.lock().unwrap();
            let started = position.map_or(n.started_ms, |p| unix_ms() - (p * 1000.0) as i64);
            let moved = (started - n.started_ms).abs() > 2000;
            let changed = n.playing != playing || (playing && moved);
            n.playing = playing;
            if moved || !playing {
                n.started_ms = started;
            }
            changed
        };
        if changed {
            self.send();
        }
    }
}

/// Builds the activity for the current state, or `None` to clear it.
fn activity(n: &Now) -> Option<Value> {
    let s = n.song.as_ref().filter(|_| n.enabled && n.playing)?;
    // Discord wants 2-128 characters in these fields.
    let field = |t: &str| {
        let t: String = t.chars().take(128).collect();
        if t.chars().count() < 2 { format!("{t}  ") } else { t }
    };
    let mut a = json!({
        "type": 2, // "Listening to"
        "details": field(&s.title),
        "state": field(if s.artist.is_empty() { "Unknown artist" } else { &s.artist }),
    });
    if let Some(d) = s.duration.filter(|d| d.is_finite() && *d > 0.0) {
        a["timestamps"] = json!({ "start": n.started_ms, "end": n.started_ms + (d * 1000.0) as i64 });
    }
    if let Some(art) = s.artwork.as_deref().filter(|u| u.starts_with("https://")) {
        a["assets"] = json!({ "large_image": art, "large_text": field(s.album.as_deref().unwrap_or(&s.title)) });
    }
    if let Some((label, url)) = s.url.as_deref().and_then(button) {
        a["buttons"] = json!([{ "label": label, "url": url }]);
    }
    Some(a)
}

/// The "listen" button for a song page; none for local files.
fn button(url: &str) -> Option<(&'static str, String)> {
    if let Some(id) = url.split_once("watch?v=").map(|(_, id)| id.split('&').next().unwrap_or(id)) {
        return Some(("Listen on YouTube Music", format!("https://music.youtube.com/watch?v={id}")));
    }
    if url.starts_with("https://soundcloud.com/") {
        return Some(("Listen on SoundCloud", url.to_string()));
    }
    None
}

fn worker(rx: Receiver<Now>) {
    let mut pipe: Option<Pipe> = None;
    let mut last_try: Option<Instant> = None;
    let mut last_sent: Option<Option<Value>> = None;
    let mut last_send_at = Instant::now() - MIN_GAP;
    let mut latest: Option<Now> = None;
    loop {
        // Wait for news; when Discord wasn't there, wake up now and then to look again.
        let wait = if pipe.is_none() && latest.as_ref().is_some_and(|n| activity(n).is_some()) { RETRY } else { Duration::from_secs(3600) };
        match rx.recv_timeout(wait) {
            Ok(n) => latest = Some(n),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Merge a burst of changes (skipping through songs) into one update.
        let gap = MIN_GAP.saturating_sub(last_send_at.elapsed());
        std::thread::sleep(gap);
        while let Ok(n) = rx.try_recv() {
            latest = Some(n);
        }
        let Some(n) = &latest else { continue };
        let want = activity(n);
        if last_sent.as_ref() == Some(&want) && pipe.is_some() {
            continue;
        }
        if !n.enabled {
            // Switched off: clear and let go of Discord.
            if let Some(mut p) = pipe.take() {
                let _ = p.set_activity(None);
            }
            last_sent = None;
            continue;
        }
        if pipe.is_none() {
            if want.is_none() || last_try.is_some_and(|t| t.elapsed() < RETRY) {
                continue;
            }
            last_try = Some(Instant::now());
            pipe = Pipe::connect();
            if pipe.is_none() {
                continue;
            }
        }
        if let Some(p) = pipe.as_mut() {
            match p.set_activity(want.clone()) {
                Ok(()) => {
                    last_sent = Some(want);
                    last_send_at = Instant::now();
                }
                Err(e) => {
                    crate::klog!("Discord status: {e}");
                    pipe = None; // Discord closed; look for it again later
                    last_sent = None;
                }
            }
        }
    }
}

// ---- the IPC pipe ----------------------------------------------------------------------

#[cfg(windows)]
type Stream = std::fs::File;
#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;

struct Pipe {
    stream: Stream,
    nonce: u64,
}

impl Pipe {
    fn connect() -> Option<Self> {
        for i in 0..10 {
            if let Some(stream) = open(i) {
                let mut p = Pipe { stream, nonce: 0 };
                if p.frame(0, &json!({ "v": 1, "client_id": CLIENT_ID })).is_ok() {
                    return Some(p);
                }
            }
        }
        None
    }

    fn set_activity(&mut self, activity: Option<Value>) -> Result<(), String> {
        self.nonce += 1;
        let msg = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity },
            "nonce": self.nonce.to_string(),
        });
        let reply = self.frame(1, &msg)?;
        if reply.get("evt").and_then(Value::as_str) == Some("ERROR") {
            return Err(reply["data"]["message"].as_str().unwrap_or("rejected").to_string());
        }
        Ok(())
    }

    /// Sends one message and reads Discord's answer: `[op u32 LE][length u32 LE][JSON]`.
    fn frame(&mut self, op: u32, body: &Value) -> Result<Value, String> {
        let data = body.to_string().into_bytes();
        let mut buf = Vec::with_capacity(8 + data.len());
        buf.extend_from_slice(&op.to_le_bytes());
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&data);
        self.stream.write_all(&buf).map_err(|e| e.to_string())?;
        let mut head = [0u8; 8];
        self.stream.read_exact(&mut head).map_err(|e| e.to_string())?;
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if len > 1 << 20 {
            return Err("reply too large".into());
        }
        let mut reply = vec![0u8; len];
        self.stream.read_exact(&mut reply).map_err(|e| e.to_string())?;
        if u32::from_le_bytes([head[0], head[1], head[2], head[3]]) == 2 {
            return Err("Discord closed the connection".into());
        }
        serde_json::from_slice(&reply).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
fn open(i: u32) -> Option<Stream> {
    std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}")).ok()
}

#[cfg(unix)]
fn open(i: u32) -> Option<Stream> {
    let dirs = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(|v| std::env::var(v).ok()).chain(["/tmp".into()]);
    for dir in dirs {
        let dir = dir.trim_end_matches('/').to_string();
        for sub in ["", "/app/com.discordapp.Discord", "/snap.discord"] {
            if let Ok(s) = std::os::unix::net::UnixStream::connect(format!("{dir}{sub}/discord-ipc-{i}")) {
                return Some(s);
            }
        }
    }
    None
}

/// Applies the setting (called at startup and when Settings change).
pub fn refresh(app: &AppHandle) {
    let on = app.state::<crate::app::Prefs>().get().discord;
    if let Some(d) = app.try_state::<Discord>() {
        d.set_enabled(on);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        Song {
            title: "sol".into(),
            artist: "Che".into(),
            album: None,
            artwork: Some("https://i.ytimg.com/vi/x/hq.jpg".into()),
            url: Some("https://www.youtube.com/watch?v=abc123&list=x".into()),
            duration: Some(135.0),
        }
    }

    #[test]
    fn builds_a_listening_activity() {
        let n = Now { song: Some(song()), playing: true, started_ms: 1_000, enabled: true };
        let a = activity(&n).unwrap();
        assert_eq!(a["type"], 2);
        assert_eq!(a["details"], "sol");
        assert_eq!(a["state"], "Che");
        assert_eq!(a["timestamps"]["end"], 136_000);
        assert_eq!(a["buttons"][0]["url"], "https://music.youtube.com/watch?v=abc123");
        assert_eq!(a["assets"]["large_image"], "https://i.ytimg.com/vi/x/hq.jpg");
    }

    #[test]
    fn clears_when_paused_or_switched_off() {
        let n = Now { song: Some(song()), playing: false, started_ms: 0, enabled: true };
        assert!(activity(&n).is_none());
        let n = Now { song: Some(song()), playing: true, started_ms: 0, enabled: false };
        assert!(activity(&n).is_none());
    }

    #[test]
    fn short_titles_are_padded_and_local_files_get_no_button() {
        let mut s = song();
        s.title = "X".into();
        s.url = Some("local:C:\\Music\\x.mp3".into());
        s.artwork = Some("art:abc.jpg".into());
        let a = activity(&Now { song: Some(s), playing: true, started_ms: 0, enabled: true }).unwrap();
        assert_eq!(a["details"].as_str().unwrap().chars().count(), 3);
        assert!(a.get("buttons").is_none() && a.get("assets").is_none());
    }
}
