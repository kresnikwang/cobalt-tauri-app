// Playlist / multi-part detection for the yt-dlp engine.
//
// The download path used to hardcode `--no-playlist`, so a YouTube playlist, a
// Bilibili collection or a multi-part video silently downloaded only its first
// entry. yt-dlp can report the whole collection up front: the entries are
// probed with `--flat-playlist` (one cheap request, no per-video resolution),
// the UI asks which episodes to keep, and every kept entry becomes its own task
// through `--playlist-items`.

use serde::{Deserialize, Serialize};

/// Upper bound of entries handed to the UI. A 2000-episode anime would freeze
/// the picker, and "Download all" covers the rest.
pub const MAX_ENTRIES: usize = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    /// 1-based position inside the playlist (what `--playlist-items` expects).
    pub index: u32,
    pub id: String,
    pub title: String,
    /// Human readable duration (`12:34`) when the extractor provides it.
    pub duration: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistProbe {
    /// True when the URL expands to more than the single media item we were
    /// asked for (playlist, channel, collection, multi-part video).
    pub is_playlist: bool,
    /// Total number of entries yt-dlp reports, even beyond `entries`.
    pub count: u32,
    pub title: String,
    pub uploader: String,
    pub extractor: String,
    pub entries: Vec<PlaylistEntry>,
}

fn string_field(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// First non-empty of several candidate keys.
fn first_field(payload: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| string_field(payload, key))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

/// Parse a `--flat-playlist --dump-single-json` payload.
///
/// A plain video yields no `entries`, which is reported as a non-playlist probe
/// so the caller can start the download immediately.
pub fn parse_probe(payload: &serde_json::Value) -> PlaylistProbe {
    let raw_entries = payload.get("entries").and_then(|value| value.as_array());
    let declared = payload
        .get("playlist_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    let mut entries: Vec<PlaylistEntry> = Vec::new();
    if let Some(raw_entries) = raw_entries {
        for (position, entry) in raw_entries.iter().enumerate() {
            if entries.len() >= MAX_ENTRIES {
                break;
            }
            let index = entry
                .get("playlist_index")
                .and_then(|value| value.as_u64())
                .unwrap_or(position as u64 + 1);
            entries.push(PlaylistEntry {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                id: string_field(entry, "id"),
                title: string_field(entry, "title"),
                duration: entry
                    .get("duration_string")
                    .and_then(|value| value.as_str())
                    .or_else(|| entry.get("duration").and_then(|value| value.as_str()))
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
            });
        }
    }

    let count = if declared > 0 {
        u32::try_from(declared).unwrap_or(u32::MAX)
    } else {
        entries.len() as u32
    };
    // yt-dlp reports `_type: "playlist"` for multi-item results; a single video
    // keeps `_type: "video"` and no entries.
    let is_playlist = payload.get("_type").and_then(|value| value.as_str()) == Some("playlist")
        || entries.len() > 1
        || (declared > 1 && entries.len() == 1);
    let is_playlist = is_playlist && count > 1;

    PlaylistProbe {
        is_playlist,
        count,
        title: string_field(payload, "title"),
        uploader: first_field(payload, &["uploader", "channel", "playlist_uploader"]),
        extractor: first_field(payload, &["extractor_key", "extractor"]),
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_video_is_not_a_playlist() {
        let payload = serde_json::json!({
            "_type": "video",
            "id": "aqz-KE-bpKQ",
            "title": "Big Buck Bunny",
            "extractor": "youtube"
        });
        let probe = parse_probe(&payload);
        assert!(!probe.is_playlist);
        assert_eq!(probe.count, 0);
        assert!(probe.entries.is_empty());
    }

    #[test]
    fn flat_playlist_is_reported_with_entries() {
        let payload = serde_json::json!({
            "_type": "playlist",
            "id": "PL123",
            "title": "Cobalt soundtrack",
            "playlist_count": 4,
            "extractor": "youtube",
            "entries": [
                { "id": "a1", "title": "Intro", "playlist_index": 1, "duration_string": "1:02" },
                { "id": "a2", "title": "Verse", "playlist_index": 2, "duration_string": "2:30" }
            ]
        });
        let probe = parse_probe(&payload);
        assert!(probe.is_playlist);
        assert_eq!(probe.count, 4);
        assert_eq!(probe.entries.len(), 2);
        assert_eq!(probe.entries[0].index, 1);
        assert_eq!(probe.entries[0].title, "Intro");
        assert_eq!(probe.entries[0].duration.as_deref(), Some("1:02"));
        assert_eq!(probe.title, "Cobalt soundtrack");
    }

    #[test]
    fn multi_part_videos_are_playlists_too() {
        // Bilibili multi-part videos arrive as `_type: "video"` with a
        // playlist_count, and only one entry in a flat dump.
        let payload = serde_json::json!({
            "_type": "playlist",
            "id": "BV1xx411c7mD",
            "title": "Episode collection",
            "playlist_count": 12,
            "entries": [
                { "id": "BV1xx411c7mD", "title": "Part 1", "playlist_index": 1 }
            ]
        });
        let probe = parse_probe(&payload);
        assert!(probe.is_playlist);
        assert_eq!(probe.count, 12);
        assert_eq!(probe.entries.len(), 1);
    }
}
