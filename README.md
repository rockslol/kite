# Kite

A lightweight, open-source music player for YouTube Music, YouTube, SoundCloud and your own files, for Windows and macOS.

No account, no ads, no telemetry. Built with Tauri, so it uses the webview your system already has instead of shipping its own browser. The installer is about 3 MB.

## Features

- Search YouTube Music, YouTube and SoundCloud, or paste a link to play it
- Import playlists from Spotify, Deezer, YouTube Music, YouTube and SoundCloud, and keep them in sync
- Liked songs, playlists, recently played and most played
- Local files: MP3, FLAC, M4A, OGG, Opus and WAV, with tags and cover art
- Queue with drag to reorder, shuffle and repeat
- Works with your media keys and the system media controls
- Keeps playing in the tray when the window is closed

## How it works

Kite uses [yt-dlp](https://github.com/yt-dlp/yt-dlp) to find streams. It downloads yt-dlp the first time it starts, checks it against the official checksums, and keeps it updated, so the installer stays small and site changes don't need a new Kite release.

YouTube needs a JavaScript runtime. Kite uses Deno or Node.js if you have one, and otherwise downloads [QuickJS](https://github.com/quickjs-ng/quickjs) (about 2 MB, checked against a fixed checksum).

Spotify and Deezer songs are matched to the same song on YouTube Music. Kite never streams from Spotify or Deezer.

Audio goes through Kite's own `kite://` protocol inside the app. No local web server is opened.

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
