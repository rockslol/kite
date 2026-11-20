#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod media;
mod stream;
mod titles;
mod ytdlp;

use std::sync::Arc;
use std::time::Duration;
use stream::Streams;
use tauri::{Manager, State};
use ytdlp::{ToolStatus, Track, YtDlp};

struct AppState {
    yt: Arc<YtDlp>,
    streams: Arc<Streams>,
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
        .register_asynchronous_uri_scheme_protocol("hum", |ctx, request, responder| {
            let Some(state) = ctx.app_handle().try_state::<AppState>() else {
                responder.respond(
                    tauri::http::Response::builder().status(503).body(Vec::new()).unwrap(),
                );
                return;
            };
            let streams = state.streams.clone();
            tauri::async_runtime::spawn(async move {
                responder.respond(streams.handle(request).await);
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
            app.manage(AppState { yt: yt.clone(), streams });

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
            media::media_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hum");
}
