//! Following artists, and watching for new albums and singles.
//!
//! Every few hours Kite reads the YouTube Music pages of the artists you follow and of the ones
//! you play most, and remembers each album and single it sees. The first look at an artist only
//! records what's already out; anything that shows up after that is new: it goes to the top of
//! Home, and for artists you follow it also gets a toast.

use crate::discover::{self, Artist};
use crate::klog;
use crate::library::{now, Library};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

/// How old an artist's last check can get before it's read again.
const RECHECK_SECS: i64 = 6 * 3600;
/// How often the watcher wakes up to look for artists that are due.
const WAKE: Duration = Duration::from_secs(30 * 60);
/// Played artists followed for releases without being followed: the most played in this window.
const PLAYED_SECS: i64 = 60 * 86400;
const TOP_PLAYED: usize = 8;
/// Names that found no YouTube Music artist are tried again after this long.
const NAME_RETRY_SECS: i64 = 30 * 86400;
/// Name lookups (one search each) per run, so a big history doesn't send a burst of searches.
const LOOKUPS_PER_RUN: usize = 4;

type Lib<'a> = State<'a, Arc<Library>>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Followed {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub artwork: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Release {
    pub id: String,
    pub title: String,
    /// "Single • 2026", "2025" (albums), "EP • 2024".
    pub subtitle: String,
    pub artwork: Option<String>,
    pub artist: String,
    pub artist_id: String,
    pub year: Option<i64>,
    /// When Kite first saw it; 0 for what was already out at the artist's first check.
    pub first_seen: i64,
    pub followed: bool,
}

/// Only one check runs at a time (the timer and a fresh follow can overlap).
#[derive(Default)]
pub struct Watcher {
    busy: tokio::sync::Mutex<()>,
}

fn year_of(subtitle: &str) -> Option<i64> {
    subtitle
        .split(|c: char| !c.is_ascii_digit())
        .filter(|w| w.len() == 4)
        .filter_map(|w| w.parse().ok())
        .rfind(|y| (1900..=2200).contains(y))
}

/// Saves what an artist page lists. Returns the releases that are new since an earlier check
/// (none on the first check, which only records what's there).
fn record(c: &mut Connection, artist_id: &str, a: &Artist) -> rusqlite::Result<Vec<Release>> {
    let tx = c.transaction()?;
    let checked_before: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM artist_checks WHERE artist_id = ?1)", [artist_id], |r| r.get(0))?;
    let at = now();
    let mut fresh = vec![];
    for (pos, card) in a.albums.iter().chain(&a.singles).enumerate() {
        if !card.id.starts_with("MPREb") {
            continue;
        }
        let year = year_of(&card.subtitle);
        let first_seen = if checked_before { at } else { 0 };
        let added = tx.execute(
            "INSERT OR IGNORE INTO releases (id, artist_id, artist, title, subtitle, artwork, year, pos, first_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![card.id, artist_id, a.name, card.title, card.subtitle, card.artwork, year, pos as i64, first_seen],
        )?;
        if added == 0 {
            tx.execute(
                "UPDATE releases SET artist = ?2, title = ?3, subtitle = ?4, artwork = COALESCE(?5, artwork), year = ?6, pos = ?7
                 WHERE id = ?1",
                params![card.id, a.name, card.title, card.subtitle, card.artwork, year, pos as i64],
            )?;
        } else if checked_before {
            fresh.push(Release {
                id: card.id.clone(),
                title: card.title.clone(),
                subtitle: card.subtitle.clone(),
                artwork: card.artwork.clone(),
                artist: a.name.clone(),
                artist_id: artist_id.to_string(),
                year,
                first_seen,
                followed: false,
            });
        }
    }
    tx.execute(
        "INSERT INTO artist_checks (artist_id, checked_at) VALUES (?1, ?2)
         ON CONFLICT(artist_id) DO UPDATE SET checked_at = excluded.checked_at",
        params![artist_id, at],
    )?;
    // Keep the follow's name and picture current.
    tx.execute(
        "UPDATE follows SET name = ?2, artwork = COALESCE(?3, artwork) WHERE artist_id = ?1",
        params![artist_id, a.name, a.artwork],
    )?;
    tx.commit()?;
    Ok(fresh)
}

/// The first artist of "A, B & C feat. D".
fn first_artist(name: &str) -> &str {
    let mut end = name.len();
    for sep in [",", " & ", " feat", " ft.", " x ", " X ", " / "] {
        if let Some(i) = name.find(sep) {
            end = end.min(i);
        }
    }
    name[..end].trim()
}

