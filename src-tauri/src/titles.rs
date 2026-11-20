//! Turns YouTube video titles into song titles: "Artist - Song (Official Video) [HD]" becomes
//! title "Song", artist "Artist". Brackets that mean something ("(Remix)", "(Live)",
//! "(feat. X)") are kept.

const NOISE: &[&str] = &[
    "official", "officiel", "oficial", "music", "video", "videoclip", "clip", "audio", "lyric",
    "lyrics", "with", "visualizer", "visualiser", "hd", "hq", "4k", "1080p", "720p", "mv", "m/v",
    "explicit", "version", "full", "the", "and", "&", "-", "/",
];

const OPEN: &[char] = &['(', '[', '【'];
const CLOSE: &[char] = &[')', ']', '】'];

/// Returns (title, artist) for a YouTube result.
pub fn clean_youtube(title: &str, channel: &str) -> (String, String) {
    let topic = channel.ends_with(" - Topic");
    let channel = clean_channel(channel);
    let title = strip_noise_groups(title);
    let title = strip_noise_segments(&title);

    // "- Topic" channels are auto-generated from label metadata: the title is already the song.
    if !topic {
        for dash in [" - ", " – ", " — "] {
            if let Some((left, right)) = title.split_once(dash) {
                let (left, right) = (left.trim(), right.trim());
                if !left.is_empty() && !right.is_empty() && left.chars().count() <= 60 {
                    return (tidy(right), tidy(left));
                }
            }
        }
    }
    (tidy(&title), channel)
}

fn clean_channel(channel: &str) -> String {
    let c = channel.trim_end_matches(" - Topic").trim();
    let c = c.strip_suffix("VEVO").unwrap_or(c).trim();
    let c = c.strip_suffix("Official").unwrap_or(c).trim();
    c.to_string()
}

fn is_noise(content: &str) -> bool {
    let lower = content.to_lowercase();
    lower
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '.' || ch == '|')
        .filter(|w| !w.is_empty())
        .all(|w| NOISE.contains(&w))
}

fn strip_noise_groups(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(OPEN) {
        let open_len = rest[start..].chars().next().map_or(1, char::len_utf8);
        let after = &rest[start + open_len..];
        let Some(end) = after.find(CLOSE) else { break };
        let close_len = after[end..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&rest[..start]);
        if !is_noise(&after[..end]) {
            out.push_str(&rest[start..start + open_len + end + close_len]);
        }
        rest = &after[end + close_len..];
    }
    out.push_str(rest);
    out
}

/// Drops trailing " | Official Video" style segments.
fn strip_noise_segments(s: &str) -> String {
    let mut parts = s.split(" | ");
    let first = parts.next().unwrap_or("").to_string();
    parts.fold(first, |mut acc, p| {
        if !is_noise(p) {
            acc.push_str(" | ");
            acc.push_str(p);
        }
        acc
    })
}

fn tidy(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.trim_matches(|c: char| c == '-' || c == '|' || c == ',' || c.is_whitespace()).to_string()
}

#[cfg(test)]
mod tests {
    use super::clean_youtube as c;

    fn t(title: &str, channel: &str) -> (String, String) {
        c(title, channel)
    }

    #[test]
    fn splits_artist_and_strips_noise() {
        assert_eq!(t("Daft Punk - Get Lucky (Official Video)", "DaftPunkVEVO"), ("Get Lucky".into(), "Daft Punk".into()));
        assert_eq!(t("Che - BA$$ [Official Music Video] (HD)", "Che"), ("BA$$".into(), "Che".into()));
        assert_eq!(t("Song Name (Lyrics)", "Some Lyrics Channel"), ("Song Name".into(), "Some Lyrics Channel".into()));
        assert_eq!(t("Artist – Track | Official Audio", "Label"), ("Track".into(), "Artist".into()));
        assert_eq!(t("Artist - Track (Official 4K Video) 【MV】", "x"), ("Track".into(), "Artist".into()));
    }

    #[test]
    fn keeps_meaningful_brackets() {
        assert_eq!(t("Artist - Track (Remix)", "x").0, "Track (Remix)");
        assert_eq!(t("Artist - Track (feat. Other) [Official Video]", "x").0, "Track (feat. Other)");
        assert_eq!(t("Artist - Track (Live at Wembley)", "x").0, "Track (Live at Wembley)");
        assert_eq!(t("Artist - Track (Official Remix Video)", "x").0, "Track (Official Remix Video)");
    }

    #[test]
    fn topic_channels_are_left_alone() {
        assert_eq!(t("Track - Remastered 2011", "The Beatles - Topic"), ("Track - Remastered 2011".into(), "The Beatles".into()));
    }

    #[test]
    fn no_dash_uses_channel() {
        assert_eq!(t("Get Lucky", "Daft Punk"), ("Get Lucky".into(), "Daft Punk".into()));
    }
}
