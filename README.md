# Hum

A lightweight, open-source music player for YouTube and SoundCloud, for Windows and macOS.

No account, no ads, no telemetry. Built with Tauri, so it uses the webview your system already has instead of shipping its own browser.

## Features

- Search and play from YouTube and SoundCloud
- Queue with previous and next, seeking, volume
- Space to play or pause, `/` or Ctrl+F to search

Planned: local files, playlists, playlist import from Spotify, Deezer and YouTube Music, an equalizer, synced lyrics, Last.fm and Discord status.

## How it works

Hum uses [yt-dlp](https://github.com/yt-dlp/yt-dlp) to find streams. It downloads yt-dlp the first time it starts, checks it against the official checksums, and keeps it updated, so the installer stays small and site changes don't need a new Hum release.

Audio is passed through Hum's own `hum://` protocol inside the app. No local web server is opened.

For full YouTube support yt-dlp needs a JavaScript runtime. Install [Deno](https://deno.com), or Node.js, which Hum will use if it finds it.

## Building

You need Node.js 20+ and Rust.

```sh
npm install
npm run tauri dev     # run in development
npm run tauri build   # build an installer
```

macOS builds have to be made on a Mac, or with the GitHub Actions workflow in `.github/workflows/build.yml`.

## License

MIT
