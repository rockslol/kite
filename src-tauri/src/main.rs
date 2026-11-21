#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod backup;
mod import;
mod hotkeys;
mod discord;
mod library;
mod log;
mod lyrics;
mod local;
mod media;
mod scrobble;
mod stream;
mod titles;
mod ytdlp;
mod watch;
mod ytmusic;

use library::Library;
use std::sync::Arc;
use std::time::Duration;
use stream::Streams;
use tauri::{Manager, State};
use ytdlp::{ToolStatus, Track, YtDlp};

pub struct AppState {
    pub yt: Arc<YtDlp>,
    pub streams: Arc<Streams>,
    pub library: Arc<Library>,
    pub http: reqwest::Client,
}

#[tauri::command]
fn tool_status(state: State<'_, AppState>) -> ToolStatus {
    state.yt.status()
}

#[tauri::command]
async fn search(state: State<'_, AppState>, source: String, query: String) -> Result<Vec<Track>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(vec![]);
    }
    if source == "ytmusic" {
        match ytmusic::search_songs(&state.http, query).await {
            Ok(found) if !found.is_empty() => return Ok(found),
            Ok(_) => return Ok(vec![]),
            // YouTube Music changed something: fall back to plain YouTube search.
            Err(e) => klog!("YouTube Music search failed, using YouTube: {e}"),
        }
        return state.yt.search("youtube", query).await;
    }
    state.yt.search(&source, query).await
}

/// Resolves a track ahead of playback so errors reach the UI as text, and the next track in the
/// queue can be warmed up early.
#[tauri::command]
async fn prepare(state: State<'_, AppState>, url: String) -> Result<(), String> {
    state.streams.prepare(&url).await
}

/// Debug builds only: invalidates cached stream links, to test recovery from expired ones.
#[tauri::command]
fn debug_break_streams(state: State<'_, AppState>) -> usize {
    #[cfg(debug_assertions)]
    return state.streams.break_links();
    #[cfg(not(debug_assertions))]
    {
        let _ = state;
        0
    }
}

fn main() {
    tauri::Builder::default()
        // Opening Kite again shows the running copy instead of starting a second one.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| app::show_main(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(hotkeys::plugin())
        .register_asynchronous_uri_scheme_protocol("kite", |ctx, request, responder| {
            let Some(state) = ctx.app_handle().try_state::<AppState>() else {
                responder.respond(
                    tauri::http::Response::builder().status(503).body(Vec::new()).unwrap(),
                );
                return;
            };
            let streams = state.streams.clone();
            let library = state.library.clone();
            let http = state.http.clone();
            tauri::async_runtime::spawn(async move {
                let path = request.uri().path();
                let resp = if path.starts_with("/file") || path.starts_with("/art/") || path == "/img" {
                    library.serve(&request, &http).await
                } else {
                    streams.handle(request).await
                };
                responder.respond(resp);
            });
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray = window.app_handle().try_state::<app::Prefs>().is_some_and(|p| p.get().close_to_tray);
                if close_to_tray {
                    api.prevent_close();
                    if let Some(w) = window.app_handle().get_webview_window(window.label()) {
                        app::set_shown(&w, false);
                    }
                }
            }
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            // The app was called Hum before 0.2: carry its library, settings and tools over.
            let old_dir = data_dir.with_file_name("app.hum.player");
            if !data_dir.exists() && old_dir.is_dir() {
                let _ = std::fs::rename(&old_dir, &data_dir);
            }
            std::fs::create_dir_all(&data_dir)?;
            log::init(app.path().app_log_dir().unwrap_or_else(|_| data_dir.join("logs")));
            klog!("Kite {} started", app.package_info().version);
            let prefs = app::Prefs::load(&data_dir);
            let check_updates = prefs.get().check_updates;
            app.manage(prefs);
            app.manage(hotkeys::Registered::default());
            app.manage(scrobble::Scrobbler::load(&data_dir));
            app.manage(discord::Discord::start(app.state::<app::Prefs>().get().discord));
            for (action, problem) in hotkeys::apply(app.handle()) {
                klog!("shortcut for {action}: {problem}");
            }
            app.manage(app::CacheDirs(vec![cache_dir.join("audio"), data_dir.join("artwork").join("web")]));
            app::build_tray(app)?;
            // Started with Windows: stay in the tray.
            if std::env::args().any(|a| a == "--autostart") {
                if let Some(w) = app.get_webview_window("main") {
                    app::set_shown(&w, false);
                }
            }
            if check_updates && !cfg!(debug_assertions) {
                app::check_updates_later(app.handle().clone());
            }
            let http = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(120))
                .build()?;

            let yt = Arc::new(YtDlp::new(&data_dir.join("bin"), &cache_dir));
            let streams = Arc::new(Streams::new(yt.clone(), http.clone(), cache_dir));
            let library = Arc::new(Library::open(&data_dir)?);
            app.manage(library.clone());
            app.manage(AppState { yt: yt.clone(), streams, library: library.clone(), http: http.clone() });
            // Pick up files added or changed while the app was closed, then watch for more.
            watch::start(app.handle(), library.clone());
            local::rescan(app.handle().clone(), library);

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { yt.ensure(handle, http).await });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tool_status,
            debug_break_streams,
            app::debug_info,
            app::log_line,
            lyrics::lyrics,
            lyrics::lyrics_offset,
            hotkeys::hotkeys_apply,
            hotkeys::hotkeys_suspend,
            scrobble::scrobble_status,
            scrobble::scrobble_now_playing,
            scrobble::scrobble,
            scrobble::lastfm_connect,
            scrobble::lastfm_disconnect,
            scrobble::listenbrainz_connect,
            scrobble::listenbrainz_disconnect,
            search,
            prepare,
            app::tray_tooltip,
            app::settings_get,
            app::settings_set,
            app::autostart_set,
            app::tools_info,
            app::tools_update,
            app::cache_size,
            app::cache_clear,
            app::update_check,
            app::update_install,
            media::media_track,
            media::media_state,
            library::lib_like,
            library::lib_liked,
            library::lib_liked_urls,
            library::lib_playlists,
            library::lib_playlist_create,
            library::lib_playlist_rename,
            library::lib_playlist_delete,
            library::lib_playlist_restore,
            library::lib_playlist_items,
            library::lib_playlist_add,
            library::lib_playlist_remove,
            library::lib_playlist_unremove,
            library::lib_playlist_move,
            library::lib_played,
            library::lib_history,
            library::lib_clear_history,
            library::lib_search,
            library::lib_local_tracks,
            library::lib_folders,
            library::lib_add_folder,
            library::lib_remove_folder,
            library::lib_rescan,
            library::lib_scanning,
            backup::lib_export,
            backup::lib_import,
            import::import_link,
            import::open_link,
            import::match_alternatives,
            import::match_choose
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kite");
}
