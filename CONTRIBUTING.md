# Contributing to Kite

Thanks for wanting to help. Bug reports, ideas and pull requests are all welcome.

## Before you start

- **Small fixes** (a bug, a typo, a rough edge): open a pull request straight away.
- **Bigger changes** (a new feature, a new source, a change to how something works): open an issue first so we can agree on the shape before you spend time on it.
- Check the [open issues](https://github.com/rockslol/kite/issues) to see if someone is already on it.

## What Kite is (and isn't)

Kite keeps a few promises. Pull requests that break them won't be merged, however good the code is.

- **Small and light.** About a 3 MB installer, low memory and almost no CPU when paused. Think twice before adding a dependency, and say in the pull request what it costs in size.
- **Free sources only.** Kite plays from YouTube Music, YouTube, SoundCloud and local files. Paid services (Spotify, Deezer, Apple Music, Tidal) are import-only: Kite reads playlists from them and never streams from them.
- **No stream extraction code in this repository.** Finding streams is yt-dlp's job. Kite downloads yt-dlp on first run and keeps it updated.
- **No telemetry, no accounts, no ads.**

## Setting up

You need Node.js 20+, Rust (stable), and on Windows the Visual Studio C++ Build Tools.

```bash
git clone https://github.com/rockslol/kite.git
cd kite
npm install
npm run tauri dev
```

The first start downloads yt-dlp into the app's data folder, so it takes a little longer.

## Where things are

| Folder | What's in it |
|---|---|
| `src/` | The interface (Svelte 5). `lib/player.svelte.js` is playback and the queue, `lib/library.svelte.js` the library, `views/` the pages. |
| `src-tauri/src/` | The backend (Rust). `ytdlp.rs` runs yt-dlp, `stream.rs` serves audio to the player, `library.rs` is the SQLite library, `import.rs` and `ytmusic.rs` import playlists and search YouTube Music, `local.rs` scans music folders. |

## Before you open a pull request

- `npm run build` finishes with no errors or Svelte warnings.
- `cargo test` passes in `src-tauri/`, and new logic that can be tested without the network has a test next to it.
- `cargo build` in `src-tauri/` has no warnings.
- You tried the change in the running app (`npm run tauri dev`), including the case where it fails (no internet, a song that can't play, an empty list).
- The pull request says what changed and why, and has a screenshot if the interface changed.

## Style

- Follow the code around you. Short functions, clear names, a comment where the *why* isn't obvious.
- Text in the app is plain and short: "Couldn't play this song", not "An error occurred while attempting playback".
- Keep pull requests to one change. Two unrelated fixes are easier to review as two pull requests.

## Reporting bugs

Use the bug report form in [Issues](https://github.com/rockslol/kite/issues/new/choose). The most useful things to include: your Kite version (Settings), Windows or macOS version, the song or link that failed, and what you expected to happen.

By contributing you agree that your work is released under the [MIT License](./LICENSE).