/// Artists to check: everyone followed, then the most played (looked up by name when the
/// songs came without an artist id). `(id, followed)`.
async fn tracked(lib: &Library, http: &reqwest::Client) -> Vec<(String, bool)> {
    let since = now() - PLAYED_SECS;
    let (mut out, played) = match lib.with(|c| {
        let follows: Vec<String> = c
            .prepare("SELECT artist_id FROM follows ORDER BY followed_at")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let played: Vec<(Option<String>, String)> = c
            .prepare(
                "SELECT t.artist_id, t.artist FROM plays p JOIN tracks t ON t.url = p.url
                 WHERE p.played_at >= ?1 AND t.source <> 'soundcloud' AND t.artist <> ''
                 GROUP BY COALESCE(t.artist_id, LOWER(t.artist)) ORDER BY COUNT(*) DESC LIMIT 30",
            )?
            .query_map([since], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok((follows.into_iter().map(|id| (id, true)).collect::<Vec<_>>(), played))
    }) {
        Ok(v) => v,
        Err(e) => {
            klog!("releases: couldn't read the library: {e}");
            return vec![];
        }
    };
    let mut lookups = 0;
    let mut extra = 0;
    for (id, artist) in played {
        if extra >= TOP_PLAYED {
            break;
        }
        let id = match id {
            Some(id) => Some(id),
            None => {
                let name = first_artist(&artist).to_string();
                let cached: Option<(Option<String>, i64)> = lib
                    .with(|c| {
                        c.query_row("SELECT artist_id, looked_up_at FROM artist_names WHERE name = ?1", [name.to_lowercase()], |r| {
                            Ok((r.get(0)?, r.get(1)?))
                        })
                        .optional()
                    })
                    .ok()
                    .flatten();
                match cached {
                    Some((id, at)) if id.is_some() || now() - at < NAME_RETRY_SECS => id,
                    _ if lookups < LOOKUPS_PER_RUN && !name.is_empty() => {
                        lookups += 1;
                        match discover::find_artist_exact(http, &name).await {
                            Ok(id) => {
                                let _ = lib.with(|c| {
                                    c.execute(
                                        "INSERT INTO artist_names (name, artist_id, looked_up_at) VALUES (?1, ?2, ?3)
                                         ON CONFLICT(name) DO UPDATE SET artist_id = excluded.artist_id, looked_up_at = excluded.looked_up_at",
                                        params![name.to_lowercase(), id, now()],
                                    )
                                });
                                id
                            }
                            Err(_) => None,
                        }
                    }
                    _ => None,
                }
            }
        };
        if let Some(id) = id {
            if !out.iter().any(|(o, _)| *o == id) {
                out.push((id, false));
                extra += 1;
            }
        }
    }
    out
}

/// Checks artists that are due (or just `only`, right away), then tells the UI.
async fn check(app: &AppHandle, only: Option<String>) {
    let watcher = app.state::<Watcher>();
    let _running = watcher.busy.lock().await;
    let lib = app.state::<Arc<Library>>().inner().clone();
    let http = app.state::<AppState>().http.clone();
    let full = only.is_none();
    let artists = match only {
        Some(id) => vec![(id, true)],
        None => tracked(&lib, &http).await,
    };
    let mut fresh = vec![];
    let mut any = false;
    for (id, followed) in &artists {
        if full {
            let last: Option<i64> = lib
                .with(|c| c.query_row("SELECT checked_at FROM artist_checks WHERE artist_id = ?1", [id], |r| r.get(0)).optional())
                .ok()
                .flatten();
            if last.is_some_and(|t| now() - t < RECHECK_SECS) {
                continue;
            }
        }
        match discover::fetch_artist(&http, id).await {
            Ok(a) => match lib.with(|c| record(c, id, &a)) {
                Ok(new) => {
                    any = true;
                    if *followed {
                        fresh.extend(new.into_iter().map(|r| Release { followed: true, ..r }));
                    }
                }
                Err(e) => klog!("releases: couldn't save {id}: {e}"),
            },
            Err(e) => klog!("releases: {id}: {e}"),
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
    if full {
        // Forget artists that are neither followed nor played much any more.
        let keep: Vec<&str> = artists.iter().map(|(id, _)| id.as_str()).collect();
        let _ = lib.with(|c| prune(c, &keep));
    }
    if any {
        let _ = app.emit("library-changed", "releases");
    }
    if !fresh.is_empty() {
        klog!("releases: {} new from followed artists", fresh.len());
        let _ = app.emit("new-releases", &fresh);
    }
}

fn prune(c: &Connection, keep: &[&str]) -> rusqlite::Result<()> {
    let all: Vec<String> = c.prepare("SELECT artist_id FROM artist_checks")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for id in all.iter().filter(|id| !keep.contains(&id.as_str())) {
        let followed: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM follows WHERE artist_id = ?1)", [id], |r| r.get(0))?;
        if !followed {
            c.execute("DELETE FROM releases WHERE artist_id = ?1", [id])?;
            c.execute("DELETE FROM artist_checks WHERE artist_id = ?1", [id])?;
        }
    }
    Ok(())
}

/// Starts the release watcher: a first run shortly after start-up, then every half hour
/// (each artist is only read again once its check is 6 hours old).
pub fn start(app: &AppHandle) {
    app.manage(Watcher::default());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            check(&app, None).await;
            tokio::time::sleep(WAKE).await;
        }
    });
}

