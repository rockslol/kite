// DJ-style crossfade: instead of always fading at a fixed time before the end, wait for the
// song's ending to go quiet (a fade-out, a soft outro, applause dying down) and start the next
// song there. Songs that stay loud to the last second get the plain crossfade.

/** How far before the end Kite starts listening for a quiet ending (seconds). The next song is
 *  loaded 25 s before the end, so this stays inside that. */
export const LISTEN_FROM = 22;
/** How much quieter than the song's own average counts as "quiet" (dB). */
export const QUIET_DROP = 12;
/** How long it has to stay quiet before the fade starts (seconds), so a short break or a
 *  drum fill doesn't trigger it. */
export const QUIET_HOLD = 1.5;

/**
 * Decides, a few times a second, whether to start fading into the next song.
 *
 * @param {object} p
 * @param {number} p.left        seconds left in the song
 * @param {number} p.crossfade   the crossfade setting (seconds)
 * @param {number|null} p.now    loudness of the last second or so (dB), null if unknown
 * @param {number|null} p.avg    the song's average loudness so far (dB), null if not enough heard
 * @param {number} p.quietFor    seconds the song has already been quiet in a row
 * @returns {"wait" | "quiet" | "plain"} start now because it's quiet, start now because time's
 *          up (the plain crossfade), or keep waiting
 */
export function decide({ left, crossfade, now, avg, quietFor }) {
  if (!(crossfade > 0) || !Number.isFinite(left) || left <= 0.5) return "wait";
  if (left <= crossfade) return "plain";
  if (left > LISTEN_FROM || now == null || avg == null) return "wait";
  return isQuiet(now, avg) && quietFor >= QUIET_HOLD ? "quiet" : "wait";
}

export function isQuiet(now, avg) {
  return now != null && avg != null && now <= avg - QUIET_DROP;
}
