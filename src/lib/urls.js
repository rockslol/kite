// URLs into the backend's hum:// protocol.

const BASE = navigator.userAgent.includes("Windows") ? "http://hum.localhost" : "hum://localhost";

/** Where the audio element loads a track from. */
export function streamUrl(track) {
  const route = track.source === "local" ? "file" : "play";
  return `${BASE}/${route}?u=${encodeURIComponent(track.url)}`;
}

/**
 * Artwork can be `art:<file>` (from a local file's tags) or a web URL. Web images go through
 * the backend, which caches them and avoids WebView2 blocking some image hosts.
 */
export function artSrc(artwork) {
  if (!artwork) return null;
  if (artwork.startsWith("art:")) return `${BASE}/art/${artwork.slice(4)}`;
  if (artwork.startsWith("https://")) return `${BASE}/img?u=${encodeURIComponent(artwork)}`;
  return artwork;
}

export function fmtTime(s) {
  if (!Number.isFinite(s) || s <= 0) return "0:00";
  s = Math.floor(s);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** "1 h 12 min" style total for list headers. */
export function fmtTotal(tracks) {
  const s = tracks.reduce((sum, t) => sum + (t.duration || 0), 0);
  const h = Math.floor(s / 3600);
  const m = Math.round((s % 3600) / 60);
  return h ? `${h} h ${m} min` : `${m} min`;
}

export function plural(n, word) {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}
