//! Routing for the bundled **gallery-dl** sidecar (image / gallery engine).
//!
//! This module decides which URLs gallery-dl owns. It is deliberately pure
//! (no process spawning here) so the decision can be unit-tested exhaustively.
//! The executor (resolve binary, `-j` enumerate, `-d` download) lives below in
//! this same module and is wired into [`crate::run_download_task`].
//!
//! # Priority matrix
//!
//! | URL shape | Primary | Fallback |
//! |---|---|---|
//! | YouTube / Bilibili / Dailymotion | yt-dlp (unchanged) | — |
//! | Image-host direct item, gallery, artist page, comic | **gallery-dl** | suggest sniffer on failure |
//! | IG / X / Pinterest / Reddit / Tumblr **single post** | remote Cobalt picker (unchanged) | Cobalt error → gallery-dl (bulk) → yt-dlp |
//! | Same hosts, **profile / board / collection / channel** | **gallery-dl** | — |
//! | Weibo **image post / album** | **gallery-dl** (browser cookie) | video keeps the existing block / sniffer path |
//! | Unknown domain | remote → yt-dlp (unchanged) | never blind-run gallery-dl |
//!
//! The [`GalleryRoute::Direct`] vs [`GalleryRoute::BulkCollection`] split only
//! drives task presentation (one item vs an album). Both run gallery-dl; the
//! authoritative item count comes from the `-j` enumeration at runtime, so the
//! static classifier is allowed to be conservative about that distinction.
//! The load-bearing decision is [`GalleryRoute::owned`] (run gallery-dl or not).

/// How (or whether) a URL should be handed to gallery-dl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryRoute {
    /// Not a gallery-dl job: keep the existing remote → yt-dlp flow.
    None,
    /// A supported image site, single work / direct item.
    Direct,
    /// A profile / board / gallery / album / tag search spanning many items.
    BulkCollection,
}

impl GalleryRoute {
    /// True when gallery-dl should handle this URL.
    pub fn owned(self) -> bool {
        self != GalleryRoute::None
    }
}

/// Image-centric hosts gallery-dl owns **even for a single item**
/// (suffix match on the normalized host: exact or `.<suffix>`).
const IMAGE_HOST_SUFFIXES: &[&str] = &[
    // Pixiv.
    "pixiv.net",
    // Portfolios / art communities.
    "artstation.com",
    "deviantart.com",
    "sta.sh",
    // booru family ("等 booru 系").
    "donmai.us",       // danbooru
    "gelbooru.com",
    "safebooru.org",
    "yande.re",
    "konachan.com",
    "konachan.net",
    "zerochan.net",
    "rule34.xxx",
    "e621.net",
    "e926.net",
    "paheal.net",
    "hypnohub.net",
    "realbooru.com",
    "xbooru.com",
    "tbib.org",
    // Image hosts / albums.
    "imgur.com",
    "flickr.com",
    "flic.kr",
    // Weibo image posts (video posts fall through to the sniffer at runtime).
    "weibo.com",
    "weibo.cn",
    // Japanese art / comic sites.
    "nijie.info",
    "seiga.nicovideo.jp",
    "nicoseiga.jp",
];

/// Hosts where gallery-dl is used **only for bulk/profile forms**; a single
/// post keeps the remote Cobalt picker path.
const BULK_ONLY_HOST_SUFFIXES: &[&str] = &[
    "instagram.com",
    "twitter.com",
    "x.com",
    "pinterest.com",
    "reddit.com",
    "tumblr.com",
];

/// Path substrings that indicate a multi-item collection on an image host.
const COLLECTION_MARKERS: &[&str] = &[
    "/gallery",
    "/galleries",
    "/album",
    "/albums",
    "/a/",          // imgur album
    "/t/",          // imgur tag
    "/board",
    "/boards",
    "/pool",
    "/pools",
    "/user/",
    "/users/",
    "/u/",          // ArtStation user id, booru users
    "/artist",
    "/artists",
    "/profile",
    "/member/",     // Niconico Seiga member
    "/clip/",
    "/folder",
    "/favourites",
    "/favorites",
    "/sets/",       // Flickr albums
    "/groups/",
    "/photostream",
    "/people/",
    "/creator/",    // pixiv Fanbox creator
    "/tag/",
    "/tags/",
    "/series",
    "/chapter",
    "/account/",
    "/search",
];

