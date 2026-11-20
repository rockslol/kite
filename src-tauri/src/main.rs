#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod import;
mod library;
mod local;
mod media;
mod stream;
mod titles;
mod ytdlp;
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
            Err(e) => eprintln!("[hum] YouTube Music search failed, using YouTube: {e}"),
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

#[tauri::command]
async fn diagnose(state: State<'_, AppState>, url: String) -> Result<String, String> {
    Ok(state.streams.diagnose(&url).await)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("hum", |ctx, request, responder| {
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
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let http = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(120))
                .build()?;

            let yt = Arc::new(YtDlp::new(&data_dir.join("bin"), &cache_dir));
            let streams = Arc::new(Streams::new(yt.clone(), http.clone(), cache_dir));
            let library = Arc::new(Library::open(&data_dir)?);
            app.manage(library.clone());
            app.manage(AppState { yt: yt.clone(), streams, library: library.clone(), http: http.clone() });
            // Pick up files added or changed while the app was closed.
            local::rescan(app.handle().clone(), library);

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { yt.ensure(handle, http).await });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tool_status,
            search,
            prepare,
            diagnose,
            media::media_track,
            media::media_state,
            library::lib_like,
            library::lib_liked,
            library::lib_liked_urls,
            library::lib_playlists,
            library::lib_playlist_create,
            library::lib_playlist_rename,
            library::lib_playlist_delete,
            library::lib_playlist_items,
            library::lib_playlist_add,
            library::lib_playlist_remove,
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
            import::import_link,
            import::open_link,
            import::match_alternatives,
            import::match_choose
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hum");
}