fn list_releases(c: &Connection, limit: i64) -> rusqlite::Result<Vec<Release>> {
    // What's new since Kite started watching first, then this year's and last year's releases,
    // each artist's newest first.
    let mut st = c.prepare(
        "SELECT r.id, r.title, r.subtitle, r.artwork, r.artist, r.artist_id, r.year, r.first_seen, f.artist_id IS NOT NULL
         FROM releases r LEFT JOIN follows f ON f.artist_id = r.artist_id
         WHERE r.first_seen > 0 OR COALESCE(r.year, 0) >= CAST(strftime('%Y', 'now') AS INTEGER) - 1
         ORDER BY r.first_seen DESC, COALESCE(r.year, 0) DESC, r.pos, r.artist
         LIMIT ?1",
    )?;
    let rows = st.query_map([limit], |r| {
        Ok(Release {
            id: r.get(0)?,
            title: r.get(1)?,
            subtitle: r.get(2)?,
            artwork: r.get(3)?,
            artist: r.get(4)?,
            artist_id: r.get(5)?,
            year: r.get(6)?,
            first_seen: r.get(7)?,
            followed: r.get(8)?,
        })
    })?;
    rows.collect()
}

#[tauri::command]
pub fn releases(lib: Lib<'_>, limit: Option<i64>) -> Result<Vec<Release>, String> {
    lib.with(|c| list_releases(c, limit.unwrap_or(24)))
}

#[tauri::command]
pub fn follows(lib: Lib<'_>) -> Result<Vec<Followed>, String> {
    lib.with(|c| {
        let mut st = c.prepare("SELECT artist_id, name, artwork FROM follows ORDER BY name COLLATE NOCASE")?;
        let rows = st.query_map([], |r| Ok(Followed { id: r.get(0)?, name: r.get(1)?, artwork: r.get(2)? }))?;
        rows.collect()
    })
}

#[tauri::command]
pub fn follow_set(app: AppHandle, lib: Lib<'_>, artist: Followed, on: bool) -> Result<(), String> {
    lib.with(|c| {
        if on {
            c.execute(
                "INSERT INTO follows (artist_id, name, artwork, followed_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(artist_id) DO UPDATE SET name = excluded.name, artwork = COALESCE(excluded.artwork, follows.artwork)",
                params![artist.id, artist.name, artist.artwork, now()],
            )
        } else {
            // Its releases go too; if it's one you play a lot, the next check brings them back.
            c.execute("DELETE FROM follows WHERE artist_id = ?1", [&artist.id])?;
            c.execute("DELETE FROM releases WHERE artist_id = ?1", [&artist.id])?;
            c.execute("DELETE FROM artist_checks WHERE artist_id = ?1", [&artist.id])
        }
    })?;
    let _ = app.emit("library-changed", "follows");
    let _ = app.emit("library-changed", "releases");
    if on {
        // Record what's out now, so only later releases count as new.
        let app = app.clone();
        tauri::async_runtime::spawn(async move { check(&app, Some(artist.id)).await });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Card;

    fn card(id: &str, title: &str, subtitle: &str) -> Card {
        Card { id: id.into(), title: title.into(), subtitle: subtitle.into(), artwork: None }
    }

    fn artist(albums: Vec<Card>, singles: Vec<Card>) -> Artist {
        Artist { name: "Someone".into(), albums, singles, ..Default::default() }
    }

    #[test]
    fn reads_years_and_first_artists() {
        assert_eq!(year_of("Single \u{2022} 2026"), Some(2026));
        assert_eq!(year_of("2019"), Some(2019));
        assert_eq!(year_of("EP"), None);
        assert_eq!(first_artist("Young Fly & Desh"), "Young Fly");
        assert_eq!(first_artist("A, B feat. C"), "A");
        assert_eq!(first_artist("Solo"), "Solo");
    }

    #[test]
    fn only_releases_after_the_first_check_are_new() {
        let dir = std::env::temp_dir().join(format!("kite-follow-{}", std::process::id()));
        let lib = Library::open(&dir).unwrap();
        let first = artist(vec![card("MPREb_old", "Old album", "2020")], vec![card("MPREb_s1", "Single one", "Single \u{2022} 2025")]);
        assert!(lib.with(|c| record(c, "UCx", &first)).unwrap().is_empty());
        // Seen again: nothing new.
        assert!(lib.with(|c| record(c, "UCx", &first)).unwrap().is_empty());
        let later = artist(
            vec![card("MPREb_old", "Old album", "2020")],
            vec![card("MPREb_s2", "Single two", "Single \u{2022} 2026"), card("MPREb_s1", "Single one", "Single \u{2022} 2025")],
        );
        let fresh = lib.with(|c| record(c, "UCx", &later)).unwrap();
        assert_eq!(fresh.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["MPREb_s2"]);
        // Home lists the new one first; the 2020 album is too old to show.
        let listed = lib.with(|c| list_releases(c, 10)).unwrap();
        assert_eq!(listed.first().map(|r| r.id.as_str()), Some("MPREb_s2"));
        assert!(!listed.iter().any(|r| r.id == "MPREb_old"));
        // Not followed and not played: forgotten.
        lib.with(|c| prune(c, &[])).unwrap();
        assert!(lib.with(|c| list_releases(c, 10)).unwrap().is_empty());
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