fn normalize_host(host: &str) -> String {
    host.trim_start_matches("www.").to_ascii_lowercase()
}

fn host_matches(host: &str, suffixes: &[&str]) -> bool {
    suffixes
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

/// Non-empty `tags=` (booru tag search) or an explicit pool listing.
fn query_is_listing(query: &str) -> bool {
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else { continue };
        match key {
            // A real tag search (empty tags means the site's front-page listing).
            "tags" => {
                let meaningful = value.replace("%20", "").replace('+', "");
                if !meaningful.is_empty() {
                    return true;
                }
            }
            "page" if value == "pool" => return true,
            "s" if value == "list" || value == "index" => return true,
            _ => {}
        }
    }
    false
}

fn image_path_is_collection(host: &str, path: &str, query: &str) -> bool {
    if query_is_listing(query) {
        return true;
    }
    // pixiv Fanbox creator roots look like `/@creator` (no fixed marker).
    if host.ends_with("fanbox.cc") && path.starts_with("/@") {
        return true;
    }
    // Flickr profile/photostream: `/photos/{user}/` with no photo id after it.
    if host.ends_with("flickr.com") || host.ends_with("flic.kr") {
        if let Some(rest) = path.strip_prefix("/photos/") {
            let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
            if segs.len() <= 1 {
                return true;
            }
        }
    }
    COLLECTION_MARKERS.iter().any(|marker| path.contains(marker))
}

/// Path segments, lower-cased, empties removed.
fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Single-post forms on bulk-only hosts (these stay with the remote picker).
fn bulk_only_is_single(host: &str, path: &str) -> bool {
    let segs = segments(path);
    let first = segs.first().copied().unwrap_or("");

    if host_matches(host, &["instagram.com"]) {
        // /p/{id}, /reel/{id}, /tv/{id}, /stories/... are single / login-walled.
        return matches!(first, "p" | "reel" | "reels" | "tv" | "stories");
    }
    if host_matches(host, &["twitter.com", "x.com"]) {
        return segs.iter().any(|s| *s == "status" || *s == "statuses");
    }
    if host_matches(host, &["pinterest.com"]) {
        return first == "pin";
    }
    if host_matches(host, &["reddit.com"]) {
        // /r/{sub}/comments/{id}, /comments/{id}, /comments/{id}/...
        return segs.iter().any(|s| *s == "comments");
    }
    if host_matches(host, &["tumblr.com"]) {
        if segs.iter().any(|s| *s == "post" || *s == "image") {
            return true;
        }
        // `/blog/view/{blog}/{postId}` is one specific post (no id => the blog).
        if first == "blog" && segs.get(1).copied() == Some("view") && segs.len() >= 4 {
            return true;
        }
    }
    false
}

