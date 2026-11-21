//! A small local log file, for bug reports. Nothing is sent anywhere: the user copies it from
//! Settings → "Copy debug info" when they want to.
//!
//! Lines go to `<app log dir>/kite.log`. When it passes `MAX_BYTES` it's moved to
//! `kite.old.log`, so the two together stay under about a megabyte.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 512 * 1024;

static FILE: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

/// Writes a line to the terminal and the log file: `klog!("couldn't play {url}: {e}")`.
#[macro_export]
macro_rules! klog {
    ($($t:tt)*) => { $crate::log::write(&format!($($t)*)) };
}

pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = FILE.set(dir.join("kite.log"));
}

pub fn write(msg: &str) {
    eprintln!("[kite] {msg}");
    let Some(path) = FILE.get() else { return };
    let _guard = LOCK.lock();
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_file_name("kite.old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {}", timestamp(crate::library::now()), msg.replace('\n', " "));
    }
}

/// The last `n` lines of the log.
pub fn tail(n: usize) -> Vec<String> {
    let Some(path) = FILE.get() else { return vec![] };
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
}

/// `2026-11-21 14:03:09` (UTC) from unix seconds, without a date library.
fn timestamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::timestamp;

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(timestamp(1_795_305_789), "2026-11-22 00:03:09");
        assert_eq!(timestamp(951_782_400), "2000-02-29 00:00:00");
    }
}
