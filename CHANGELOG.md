# Changelog

What changed in each version of Kite. The newest version is at the top.

## 0.5.1 (2026-11-23)

### Queue

- **How long the queue still plays**, at the top of the queue: "23 songs · 1 h 12 min left".
- **Remove duplicates** in the queue: takes out songs that are already playing or already up next. Undo puts them back where they were.
- **Played**: the songs you played before the current one stay in the queue, above it. Double-click one to play it again. The last three show, the rest behind "Show all".
- **Undo after Play next and Add to queue**: a short message says what was added, with Undo.

## 0.5.0 (2026-11-23)

### Discover

- **Artist pages**: top songs (and all of them), albums, singles and EPs, artists fans also like, and a short bio. Click an artist's name anywhere to open it.
- **Album pages**: every song, play or shuffle the album, or save it as a playlist. Click an album's name to open it.
- **Radio**: start an endless radio from any song (right-click → Start radio) or from an artist's page. Kite keeps adding songs as you go.
- **Back** button (also Alt+← and the mouse's back button) for moving between pages.
- **Home**: songs you played lately, a mix for each artist you play most, new albums and singles from the artists you follow and play, and the charts for any of 70 countries (top songs, chart playlists, top artists).
- **Follow artists** from their page. Kite checks for new releases every few hours and tells you when one comes out.
- Chart playlists open like albums: play, shuffle or save them.
- Library backups now include the artists you follow.
- **Search suggestions** while you type, with the top matching artist, album or song on the YouTube Music tab. Use the arrow keys and Enter to pick one. With the field empty, it shows your recent searches.
- **Find in a playlist** or in Liked songs: a search field above long lists. Accents don't matter, so "ujra" finds "újra".
- **Because you liked …** on Home: songs like one you liked (or, before you like anything, one you play a lot), a different one each day.
- **Song info** (right-click a song): length, when it came out, YouTube views, how often you played it, when you liked it, and the playlists it's in. Open it on YouTube or copy the link.

## 0.4.1 (2026-11-23)

### Fixes

- **Switching Data saver while a song played could stop that song** partway with "This song couldn't be played", and Kite skipped to the next one. A song now keeps the quality it started with, and the setting applies from the next song.

## 0.4.0 (2026-11-23)

### Sound

- **Sound panel** (the sliders button, or **E**): one-click effects (Slowed, Slowed + reverb, Nightcore, Sped up, 8D), speed with or without keeping the pitch, reverb, and a 10-band equalizer with presets.
- **Balance and mono**, for one-sided hearing or a single earbud.
- **Sleep timer**: stop after 15 minutes, 30 minutes, an hour, or at the end of the song, fading out gently.
- **Crossfade** (0-12 seconds): the next song fades in while this one fades out. **Wait for a quiet ending** makes it work like a DJ: when a song fades out or winds down early, the next one starts there instead of at a fixed time.
- **Even out volume**: quiet and loud songs play at a similar level. Kite measures each song as it plays and remembers it, so it starts at the right level next time.
- **Data saver** (Settings → Streaming): smaller streams, about half the data, for when you're on a hotspot.
- **Play on** (top of the Sound panel, Windows): pick the speakers or headphones Kite plays on, or follow Windows. If the picked device is unplugged, Kite plays where Windows does until it's back.
- **Volume and equalizer per device**: headphones and speakers each keep their own volume and equalizer. When the output changes, Kite switches to that device's settings and says where it's playing.
- Kite only switches its sound engine on once you use one of these, so plain listening is as light as before.

## 0.3.0 (2026-11-22)

### Lyrics

- **Synced lyrics** beside the song list (the microphone button, or **Y**): the line being sung is highlighted and kept in the middle, and clicking a line jumps there. Lyrics come from [LRCLIB](https://lrclib.net), free and without an account, or from a `.lrc` or `.txt` file next to a local song.
- **Early or late?** Nudge the lyrics by half a second at a time; Kite remembers it for that song.

### Sharing

- **Discord status** (Settings → Sharing, off until you switch it on): "Listening to Kite" with the song, artist, cover, a progress bar and a button to listen. Clears when you pause.
- **Scrobble to Last.fm and ListenBrainz** with your own account: "now playing" when a song starts, and a scrobble once you've heard half of it (or 4 minutes). Scrobbles made offline are sent later.

### Control

- **Shortcuts that work everywhere** (Settings → Keyboard shortcuts): play or pause, next, previous, like and show Kite while you're in another app. Off until you switch them on, and every key can be changed.

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