/// Profile / board / collection forms on bulk-only hosts.
fn bulk_only_is_collection(host: &str, path: &str) -> bool {
    let segs = segments(path);
    let first = segs.first().copied().unwrap_or("");

    if host_matches(host, &["instagram.com"]) {
        // A bare handle `/{user}/`, a saved/tagged sub-page, or a hashtag explore.
        if first == "explore" {
            return segs.get(1).copied() == Some("tags");
        }
        return !segs.is_empty() && !matches!(first, "explore" | "accounts" | "developer");
    }
    if host_matches(host, &["twitter.com", "x.com"]) {
        const RESERVED: &[&str] = &[
            "home", "explore", "messages", "notifications", "settings", "i", "intent",
            "share", "login", "signup", "hashtag",
        ];
        if first == "search" || first == "hashtag" || segs.iter().any(|s| *s == "events") {
            return true;
        }
        // /{handle}, /{handle}/media, /{handle}/with_replies, /{handle}/likes.
        if segs.len() == 1 {
            return !RESERVED.contains(&first);
        }
        if segs.len() >= 2 && !RESERVED.contains(&first) {
            return segs[1..].iter().all(|s| {
                matches!(*s, "media" | "with_replies" | "likes" | "followers" | "following")
            });
        }
        return false;
    }
    if host_matches(host, &["pinterest.com"]) {
        if first == "search" {
            return true;
        }
        // /{user}/ (profile) or /{user}/{board}/.
        return !segs.is_empty()
            && !matches!(first, "business" | "ideas" | "categories" | "today" | "topics");
    }
    if host_matches(host, &["reddit.com"]) {
        if first == "search" {
            return true;
        }
        // /r/{sub}[/hot|new|top|...], /user/{u}, /u/{u}.
        return matches!(first, "r" | "user" | "u") && !segs.iter().any(|s| *s == "comments");
    }
    if host_matches(host, &["tumblr.com"]) {
        // Console blog view, tag/archive pages, or a blog subdomain root.
        if first == "blog" && segs.get(1).copied() == Some("view") {
            return true;
        }
        if segs.iter().any(|s| *s == "tagged" || *s == "archive") {
            return true;
        }
        // `{name}.tumblr.com` is itself a blog; anything that is not a single
        // post is a blog-level (multi-post) view.
        return host != "tumblr.com" && host != "www.tumblr.com";
    }
    false
}

/// Classify a URL for the gallery-dl sidecar. Never panics; bad input is
/// [`GalleryRoute::None`] so the caller falls back to the existing flow.
pub fn classify(url: &str) -> GalleryRoute {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return GalleryRoute::None;
    };
    let Some(raw_host) = parsed.host_str() else {
        return GalleryRoute::None;
    };
    let host = normalize_host(raw_host);
    let path = parsed.path().to_ascii_lowercase();
    let query = parsed.query().unwrap_or("").to_ascii_lowercase();

    if host_matches(&host, IMAGE_HOST_SUFFIXES) {
        if image_path_is_collection(&host, &path, &query) {
            GalleryRoute::BulkCollection
        } else {
            GalleryRoute::Direct
        }
    } else if host_matches(&host, BULK_ONLY_HOST_SUFFIXES) {
        if bulk_only_is_single(&host, &path) {
            GalleryRoute::None
        } else if bulk_only_is_collection(&host, &path) {
            GalleryRoute::BulkCollection
        } else {
            GalleryRoute::None
        }
    } else {
        GalleryRoute::None
    }
}

/// True when the **host** is one gallery-dl supports, regardless of whether
/// this specific URL is a single post or a collection. Used by the remote-error
/// fallback: a single IG / Reddit / ... post the remote picker rejected can
/// still be tried with gallery-dl before yt-dlp.
pub fn is_gallery_capable_host(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(raw_host) = parsed.host_str() else {
        return false;
    };
    let host = normalize_host(raw_host);
    host_matches(&host, IMAGE_HOST_SUFFIXES) || host_matches(&host, BULK_ONLY_HOST_SUFFIXES)
}

