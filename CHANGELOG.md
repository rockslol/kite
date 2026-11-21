# Changelog

What changed in each version of Kite. The newest version is at the top.

## 0.2.0 (2026-11-21)

### Play

- **Songs start faster.** The top search results are made ready while you read them, and a song you rest the mouse on starts loading. A top result starts in about 2 seconds (was about 5).
- **No gaps between songs.** Kite loads the next song before the current one ends, and **Next** is instant.
- **Songs that can't play are skipped** with a short message, instead of stopping the queue.
- **Play all** and **Shuffle** for search results.
- **Songs in playlists start faster.** Opening a playlist, liked songs or history prepares the first songs (and the one Shuffle will start with), hovering a song prepares it, the next two songs in the queue are always ready, and songs Kite looked up in the last few hours start right away, even after a restart.
- **Fewer slow starts.** About one YouTube link in ten never starts working. Kite now checks each link before playing it and gets a new one right away, instead of waiting it out.
- **Resuming after a long pause just works.** A song paused for more than 20 minutes (or one whose stream dropped mid-song) picks up where it was, instead of failing or skipping.

### Library

- **Select several songs** with Ctrl+click and Shift+click (Ctrl+A selects the whole list, Esc clears it). Right-click the selection to play it, queue it, like it or add it to a playlist, or drag it onto a playlist in the sidebar.
- **Undo**: deleting a playlist, removing songs from a playlist and clearing the queue now show an **Undo** button for a few seconds. Deleting a playlist no longer asks you to confirm first.
- **Music folders are watched** while Kite runs: songs you add, rename, move or delete show up in Local files within a few seconds, no restart or Rescan needed.
- **Back up or move your library** (Settings → Library): export liked songs, playlists, history and music folders to one file, and import it on another PC. Importing adds what's missing and keeps what you have, so importing twice is safe.

### macOS

- **Kite now runs on macOS 11 and later**, Apple Silicon and Intel, in one download. It isn't notarized by Apple yet, so the first time, right-click Kite in Applications and choose **Open**.

### Help

- **Copy debug info** (Settings → About): your Kite and yt-dlp versions, library size and recent problems, ready to paste into a bug report. Kite keeps a small log file for this. Nothing is sent anywhere.

## 0.1.0 (2026-11-20)

The first release: a small, open-source music player for YouTube Music, YouTube, SoundCloud and your own files. No account, no ads, no telemetry.

### Play

- **Search YouTube Music, YouTube and SoundCloud** from one search box, or paste a link to a song and it starts playing.
- **Queue**: play next, add to queue, drag to reorder, shuffle and repeat. Kite remembers your queue and where you were when you close it.
- **Media keys and Windows media controls**: the keyboard keys, the volume flyout and apps that show what's playing all work with Kite.
- **Keeps playing in the tray** when you close the window. Quit from the tray icon.

### Library

- **Liked songs**, **playlists** (rename, reorder, drag songs onto them) and **Recently played / Most played**.
- **Local files**: add a music folder and Kite reads titles, artists, albums and covers from MP3, FLAC, M4A, OGG, Opus and WAV files.

### Import

- **Bring your playlists** from Spotify, Deezer, YouTube Music, YouTube and SoundCloud: paste the link, and Spotify and Deezer songs are matched to the same song on YouTube Music. **Sync** adds songs that are new at the source.
- **Wrong song?** Right-click it in a playlist and pick another version. Kite remembers your choice.
- Spotify only shares the first 100 songs of a playlist with apps that aren't logged in, so bigger Spotify playlists come in cut off for now.

### Small and light

- About a 3 MB installer. About 160 MB of memory while playing with the window open, 133 MB in the tray, and almost no CPU.
- Kite downloads yt-dlp (and, if your PC has no Node.js or Deno, QuickJS) the first time it starts, checks them against fixed checksums, and keeps yt-dlp updated by itself.