/// Count the media entries in `gallery-dl -j` (dump-json) output.
///
/// Current gallery-dl emits one **pretty-printed top-level JSON array** whose
/// elements are message arrays; resolved URL rows look like `[3, "<url>", {...}]`
/// while metadata rows use other codes (1/2). Older versions / other flags may
/// emit one compact message array per line, so both shapes are accepted.
pub fn count_dump_items(stdout: &str) -> usize {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return 0;
    }

    // Shape A: one JSON document — an array of message arrays.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(items) = value.as_array() {
            return items
                .iter()
                .filter(|item| item.get(0).and_then(|code| code.as_i64()) == Some(3))
                .count();
        }
    }

    // Shape B: one compact JSON message array per line (NDJSON).
    let mut count = 0usize;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value.get(0).and_then(|code| code.as_i64()) == Some(3) {
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_host_single_items_are_direct() {
        assert_eq!(
            classify("https://www.pixiv.net/en/artworks/123456789"),
            GalleryRoute::Direct
        );
        assert_eq!(
            classify("https://www.artstation.com/artwork/abc123"),
            GalleryRoute::Direct
        );
        assert_eq!(
            classify("https://www.deviantart.com/someuser/art/my-piece-987654321"),
            GalleryRoute::Direct
        );
        assert_eq!(
            classify("https://danbooru.donmai.us/posts/1234567"),
            GalleryRoute::Direct
        );
        assert_eq!(
            classify("https://gelbooru.com/index.php?page=post&s=view&id=1234567"),
            GalleryRoute::Direct
        );
        assert_eq!(classify("https://yande.re/post/show/123456"), GalleryRoute::Direct);
        assert_eq!(classify("https://i.imgur.com/abcd123.png"), GalleryRoute::Direct);
        assert_eq!(classify("https://imgur.com/abcd123"), GalleryRoute::Direct);
        assert_eq!(
            classify("https://www.flickr.com/photos/someuser/53123456789/"),
            GalleryRoute::Direct
        );
        assert_eq!(classify("https://nijie.info/view.php?id=123456"), GalleryRoute::Direct);
        assert_eq!(
            classify("https://seiga.nicovideo.jp/seiga/im12345678"),
            GalleryRoute::Direct
        );
        // Weibo single image post.
        assert_eq!(
            classify("https://weibo.com/1234567890/AbC123XyZ"),
            GalleryRoute::Direct
        );
    }

    #[test]
    fn image_host_collections_are_bulk() {
        assert_eq!(
            classify("https://www.pixiv.net/users/12345678"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(
            classify("https://www.deviantart.com/someuser/gallery/all"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(
            classify("https://www.artstation.com/u/123456"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(
            classify("https://danbooru.donmai.us/posts?tags=hatsune_miku"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(
            classify("https://gelbooru.com/index.php?page=post&s=list&tags=cat"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(classify("https://yande.re/post?tags=long_hair"), GalleryRoute::BulkCollection);
        assert_eq!(classify("https://imgur.com/a/abcd123"), GalleryRoute::BulkCollection);
        assert_eq!(classify("https://imgur.com/t/cat"), GalleryRoute::BulkCollection);
        assert_eq!(
            classify("https://www.flickr.com/photos/someuser/"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(
            classify("https://www.flickr.com/photos/someuser/sets/721576/"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(classify("https://nijie.info/users/123456"), GalleryRoute::BulkCollection);
        assert_eq!(
            classify("https://seiga.nicovideo.jp/member/illust/12345"),
            GalleryRoute::BulkCollection
        );
    }

    #[test]
    fn bulk_only_single_posts_stay_with_remote() {
        assert_eq!(classify("https://www.instagram.com/p/AbC123XYZ/"), GalleryRoute::None);
        assert_eq!(classify("https://www.instagram.com/reel/AbC123XYZ/"), GalleryRoute::None);
        assert_eq!(classify("https://x.com/someuser/status/123456789"), GalleryRoute::None);
        assert_eq!(
            classify("https://twitter.com/someuser/status/123456789/photo/1"),
            GalleryRoute::None
        );
        assert_eq!(classify("https://www.pinterest.com/pin/123456789/"), GalleryRoute::None);
        assert_eq!(
            classify("https://www.reddit.com/r/cats/comments/abc123/my_post/"),
            GalleryRoute::None
        );
        assert_eq!(
            classify("https://www.reddit.com/comments/abc123/my_post/"),
            GalleryRoute::None
        );
        assert_eq!(
            classify("https://someuser.tumblr.com/post/123456789/my-post"),
            GalleryRoute::None
        );
        assert_eq!(
            classify("https://www.tumblr.com/blog/view/someuser/123456789"),
            GalleryRoute::None
        );
    }

    #[test]
    fn bulk_only_profiles_and_boards_are_owned() {
        assert_eq!(classify("https://www.instagram.com/someuser/"), GalleryRoute::BulkCollection);
        assert_eq!(classify("https://x.com/someuser/media"), GalleryRoute::BulkCollection);
        assert_eq!(classify("https://x.com/someuser"), GalleryRoute::BulkCollection);
        assert_eq!(
            classify("https://twitter.com/search?q=cat&src=typed_query"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(
            classify("https://www.pinterest.com/someuser/"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(
            classify("https://www.pinterest.com/someuser/my-board/"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(classify("https://www.reddit.com/r/cats/"), GalleryRoute::BulkCollection);
        assert_eq!(classify("https://www.reddit.com/r/cats/top/?t=week"), GalleryRoute::BulkCollection);
        assert_eq!(classify("https://www.reddit.com/user/someuser/"), GalleryRoute::BulkCollection);
        assert_eq!(classify("https://someuser.tumblr.com/"), GalleryRoute::BulkCollection);
        assert_eq!(
            classify("https://someuser.tumblr.com/tagged/cat"),
            GalleryRoute::BulkCollection
        );
        assert_eq!(
            classify("https://www.tumblr.com/blog/view/someuser"),
            GalleryRoute::BulkCollection
        );
    }

    #[test]
    fn unknown_and_lookalike_hosts_are_never_owned() {
        assert_eq!(classify("https://vimeo.com/123456789"), GalleryRoute::None);
        assert_eq!(classify("https://www.youtube.com/watch?v=abc"), GalleryRoute::None);
        assert_eq!(classify("https://notpixiv.net/artworks/1"), GalleryRoute::None);
        assert_eq!(classify("https://pixiv.evil.com/artworks/1"), GalleryRoute::None);
        assert_eq!(classify("https://danbooru.example.com/posts/1"), GalleryRoute::None);
        assert_eq!(classify("https://myimgur.com/a/1"), GalleryRoute::None);
        assert_eq!(classify("https://reddit.example.com/r/cats"), GalleryRoute::None);
        assert_eq!(classify("https://www.tumblr.com/dashboard"), GalleryRoute::None);
        assert_eq!(classify("https://www.instagram.com/explore/"), GalleryRoute::None);
        // A direct image on an unknown host must NOT blind-run gallery-dl.
        assert_eq!(classify("https://example.com/photo.jpg"), GalleryRoute::None);
        // Garbage input.
        assert_eq!(classify("not a url"), GalleryRoute::None);
        assert_eq!(classify(""), GalleryRoute::None);
    }

    #[test]
    fn owned_helper_matches_non_none() {
        assert!(classify("https://www.pixiv.net/artworks/1").owned());
        assert!(classify("https://x.com/u/media").owned());
        assert!(!classify("https://example.com").owned());
    }

    #[test]
    fn capable_host_matches_single_posts_too() {
        // Bulk-only single posts classify as None but the host is still capable.
        assert!(is_gallery_capable_host("https://www.instagram.com/p/AbC/"));
        assert!(is_gallery_capable_host("https://www.reddit.com/r/cats/comments/abc/x/"));
        assert!(is_gallery_capable_host("https://x.com/u/status/1"));
        assert!(is_gallery_capable_host("https://danbooru.donmai.us/posts/1"));
        assert!(!is_gallery_capable_host("https://vimeo.com/1"));
        assert!(!is_gallery_capable_host("https://reddit.example.com/r/x"));
        assert!(!is_gallery_capable_host("not a url"));
    }

    #[test]
    fn counts_only_url_rows_in_dump_json() {
        let dump = "\
[2, {\"category\": \"pixiv\", \"filename\": \"123_p0\"}]
[3, \"https://i.pximg.net/img/123_p0.png\", {\"extension\": \"png\"}]
some non-json progress line
[1, {\"category\": \"pixiv\"}]
[3, \"https://i.pximg.net/img/123_p1.png\", {\"extension\": \"png\"}]
";
        assert_eq!(count_dump_items(dump), 2);
        assert_eq!(count_dump_items(""), 0);
        assert_eq!(count_dump_items("garbage\nmore garbage"), 0);

        // Real gallery-dl shape: a single pretty-printed top-level array.
        let pretty = "[
  [
    2,
    { \"category\": \"yandere\", \"filename\": \"yande.re_1_a\" }
  ],
  [
    3,
    \"https://files.example/1.jpg\",
    { \"extension\": \"jpg\" }
  ],
  [
    3,
    \"https://files.example/2.jpg\",
    { \"extension\": \"jpg\" }
  ]
]";
        assert_eq!(count_dump_items(pretty), 2);
    }
}
