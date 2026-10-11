//! User-Guided Extraction (UGE): a general "open a browser, play the video,
//! pick the detected stream" fallback for sites no engine handles out of the
//! box (the Downie-style safety net).
//!
//! It is deliberately **user-triggered** (never auto-routed): the caller opens
//! a real webview at the page URL, an injected script watches network traffic
//! and media elements for progressive media URLs, lists them in a small
//! overlay, and the user clicks the one they want. The chosen URL is handed
//! back through the `cobalt-uge://` scheme and routed to the direct-stream
//! downloader. Pure helpers are unit-tested; the DOM/sniffing lives in the
//! injected `UGE_SCRIPT`.

use std::time::Duration;

use tauri::{webview::WebviewWindowBuilder, AppHandle, Manager, Url, WebviewUrl};

const UGE_SCHEME: &str = "cobalt-uge";
/// The user is driving this window, so give them far longer than the
/// Xinpianchang auto-resolver before timing out.
const UGE_TIMEOUT: Duration = Duration::from_secs(180);

/// Realistic desktop UA for the extraction webview. A Tauri/WRY default UA
/// ("Tauri"/"Mozilla/5.0 (X11; …) AppleWebKit/…") is a well-known bot signal,
/// so many sites degrade or refuse to serve their media API to it. Spoofing a
/// current Safari UA keeps those sites on their normal path.
///
/// Re-exported from the crate root so every surface (yt-dlp `--user-agent`,
/// UGE webview, direct-stream headers) stays in sync from one constant.
pub use crate::DESKTOP_UA as UGE_USER_AGENT;

/// Progressive containers and audio the direct downloader can save as-is.
/// Kept in lockstep with the injected script's `PROGRESSIVE` / `AUDIO_RE`.
const DIRECT_MEDIA_EXT: &[&str] = &[
    "mp4", "m4v", "webm", "mkv", "mk3d", "mks", "mov", "flv", "f4v", "3gp", "3g2", "avi", "wmv",
    "mpg", "mpeg", "m2ts", "mts", "ogv", "mp3", "m4a", "m4b", "aac", "ogg", "oga", "opus", "wav",
    "flac", "weba", "wma", "mka",
];

/// HLS/DASH/HDS/Smooth manifests. yt-dlp demuxes these; the direct downloader
/// would only save playlist text. Mirrors the script's `MANIFEST` regex.
const MANIFEST_EXT: &[&str] = &["m3u8", "m3u", "mpd", "f4m", "isml", "ism"];

/// Raw fragments. A single fragment is not a full presentation, so the picker
/// caps how many it shows and the direct downloader keeps the real extension
/// instead of pretending the bytes are an mp4. Mirrors `TS_SEGMENT` / `RAW_SEGMENT`.
const SEGMENT_EXT: &[&str] = &[
    "ts", "m4s", "cmfv", "cmfa", "h264", "265", "avc", "hevc", "adts",
];

/// Extensions that are never the media asset, even if the path contains a
/// media-ish token (`/media/logo.png`, `thumb.jpg`).
const NON_MEDIA_EXT: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "avif", "bmp", "svg", "ico", "css", "js", "mjs", "html",
    "htm", "json", "txt", "pdf", "zip", "gz", "wasm", "woff", "woff2", "ttf", "map", "php", "asp",
    "aspx", "jsp", "mhtml",
];

/// What the picker resolved, and which fetcher should take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UgeMediaClass {
    /// Progressive container or audio file.
    Direct,
    /// Manifest that has to be demuxed by yt-dlp.
    Manifest,
    /// One transport / fMP4 fragment. Saved with its own extension.
    Segment,
}

/// Is this resolved URL an HLS/DASH/HDS/Smooth manifest that needs yt-dlp?
pub fn is_manifest_url(url: &str) -> bool {
    classify_uge_url(url) == Some(UgeMediaClass::Manifest)
}

/// Is this a direct playable media URL (progressive container or audio)?
/// Manifests, fragments, and extension-less pages are not. Used by the
/// generic-extractor fallback so a bare page scraped by `generic` is rejected
/// unless yt-dlp actually found a playable format.
pub fn extension_is_direct_media(url: &str) -> bool {
    media_extension(url).is_some_and(|ext| is_direct_ext(&ext))
}

/// File extension yt-dlp (or the direct downloader) can treat as audio/video,
/// including manifests and raw fragments. Images and documents are not.
pub fn ext_is_playable_media(ext: &str) -> bool {
    let ext = normalize_ext(ext);
    is_direct_ext(&ext) || is_manifest_ext(&ext) || is_segment_ext(&ext)
}

pub fn is_non_media_extension(ext: &str) -> bool {
    is_non_media_ext(&normalize_ext(ext))
}

#[derive(Debug, Clone)]
pub struct ResolvedUge {
    pub url: String,
    pub filename: String,
    pub expected_bytes: u64,
    pub media_class: UgeMediaClass,
}

/// Manifests are not single files. Everything else the picker offers can be
/// fetched directly (segments keep their real extension).
pub fn needs_ytdlp_demux(resolved: &ResolvedUge) -> bool {
    resolved.media_class == UgeMediaClass::Manifest
}

/// Manual extraction only makes sense for a real web page.
pub fn is_candidate(url: &str) -> bool {
    matches!(Url::parse(url).map(|parsed| parsed.scheme().to_owned()), Ok(scheme) if scheme == "http" || scheme == "https")
}

fn normalize_ext(ext: &str) -> String {
    ext.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn is_direct_ext(ext: &str) -> bool {
    DIRECT_MEDIA_EXT.contains(&ext)
}

fn is_manifest_ext(ext: &str) -> bool {
    MANIFEST_EXT.contains(&ext)
}

fn is_segment_ext(ext: &str) -> bool {
    SEGMENT_EXT.contains(&ext)
}

fn is_non_media_ext(ext: &str) -> bool {
    NON_MEDIA_EXT.contains(&ext)
}

fn ext_token(filename: &str) -> Option<String> {
    if !filename.contains('.') {
        return None;
    }
    let ext = filename.rsplit('.').next()?;
    if (2..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(ext.to_ascii_lowercase())
    } else {
        None
    }
}

/// Best-effort container extension from a media URL's path (ignoring query
/// and a trailing slash, so `video.mp4/` still counts).
fn extension_from_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let path = path.trim_end_matches('/');
    let last = path.rsplit('/').next()?;
    ext_token(last)
}

/// Extension declared by a query value (`file=clip.mp4`, `format=m3u8`,
/// `mime=video/mp4`) when the path itself has none.
fn extension_from_query(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    for (key, value) in parsed.query_pairs() {
        let key = key.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "format" | "ext" | "container" | "mime" | "type"
        ) {
            if let Some(ext) = ext_from_format_token(&value) {
                return Some(ext);
            }
        }
        if let Some(ext) = extension_from_url(&value) {
            if is_direct_ext(&ext) || is_manifest_ext(&ext) || is_segment_ext(&ext) {
                return Some(ext);
            }
        }
    }
    None
}

fn ext_from_format_token(value: &str) -> Option<String> {
    let token = value.split(';').next()?.trim().to_ascii_lowercase();
    let token = token.split('/').next_back()?.trim();
    if token.is_empty() {
        return None;
    }
    if token.contains("mpegurl") || token == "hls" || token == "m3u8" || token == "x-mpegurl" {
        return Some("m3u8".to_string());
    }
    if token == "m3u" {
        return Some("m3u".to_string());
    }
    if token.contains("dash") || token == "mpd" {
        return Some("mpd".to_string());
    }
    if token == "f4m" || token == "hds" {
        return Some("f4m".to_string());
    }
    if is_direct_ext(token) || is_manifest_ext(token) || is_segment_ext(token) {
        return Some(token.to_string());
    }
    None
}

/// Path extension when it is one we know, otherwise a media extension carried
/// in the query string. A known non-media path extension wins so
/// `logo.jpg?format=mp4` stays a jpeg.
fn media_extension(url: &str) -> Option<String> {
    if let Some(ext) = extension_from_url(url) {
        if is_non_media_ext(&ext)
            || is_direct_ext(&ext)
            || is_manifest_ext(&ext)
            || is_segment_ext(&ext)
        {
            return Some(ext);
        }
    }
    extension_from_query(url)
}

fn class_from_ext(ext: &str) -> Option<UgeMediaClass> {
    if is_manifest_ext(ext) {
        Some(UgeMediaClass::Manifest)
    } else if is_segment_ext(ext) {
        Some(UgeMediaClass::Segment)
    } else if is_direct_ext(ext) {
        Some(UgeMediaClass::Direct)
    } else {
        None
    }
}

/// Extension-less URLs that still name a manifest or a direct stream.
/// Deliberately narrower than "anything with /video/ or /media/ in the path",
/// which matches thumbnails and article pages.
fn hint_class(url: &str) -> Option<UgeMediaClass> {
    let lower = url.to_ascii_lowercase();
    if lower.contains("format=m3u8")
        || lower.contains("format=m3u")
        || lower.contains("format=hls")
        || lower.contains("format=mpd")
        || lower.contains("format=dash")
        || lower.contains("mpegurl")
        || lower.contains(".m3u8")
        || lower.contains(".m3u")
        || lower.contains(".mpd")
        || lower.contains(".f4m")
        || lower.contains(".isml")
    {
        return Some(UgeMediaClass::Manifest);
    }
    if lower.contains("videoplayback")
        || lower.contains("mime=video")
        || lower.contains("mime=audio")
        || lower.contains("format=mp4")
        || lower.contains("format=webm")
        || lower.contains("format=mkv")
    {
        return Some(UgeMediaClass::Direct);
    }
    None
}

/// URL-only classification. Matches the injected script's extension and
/// strong-hint branches (content-type is applied separately at callback time).
pub fn classify_uge_url(url: &str) -> Option<UgeMediaClass> {
    if let Some(ext) = media_extension(url) {
        if is_non_media_ext(&ext) {
            return None;
        }
        if let Some(class) = class_from_ext(&ext) {
            return Some(class);
        }
    }
    hint_class(url)
}

fn class_from_mime(mime: &str) -> Option<UgeMediaClass> {
    let mime = mime.split(';').next()?.trim().to_ascii_lowercase();
    if mime.is_empty()
        || mime.starts_with("text/")
        || mime.starts_with("image/")
        || mime.contains("html")
        || mime == "application/json"
        || mime == "text/json"
        || mime.contains("javascript")
    {
        return None;
    }
    if mime.contains("mpegurl")
        || mime.contains("dash+xml")
        || mime.contains("sstr+xml")
        || mime == "application/f4m+xml"
    {
        return Some(UgeMediaClass::Manifest);
    }
    if mime == "video/mp2t" {
        return Some(UgeMediaClass::Segment);
    }
    if mime.starts_with("video/") || mime.starts_with("audio/") {
        return Some(UgeMediaClass::Direct);
    }
    None
}

/// Does a URL carry a media file extension (or a strong stream hint) the
/// picker should surface? Content-type matches are decided in
/// `parse_callback_url`, which also receives the mime the sniffer observed.
pub fn is_uge_media_url(url: &str) -> bool {
    classify_uge_url(url).is_some()
}

fn callback_media_class(url: &str, mime: &str) -> Option<UgeMediaClass> {
    if let Some(ext) = media_extension(url) {
        if is_non_media_ext(&ext) {
            return None;
        }
        if let Some(class) = class_from_ext(&ext) {
            return Some(class);
        }
    }
    if let Some(class) = hint_class(url) {
        return Some(class);
    }
    // Extension-less CDN URLs are real streams when the response said so.
    // A document mime never promotes a page into a download.
    class_from_mime(mime)
}

/// Build a safe, readable filename from the page title + media URL.
///
/// Direct containers and raw fragments keep their own suffix. Manifests are
/// demuxed by yt-dlp, so the placeholder name is `.mp4` rather than `.m3u8`
/// (the direct downloader never saves the playlist body).
pub fn safe_uge_filename(title: &str, url: &str) -> String {
    let ext = match classify_uge_url(url) {
        Some(UgeMediaClass::Direct) => media_extension(url)
            .filter(|ext| is_direct_ext(ext))
            .unwrap_or_else(|| "mp4".to_string()),
        Some(UgeMediaClass::Segment) => media_extension(url)
            .filter(|ext| is_segment_ext(ext))
            .unwrap_or_else(|| "ts".to_string()),
        Some(UgeMediaClass::Manifest) | None => "mp4".to_string(),
    };
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim();
    let stem: String = if cleaned.is_empty() {
        "manual-extract".to_string()
    } else {
        cleaned.chars().take(120).collect()
    };
    format!("{stem}.{ext}")
}

/// Parse a `cobalt-uge://resolved|error/…` callback emitted by the injected
/// script. Mirrors the Xinpianchang resolver contract.
pub fn parse_callback_url(callback: &Url) -> Result<ResolvedUge, String> {
    match (callback.scheme(), callback.host_str()) {
        (UGE_SCHEME, Some("resolved")) => {
            let mut media = None;
            let mut title = None;
            let mut mime = None;
            for (key, value) in callback.query_pairs() {
                match key.as_ref() {
                    "url" => media = Some(value.into_owned()),
                    "title" => title = Some(value.into_owned()),
                    "mime" => mime = Some(value.into_owned()),
                    _ => {}
                }
            }
            let media = media
                .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
                .ok_or_else(|| "Manual extraction returned no media URL".to_string())?;
            // The picker only offers sniffed media URLs, but a hand-crafted
            // callback (or a future panel bug) could hand back a bare page —
            // fail loudly instead of downloading HTML as ".mp4". An
            // extension-less URL is accepted only when the sniffer observed a
            // real media content-type (passed back as `mime`).
            let media_class = callback_media_class(&media, mime.as_deref().unwrap_or(""))
                .ok_or_else(|| "Manual extraction returned a non-media URL".to_string())?;
            let filename = safe_uge_filename(title.as_deref().unwrap_or(""), &media);
            Ok(ResolvedUge {
                url: media,
                filename,
                expected_bytes: 0,
                media_class,
            })
        }
        (UGE_SCHEME, Some("error")) => {
            let message = callback
                .query_pairs()
                .find(|(key, _)| key == "message")
                .map(|(_, value)| value.into_owned())
                .unwrap_or_else(|| "Manual extraction was cancelled".to_string());
            Err(message)
        }
        _ => Err("Unexpected manual-extraction callback".to_string()),
    }
}

fn uge_catalog(locale: &str) -> &'static str {
    match locale.trim() {
        "zh" | "zh-CN" | "zh-Hans" | "zh-cn" => include_str!("../../src/lib/i18n/zh.json"),
        "ru" | "ru-RU" => include_str!("../../src/lib/i18n/ru.json"),
        _ => include_str!("../../src/lib/i18n/en.json"),
    }
}

fn uge_string(catalog: &serde_json::Value, fallback: &serde_json::Value, key: &str) -> String {
    catalog
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .or_else(|| fallback.get(key).and_then(|value| value.as_str()))
        .unwrap_or("")
        .to_string()
}

/// Overlay copy for the extraction webview, taken from the app i18n catalogs.
/// Unknown locales fall back to English. `zh-CN` is accepted as `zh`.
pub fn uge_overlay_messages(locale: &str) -> serde_json::Value {
    let english: serde_json::Value =
        serde_json::from_str(include_str!("../../src/lib/i18n/en.json"))
            .unwrap_or(serde_json::json!({}));
    let local: serde_json::Value =
        serde_json::from_str(uge_catalog(locale)).unwrap_or_else(|_| english.clone());
    let pick = |key: &str| uge_string(&local, &english, key);
    serde_json::json!({
        "title": pick("uge.title"),
        "empty": pick("uge.empty"),
        "mse": pick("uge.mse"),
        "cancelled": pick("uge.cancelled"),
        "window": pick("uge.window"),
        "kinds": {
            "video": pick("uge.kind.video"),
            "audio": pick("uge.kind.audio"),
            "manifest": pick("uge.kind.manifest"),
            "segment": pick("uge.kind.segment"),
            "hint": pick("uge.kind.hint"),
        },
        "vias": {
            "player": pick("uge.via.player"),
            "body": pick("uge.via.body"),
            "resource": pick("uge.via.resource"),
            "network": pick("uge.via.network"),
        }
    })
}

pub fn uge_initialization_script(locale: &str) -> String {
    let messages = uge_overlay_messages(locale);
    format!("window.__cobaltUgeI18n = {messages};\n{UGE_SCRIPT}")
}

/// Open a real webview at `url`, inject the media sniffer, and wait for the
/// user to pick a stream (or cancel / time out). `locale` selects the overlay
/// strings (`en`, `zh` / `zh-CN`, `ru`).
pub async fn start_uge(
    app_handle: &AppHandle,
    url: &str,
    locale: &str,
) -> Result<ResolvedUge, String> {
    let parsed = Url::parse(url).map_err(|error| format!("Invalid URL: {error}"))?;

    // A previous run may still hold the label; close it so the rebuild succeeds.
    if let Some(existing) = app_handle.get_webview_window("uge") {
        let _ = existing.close();
    }

    let messages = uge_overlay_messages(locale);
    let window_title = messages
        .get("window")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Cobalt · Manual extraction");
    let (navigation_tx, mut navigation_rx) = tokio::sync::mpsc::unbounded_channel::<Url>();
    let window = WebviewWindowBuilder::new(app_handle, "uge", WebviewUrl::External(parsed))
        .title(window_title)
        .inner_size(1000.0, 760.0)
        .user_agent(UGE_USER_AGENT)
        .initialization_script(uge_initialization_script(locale))
        .on_navigation(move |navigation_url| {
            if navigation_url.scheme() == UGE_SCHEME {
                let _ = navigation_tx.send(navigation_url.clone());
                return false;
            }
            // Free browsing: the user navigates the site and plays the video.
            true
        })
        .build()
        .map_err(|error| format!("Unable to open the extraction browser: {error}"))?;

    let callback = tokio::time::timeout(UGE_TIMEOUT, navigation_rx.recv()).await;
    let _ = window.close();

    match callback {
        Ok(Some(callback_url)) => parse_callback_url(&callback_url),
        Ok(None) => Err("The extraction window closed before a stream was chosen".to_string()),
        Err(_) => Err("Timed out waiting for a stream selection".to_string()),
    }
}

/// Injected into every page the extraction webview loads.
const UGE_SCRIPT: &str = r##"
(() => {
  if (window.__cobaltUGE) return;
  window.__cobaltUGE = true;
  const I18N = Object.assign({
    title: "Cobalt · detected media",
    empty: "No media yet — try playing the video.",
    mse: "This player keeps the stream inside the browser (MediaSource/blob), so there is no file URL yet. Press play — a playlist will show up here if the page requests one.",
    cancelled: "Cancelled",
    kinds: { video: "video", audio: "audio", manifest: "manifest", segment: "segment", hint: "hint" },
    vias: { player: "player", body: "body", resource: "resource", network: "network" }
  }, window.__cobaltUgeI18n || {});

  // Terminator includes "/" so `video.mp4/` is still a media URL.
  const PROGRESSIVE = /\.(mp4|m4v|webm|mkv|mk3d|mks|mov|flv|f4v|3gp|3g2|avi|wmv|mpg|mpeg|m2ts|mts|ogv)(\?|#|\/|$)/i;
  const AUDIO_RE = /\.(mp3|m4a|m4b|aac|ogg|oga|opus|wav|flac|weba|wma|mka)(\?|#|\/|$)/i;
  const MANIFEST = /\.(m3u8|m3u|mpd|f4m|isml|ism)(\?|#|\/|$)/i;
  // A raw transport-stream segment is almost never the playable asset.
  // Require a ?/#//$ terminator so query keys like `?ts=123` don't match.
  const TS_SEGMENT = /\.ts(\?|#|\/|$)/i;
  // fMP4 parts and raw elementary streams. `.aac` is audio (AUDIO_RE), not a fragment.
  const RAW_SEGMENT = /\.(m4s|cmfv|cmfa|h264|265|avc|hevc|adts)(\?|#|\/|$)/i;
  // Strong stream tokens only. `/media/`, `/video/` and a bare `stream` match
  // thumbnails and article pages, so they are not hints.
  const STRONG_HINT = /(?:\.m3u8\b|\.m3u\b|\.mpd\b|\.f4m\b|mpegurl|videoplayback|mime=video|mime=audio|(?:^|[?&#])format=(?:m3u8|m3u|mpd|mp4|webm|mkv|hls|dash)\b)/i;
  const NON_MEDIA = /\.(jpg|jpeg|png|gif|webp|avif|bmp|svg|ico|css|js|mjs|html|htm|json|txt|pdf|zip|gz|wasm|woff2?|ttf|map|php|asp|aspx|jsp|mhtml)(\?|#|\/|$)/i;
  const ABSOLUTE = /^https?:/i;
  const found = new Map();
  // Browsers hide the real stream behind blob:/MediaSource URLs; remembering
  // which media elements hit that wall lets the picker say so explicitly
  // instead of showing "No media yet" while a video is visibly playing.
  const BLOB_ELEMENTS = new Set();

  const isManifestType = (type) => {
    if (!type) return false;
    return /mpegurl|dash\+xml|sstr\+xml|f4m\+xml/i.test(String(type));
  };

  const pathOnly = (url) => String(url || '').split('?')[0].split('#')[0].replace(/\/+$/, '');
  const resolveUrl = (base, u) => {
    if (u == null) return '';
    u = String(u).trim().replace(/\\\//g, '/');
    if (!u || /^(data|blob):/i.test(u)) return '';
    try { return new URL(u, base || location.href).href; } catch (e) { return ''; }
  };
  const guessType = (url) => {
    const clean = pathOnly(url);
    const m = clean.match(/\.([a-z0-9]{2,5})$/i);
    return m ? m[1].toLowerCase() : 'media';
  };
  const isMedia = (url, type) => {
    if (!url || !ABSOLUTE.test(url)) return false;
    const path = pathOnly(url);
    if (NON_MEDIA.test(path)) return false;
    if (isManifestType(type)) return true;
    if (PROGRESSIVE.test(path) || AUDIO_RE.test(path) || MANIFEST.test(path) || TS_SEGMENT.test(path) || RAW_SEGMENT.test(path)) return true;
    if (type) {
      const t = String(type).toLowerCase();
      if (/^(video|audio)\//.test(t)) return true;
    }
    return STRONG_HINT.test(url);
  };

  // Higher-priority provenance wins so a URL first seen as a bare network fetch
  // gets upgraded when the player actually plays it or a manifest references it.
  const viaRank = { player: 3, body: 2, resource: 1, network: 1 };
  const kindOf = (url, type) => {
    const path = pathOnly(url);
    if (isManifestType(type) || MANIFEST.test(path) || /format=m3u8|format=mpd|format=hls|format=dash|mpegurl/i.test(url)) return 'manifest';
    if (AUDIO_RE.test(path) || (type && /^audio\//i.test(String(type)))) return 'audio';
    if (TS_SEGMENT.test(path) || RAW_SEGMENT.test(path) || (type && /mp2t/i.test(String(type)))) return 'segment';
    if (STRONG_HINT.test(url) && !PROGRESSIVE.test(path)) return 'hint';
    return 'video';
  };
  const countKind = (kind) => {
    let n = 0;
    found.forEach(function (v) { if (v.kind === kind) n++; });
    return n;
  };
  const add = (url, type, via) => {
    if (!isMedia(url, type)) return;
    const realMime = type && String(type).indexOf('/') !== -1;
    const kind = kindOf(url, type);
    const existing = found.get(url);
    if (existing) {
      if (realMime) {
        existing.type = type;
        existing.kind = kind;
      }
      if ((viaRank[via] || 0) > (viaRank[existing.via] || 0)) existing.via = via;
      scheduleRender();
      return;
    }
    // A media playlist can list hundreds of fragments. Keep a handful as a
    // last resort and always leave room for the manifest / container itself.
    if (kind === 'segment' && countKind('segment') >= 3) return;
    if (found.size >= 48 && kind !== 'manifest') return;
    found.set(url, {
      url,
      type: realMime ? type : guessType(url),
      via,
      kind,
    });
    scheduleRender();
  };

  // Pull media URLs out of an m3u8/mpd/JSON response body. Relative paths
  // are resolved against the response URL, which is how extension-less API
  // endpoints still yield a real playlist. HLS media playlists are NOT expanded
  // into every fragment — that floods the picker with hundreds of .ts URLs.
  const KNOWN_KEYS = {
    url: 1, src: 1, source: 1, file: 1, streamurl: 1, playurl: 1, videourl: 1,
    audiourl: 1, hlsurl: 1, dashurl: 1, manifest: 1, playlist: 1, m3u8: 1,
    m3u8url: 1, mp4: 1, mp4url: 1, downloadurl: 1, fileurl: 1, backupurl: 1,
    videoplayurl: 1, hls: 1, dash: 1
  };
  const keyNorm = (key) => String(key || '').toLowerCase().replace(/[_\-\s]/g, '');
  const pickGeneric = (u, source, base) => {
    const url = resolveUrl(base || location.href, u);
    if (!url) return;
    add(url, null, source);
  };
  const considerString = (raw, base, key) => {
    if (typeof raw !== 'string') return;
    const s = raw.trim().replace(/\\\//g, '/');
    if (!s || s.length > 4096) return;
    const known = !!(key && KNOWN_KEYS[keyNorm(key)]);
    const looks = PROGRESSIVE.test(s) || AUDIO_RE.test(s) || MANIFEST.test(s) || TS_SEGMENT.test(s) || RAW_SEGMENT.test(s) || STRONG_HINT.test(s);
    if (!known && !looks) return;
    if (s.length < 5) return;
    pickGeneric(s, 'network', base);
  };
  const scanJsonValue = (value, base, depth, key) => {
    if (value == null || depth > 4) return;
    if (typeof value === 'string') {
      considerString(value, base, key);
      return;
    }
    if (Array.isArray(value)) {
      for (let i = 0; i < Math.min(value.length, 40); i++) scanJsonValue(value[i], base, depth + 1, key);
      return;
    }
    if (typeof value === 'object') {
      const keys = Object.keys(value);
      for (let i = 0; i < Math.min(keys.length, 60); i++) {
        scanJsonValue(value[keys[i]], base, depth + 1, keys[i]);
      }
    }
  };
  // Master/media playlist: record the playlist itself and any nested variant
  // playlists. Keep a single fragment only when the body had no variant.
  const scanHls = (text, base) => {
    if (base) add(base, 'application/vnd.apple.mpegurl', 'body');
    const lines = String(text).split(/\r?\n/);
    let variants = 0;
    let sample = '';
    let pendingBw = 0;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].trim();
      if (!line) continue;
      if (line.charAt(0) === '#') {
        if (/^#EXT-X-STREAM-INF/i.test(line)) {
          const bw = line.match(/BANDWIDTH=(\d+)/i);
          pendingBw = bw ? parseInt(bw[1], 10) || 0 : 0;
        }
        continue;
      }
      const abs = resolveUrl(base, line);
      if (!abs || abs === base) { pendingBw = 0; continue; }
      const path = pathOnly(abs);
      if (MANIFEST.test(path) || /\.m3u8?$/i.test(path)) {
        add(abs, 'application/vnd.apple.mpegurl', 'body');
        const item = found.get(abs);
        if (item && pendingBw) item.bandwidth = Math.max(item.bandwidth || 0, pendingBw);
        variants++;
      } else if (!sample) {
        sample = abs;
      }
      pendingBw = 0;
    }
    if (!variants && sample) add(sample, null, 'body');
  };
  const scanMpd = (text, base) => {
    if (base) add(base, 'application/dash+xml', 'body');
    const re = /<BaseURL[^>]*>\s*([^<]+)\s*<\/BaseURL>/gi;
    let m;
    let n = 0;
    while ((m = re.exec(text)) && n < 8) {
      const abs = resolveUrl(base, m[1].trim());
      if (!abs || abs === base) continue;
      add(abs, null, 'body');
      n++;
    }
  };
  const scanTextForUrls = (text, base) => {
    if (!text) return;
    text = String(text);
    const absBase = resolveUrl(location.href, base) || String(base || location.href || '');
    const head = text.slice(0, 80).replace(/^\uFEFF/, '').trimStart();
    if (head.indexOf('#EXTM3U') === 0 || head.indexOf('#EXT-X-') === 0) {
      scanHls(text, absBase);
      return;
    }
    if (head.charAt(0) === '<' && /<MPD[\s>]/i.test(text.slice(0, 2500))) {
      scanMpd(text, absBase);
      return;
    }
    let parsed = null;
    try { parsed = JSON.parse(text); } catch (e) { parsed = null; }
    if (parsed && typeof parsed === 'object') { scanJsonValue(parsed, absBase, 0, ''); return; }
    if (text.length > 512000) return;
    const flat = text.indexOf('\\/') === -1 ? text : text.replace(/\\\//g, '/');
    const re = /https?:\/\/[^\s"'<>]+/gi;
    let m;
    let guard = 0;
    while ((m = re.exec(flat)) && guard++ < 60) pickGeneric(m[0], 'body', absBase);
  };

  // Rank so the best stream surfaces first: a progressive container beats a
  // manifest beats a raw fragment; something the player used beats a background
  // fetch; a resolution or HLS BANDWIDTH hint breaks ties.
  const scoreOf = (it) => {
    const kind = it.kind || kindOf(it.url, it.type);
    let s = 0;
    if (kind === 'video') s += 60;
    else if (kind === 'manifest') s += 50;
    else if (kind === 'audio') s += 45;
    else if (kind === 'hint') s += 35;
    else s += 5;
    if (it.via === 'player') s += 30;
    else if (it.via === 'body') s += 12;
    const h = String(it.url).match(/(?:^|[^\d])(\d{3,4})p\b/i);
    if (h) s += Math.min(parseInt(h[1], 10) || 0, 2160) / 100;
    if (it.bandwidth) s += Math.min(it.bandwidth, 20000000) / 1000000;
    return s;
  };

  let report = (kind, params) => {
    const query = new URLSearchParams(params).toString();
    window.location.replace('cobalt-uge://' + kind + '/?' + query);
  };

  const requestUrl = (input) => {
    if (!input) return '';
    if (typeof input === 'string') return input;
    try {
      if (typeof URL !== 'undefined' && input instanceof URL) return input.href;
    } catch (e) {}
    if (typeof input.url === 'string') return input.url;
    return '';
  };
  const shouldScanBody = (ct) => /mpegurl|dash\+xml|json|javascript|ecmascript|text\/plain|xml/i.test(String(ct || ''));

  try {
    const origFetch = window.fetch;
    window.fetch = function () {
      const args = Array.prototype.slice.call(arguments);
      try {
        const url = resolveUrl(location.href, requestUrl(args[0]));
        add(url, null, 'network');
        const result = origFetch.apply(this, args);
        if (result && result.then) {
          result.then(function (res) {
            try {
              const ct = res.headers.get('content-type');
              add(url, ct, 'network');
              // Extension-less API endpoints often hide the real playlist in
              // the body. Skip huge binary payloads (octet-stream video).
              if (shouldScanBody(ct)) {
                const clone = res.clone && res.clone();
                if (clone && clone.text) {
                  clone.text().then(function (t) { scanTextForUrls(t, url); }).catch(function () {});
                }
              }
            } catch (e) {}
          }).catch(function () {});
        }
        return result;
      } catch (e) { return origFetch.apply(this, args); }
    };
  } catch (e) {}

  try {
    const origOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (method, url) {
      const abs = resolveUrl(location.href, url);
      add(abs, null, 'network');
      this.addEventListener('load', function () {
        try {
          const ct = this.getResponseHeader('content-type');
          add(abs, ct, 'network');
          if (shouldScanBody(ct)) scanTextForUrls(this.responseText, abs);
        } catch (e) {}
      });
      return origOpen.apply(this, arguments);
    };
  } catch (e) {}

  // MSE players (YouTube/Bilibili-style DASH-in-MSE) feed the element through
  // MediaSource, so neither fetch/XHR nor currentSrc ever shows the segments.
  // Sniffing addSourceBuffer at least records the mime + lets scanMedia flag
  // the element as blob-backed, so the picker can explain instead of stalling.
  const BLOB_MIMES = [];
  try {
    if (window.MediaSource && MediaSource.prototype.addSourceBuffer && !window.__cobaltUGEMseHooked) {
      window.__cobaltUGEMseHooked = true;
      const origAddSb = MediaSource.prototype.addSourceBuffer;
      MediaSource.prototype.addSourceBuffer = function (mime) {
        try { BLOB_MIMES.push(String(mime || '')); } catch (e) {}
        return origAddSb.apply(this, arguments);
      };
    }
  } catch (e) {}

  try {
    const po = new PerformanceObserver(function (list) {
      list.getEntries().forEach(function (entry) {
        add(resolveUrl(location.href, entry.name), null, 'resource');
      });
    });
    po.observe({ type: 'resource', buffered: true });
    performance.getEntriesByType('resource').forEach(function (e) {
      add(resolveUrl(location.href, e.name), null, 'resource');
    });
  } catch (e) {}

  const noteElement = (el) => {
    if (!el) return;
    if (el.currentSrc) {
      if (/^blob:/i.test(el.currentSrc)) BLOB_ELEMENTS.add(el);
      else add(el.currentSrc, null, 'player');
    }
    if (el.src && el.src !== el.currentSrc) {
      if (/^blob:/i.test(el.src)) BLOB_ELEMENTS.add(el);
      else add(el.src, null, 'player');
    }
    el.querySelectorAll('source').forEach(function (s) { if (s.src) add(s.src, s.type, 'player'); });
    el.querySelectorAll('track').forEach(function (t) { if (t.src) pickGeneric(t.src, 'player', location.href); });
  };
  // Open shadow roots and same-origin iframes. Nested roots are walked a few
  // levels deep; closed shadow roots stay unreachable.
  const visitRoot = (root, depth) => {
    if (!root || depth > 3 || !root.querySelectorAll) return;
    root.querySelectorAll('video,audio').forEach(noteElement);
    root.querySelectorAll('*').forEach(function (host) {
      let shadow = null;
      try { shadow = host.shadowRoot; } catch (e) { shadow = null; }
      if (shadow) visitRoot(shadow, depth + 1);
    });
    root.querySelectorAll('iframe').forEach(function (frame) {
      try {
        if (frame.contentDocument) visitRoot(frame.contentDocument, depth + 1);
      } catch (e) {}
    });
  };
  let emptyNote = '';
  let scriptPasses = 0;
  const scanInlineScripts = () => {
    if (scriptPasses >= 8 || !document.querySelectorAll) return;
    scriptPasses++;
    document.querySelectorAll('script').forEach(function (s) {
      if (s.src || !s.textContent) return;
      const body = s.textContent;
      if (body.length < 40 || body.length > 200000) return;
      scanTextForUrls(body, location.href);
    });
  };
  const scanMedia = function () {
    if (!document.documentElement) return;
    visitRoot(document, 0);
    scanInlineScripts();
    // Re-render the empty state only when it changes. Rendering mutates the
    // DOM, which fires this observer; an unconditional schedule would loop.
    const note = !found.size && (BLOB_ELEMENTS.size || BLOB_MIMES.length) ? 'mse' : '';
    if (note && note !== emptyNote) {
      emptyNote = note;
      scheduleRender();
    }
  };
  try {
    new MutationObserver(scanMedia).observe(document.documentElement, { childList: true, subtree: true });
  } catch (e) {}
  scanMedia();
  setInterval(scanMedia, 1000);

  const ensurePanel = function () {
    let host = document.getElementById('__cobalt_uge');
    if (host) return host.shadowRoot;
    host = document.createElement('div');
    host.id = '__cobalt_uge';
    const shadow = host.attachShadow({ mode: 'open' });
    shadow.innerHTML =
      '<style>' +
      '.panel{position:fixed;right:16px;bottom:16px;z-index:2147483647;width:360px;max-height:60vh;overflow:auto;' +
      'font:13px/1.45 -apple-system,system-ui,sans-serif;background:#17171b;color:#e8e8ea;border:1px solid #3a3a40;' +
      'border-radius:12px;box-shadow:0 14px 44px rgba(0,0,0,.55);padding:12px}' +
      '.h{display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;font-weight:600}' +
      '.h button{all:unset;cursor:pointer;color:#9a9aa2;padding:2px 8px;font-size:14px}' +
      '.h button:hover{color:#e8e8ea}' +
      '.item{padding:8px 10px;border:1px solid #333;border-radius:8px;margin-bottom:6px;cursor:pointer}' +
      '.item:hover{background:#23232a;border-color:#5b8def}' +
      '.u{color:#8ab4f8;word-break:break-all;font-size:11px}' +
      '.m{color:#9a9aa2;font-size:11px;margin-top:3px}' +
      '.none{color:#9a9aa2;padding:6px 2px}' +
      '</style>' +
      '<div class="panel">' +
      '<div class="h"><span id="__cobalt_uge_title"></span><button id="__cobalt_uge_close">✕</button></div>' +
      '<div id="__cobalt_uge_list"></div>' +
      '</div>';
    document.documentElement.appendChild(host);
    const title = shadow.getElementById('__cobalt_uge_title');
    if (title) title.textContent = I18N.title || '';
    shadow.getElementById('__cobalt_uge_close').onclick = function () {
      report('error', { message: I18N.cancelled || 'Cancelled' });
    };
    return shadow;
  };

  let renderTimer = 0;
  function scheduleRender() {
    if (renderTimer) return;
    renderTimer = setTimeout(function () {
      renderTimer = 0;
      try { render(); } catch (e) {}
    }, 80);
  }

  const render = function () {
    if (!document.documentElement) return;
    const shadow = ensurePanel();
    if (!shadow) return;
    const list = shadow.getElementById('__cobalt_uge_list');
    const items = Array.from(found.values());
    if (!items.length) {
      const blobCount = BLOB_ELEMENTS.size || 0;
      const mseCount = BLOB_MIMES.length || 0;
      const note = document.createElement('div');
      note.className = 'none';
      note.textContent = (blobCount > 0 || mseCount > 0) ? (I18N.mse || '') : (I18N.empty || '');
      list.replaceChildren(note);
      return;
    }
    // Best stream first so the user usually only needs the top entry.
    items.sort(function (a, b) { return scoreOf(b) - scoreOf(a); });
    const shown = items.slice(0, 24);
    list.innerHTML = '';
    shown.forEach(function (it, i) {
      const div = document.createElement('div');
      div.className = 'item';
      const u = document.createElement('div');
      u.className = 'u';
      u.textContent = (i === 0 ? '★ ' : '') + (it.url.length > 96 ? it.url.slice(0, 96) + '…' : it.url);
      const m = document.createElement('div');
      m.className = 'm';
      let label = it.type || '';
      if (label.indexOf('/') !== -1) label = label.split('/')[1];
      label = String(label).split(';')[0];
      const kind = it.kind || 'video';
      const kindLabel = (I18N.kinds && I18N.kinds[kind]) || kind;
      const viaLabel = (I18N.vias && I18N.vias[it.via]) || it.via;
      m.textContent = kindLabel + ' · ' + label + ' · ' + viaLabel;
      div.appendChild(u);
      div.appendChild(m);
      div.onclick = function () {
        const mime = it.type && String(it.type).indexOf('/') !== -1 ? it.type : '';
        report('resolved', { url: it.url, title: document.title || 'media', mime: mime, kind: kind });
      };
      list.appendChild(div);
    });
  };
  scheduleRender();
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', scheduleRender);
  }
})();
"##;

#[cfg(test)]
mod tests {
    use super::{
        classify_uge_url, extension_from_url, extension_is_direct_media, is_candidate,
        is_manifest_url, is_non_media_extension, is_uge_media_url, needs_ytdlp_demux,
        parse_callback_url, safe_uge_filename, UgeMediaClass, DIRECT_MEDIA_EXT, MANIFEST_EXT,
        SEGMENT_EXT, UGE_SCRIPT,
    };
    use tauri::Url;

    #[test]
    fn only_http_pages_are_candidates() {
        assert!(is_candidate("https://example.com/watch/123"));
        assert!(is_candidate("http://example.com/v/1"));
        assert!(!is_candidate("ftp://example.com/v.mp4"));
        assert!(!is_candidate("cobalt://download?url=x"));
        assert!(!is_candidate("not a url"));
    }

    #[test]
    fn extracts_container_extensions() {
        assert_eq!(
            extension_from_url("https://cdn.example.com/a/b/video.mp4?token=1"),
            Some("mp4".to_string())
        );
        assert_eq!(
            extension_from_url("https://cdn.example.com/clip.WEBM"),
            Some("webm".to_string())
        );
        assert_eq!(extension_from_url("https://cdn.example.com/stream"), None);
        assert_eq!(extension_from_url("https://cdn.example.com/a.b.c"), None);
        // A trailing slash must not hide the container.
        assert_eq!(
            extension_from_url("https://cdn.example.com/video.MP4/"),
            Some("mp4".to_string())
        );
    }

    #[test]
    fn builds_safe_filenames() {
        assert_eq!(
            safe_uge_filename("My Video: Part 1/2", "https://cdn.example.com/v.mp4?e=1"),
            "My Video_ Part 1_2.mp4"
        );
        assert_eq!(
            safe_uge_filename("Clip", "https://cdn.example.com/stream"),
            "Clip.mp4"
        );
        assert_eq!(
            safe_uge_filename("", "https://cdn.example.com/audio.m4a"),
            "manual-extract.m4a"
        );
        // Newly surfaced container extensions keep their own suffix.
        assert_eq!(
            safe_uge_filename("Show", "https://cdn.example.com/movie.mk3d"),
            "Show.mk3d"
        );
        assert_eq!(
            safe_uge_filename("Song", "https://cdn.example.com/a.weba"),
            "Song.weba"
        );
    }

    #[test]
    fn manifest_and_segment_urls_are_pickable_media() {
        // The whole point of the UGE upgrade: HLS/DASH/TS are surfaced.
        assert!(is_uge_media_url("https://cdn.example.com/hls/index.m3u8"));
        assert!(is_uge_media_url(
            "https://cdn.example.com/hls/index.m3u8?token=abc"
        ));
        assert!(is_uge_media_url(
            "https://cdn.example.com/dash/manifest.mpd"
        ));
        assert!(is_uge_media_url("https://cdn.example.com/seg/00042.ts"));
        assert!(is_uge_media_url("https://cdn.example.com/v.mp4"));
        // fMP4/DASH init parts + raw segments recovered by later passes.
        assert!(is_uge_media_url(
            "https://cdn.example.com/seg/init.mp4?range=0-712"
        ));
        assert!(is_uge_media_url("https://cdn.example.com/seg/chunk.m4s"));
        assert!(is_uge_media_url("https://cdn.example.com/seg/part.cmfv"));
        assert!(is_uge_media_url("https://cdn.example.com/a.weba"));
        assert!(is_uge_media_url("https://cdn.example.com/movie.mk3d"));
        assert!(is_uge_media_url("https://cdn.example.com/hls/index.m3u"));
        assert!(is_uge_media_url(
            "https://cdn.example.com/live/manifest.f4m"
        ));
        assert!(is_uge_media_url("https://cdn.example.com/clip.avi"));
        assert!(is_uge_media_url("https://cdn.example.com/video.mp4/"));
        // Query-string filenames and format tokens, not just the path.
        assert!(is_uge_media_url(
            "https://cdn.example.com/download?file=clip.mp4"
        ));
        assert!(is_uge_media_url("https://cdn.example.com/live?format=m3u8"));
        assert!(is_uge_media_url(
            "https://cdn.example.com/videoplayback?id=abc&mime=video%2Fmp4"
        ));
        assert!(!is_uge_media_url("https://cdn.example.com/page"));
        assert!(!is_uge_media_url("https://cdn.example.com/api/playlist"));
        assert!(!is_uge_media_url("https://cdn.example.com/media/logo.png"));
        assert!(!is_uge_media_url(
            "https://cdn.example.com/thumb.jpg?format=mp4"
        ));
        assert!(!is_uge_media_url("not a url"));
        assert_eq!(
            classify_uge_url("https://cdn.example.com/live?format=m3u8"),
            Some(UgeMediaClass::Manifest)
        );
        assert_eq!(
            classify_uge_url("https://cdn.example.com/seg/00001.ts"),
            Some(UgeMediaClass::Segment)
        );
        assert!(is_non_media_extension("JPG"));
        assert!(!is_non_media_extension("mp4"));
    }

    #[test]
    fn manifest_urls_are_detected_for_ytdlp_routing() {
        assert!(is_manifest_url("https://cdn.example.com/hls/index.m3u8"));
        assert!(is_manifest_url(
            "https://cdn.example.com/dash/manifest.mpd?token=x"
        ));
        assert!(is_manifest_url("https://cdn.example.com/live?format=hls"));
        assert!(is_manifest_url("https://cdn.example.com/p/master.m3u8/"));
        assert!(!is_manifest_url("https://cdn.example.com/v.mp4"));
        assert!(!is_manifest_url("https://cdn.example.com/seg/00042.ts"));
        assert!(!is_manifest_url("https://cdn.example.com/page"));
        // A blog post that merely discusses the format is not a manifest.
        assert!(!is_manifest_url(
            "https://example.com/blog/what-is-an-m3u8-file"
        ));
    }

    #[test]
    fn direct_media_detection_excludes_manifests_and_segments() {
        // Playable containers pass; manifests/segments need yt-dlp demux.
        assert!(extension_is_direct_media("https://cdn.example.com/v.mp4"));
        assert!(extension_is_direct_media("https://cdn.example.com/a.weba"));
        assert!(extension_is_direct_media(
            "https://cdn.example.com/movie.mk3d"
        ));
        assert!(!extension_is_direct_media(
            "https://cdn.example.com/hls/index.m3u8"
        ));
        assert!(!extension_is_direct_media(
            "https://cdn.example.com/dash/manifest.mpd"
        ));
        assert!(!extension_is_direct_media(
            "https://cdn.example.com/seg/00042.ts"
        ));
        assert!(!extension_is_direct_media(
            "https://cdn.example.com/seg/chunk.m4s"
        ));
        assert!(!extension_is_direct_media("https://cdn.example.com/page"));
    }

    #[test]
    fn manifests_resolve_to_an_mp4_target_not_manifest_text() {
        // An .m3u8/.mpd/.ts stream is demuxed by yt-dlp, so the saved file must
        // not keep the manifest extension (which would just save its text).
        assert_eq!(
            safe_uge_filename("Live show", "https://cdn.example.com/index.m3u8"),
            "Live show.mp4"
        );
        assert_eq!(
            safe_uge_filename("Movie", "https://cdn.example.com/manifest.mpd"),
            "Movie.mp4"
        );
        // A fragment is saved as itself: renaming it to .mp4 makes it unplayable.
        assert_eq!(
            safe_uge_filename("Segment", "https://cdn.example.com/seg/00001.ts"),
            "Segment.ts"
        );
        assert_eq!(
            safe_uge_filename("Chunk", "https://cdn.example.com/seg/chunk.m4s"),
            "Chunk.m4s"
        );
        // A genuine container extension is still preserved.
        assert_eq!(
            safe_uge_filename("Clip", "https://cdn.example.com/v.webm"),
            "Clip.webm"
        );
    }

    #[test]
    fn parses_a_resolved_callback() {
        let callback = Url::parse(
            "cobalt-uge://resolved/?url=https%3A%2F%2Fcdn.example.com%2Fv.mp4%3Fe%3D1&title=A%2FB%3A+C",
        )
        .unwrap();
        let resolved = parse_callback_url(&callback).unwrap();
        assert_eq!(resolved.url, "https://cdn.example.com/v.mp4?e=1");
        assert_eq!(resolved.filename, "A_B_ C.mp4");
        assert_eq!(resolved.media_class, UgeMediaClass::Direct);
        assert!(!needs_ytdlp_demux(&resolved));
    }

    #[test]
    fn extensionless_stream_needs_an_observed_media_mime() {
        let sniffed = Url::parse(
            "cobalt-uge://resolved/?url=https%3A%2F%2Fcdn.example.com%2Fget%3Fid%3D1&title=Show&mime=video%2Fmp4",
        )
        .unwrap();
        let resolved = parse_callback_url(&sniffed).unwrap();
        assert_eq!(resolved.media_class, UgeMediaClass::Direct);
        assert_eq!(resolved.filename, "Show.mp4");

        let playlist = Url::parse(
            "cobalt-uge://resolved/?url=https%3A%2F%2Fcdn.example.com%2Fapi%2Fplaylist&title=Live&mime=application%2Fvnd.apple.mpegurl",
        )
        .unwrap();
        let manifest = parse_callback_url(&playlist).unwrap();
        assert_eq!(manifest.media_class, UgeMediaClass::Manifest);
        assert!(needs_ytdlp_demux(&manifest));

        let html = Url::parse(
            "cobalt-uge://resolved/?url=https%3A%2F%2Fcdn.example.com%2Farticle&title=Hi&mime=text%2Fhtml",
        )
        .unwrap();
        assert_eq!(
            parse_callback_url(&html).unwrap_err(),
            "Manual extraction returned a non-media URL"
        );

        // A jpeg is not promoted by a spoofed video mime.
        let jpeg = Url::parse(
            "cobalt-uge://resolved/?url=https%3A%2F%2Fcdn.example.com%2Fmedia%2Flogo.jpg&mime=video%2Fmp4",
        )
        .unwrap();
        assert!(parse_callback_url(&jpeg).is_err());
    }

    #[test]
    fn injected_script_lists_every_classified_extension() {
        fn listed(ext: &str) -> bool {
            let patterns = [
                format!("({ext}|"),
                format!("|{ext}|"),
                format!("|{ext})"),
                format!(".{ext}(\\"),
            ];
            patterns.iter().any(|pattern| UGE_SCRIPT.contains(pattern))
        }
        for ext in DIRECT_MEDIA_EXT
            .iter()
            .chain(MANIFEST_EXT.iter())
            .chain(SEGMENT_EXT.iter())
        {
            assert!(listed(ext), "injected script is missing .{ext}");
        }
        assert!(UGE_SCRIPT.contains("origOpen.apply"));
        assert!(UGE_SCRIPT.contains("mime: mime"));
        assert!(UGE_SCRIPT.contains("#EXTM3U"));
        assert!(UGE_SCRIPT.contains("if (!variants && sample)"));
        assert!(UGE_SCRIPT.contains("visitRoot"));
        // Broad path tokens were dropping thumbnails into the picker.
        assert!(!UGE_SCRIPT.contains("\\/media\\/"));
        assert!(!UGE_SCRIPT.contains("\\/video\\/"));
        assert!(!UGE_SCRIPT.contains("GENERIC_MEDIA_HINT"));
    }

    #[test]
    fn reports_cancellation_and_rejects_bad_payloads() {
        let cancelled = Url::parse("cobalt-uge://error/?message=Cancelled").unwrap();
        assert_eq!(parse_callback_url(&cancelled).unwrap_err(), "Cancelled");

        let no_url = Url::parse("cobalt-uge://resolved/?title=Hi").unwrap();
        assert!(parse_callback_url(&no_url).is_err());

        let bad_scheme = Url::parse("cobalt-uge://resolved/?url=javascript:alert(1)").unwrap();
        assert!(parse_callback_url(&bad_scheme).is_err());

        // A bare page URL must not slip through as a "media" download.
        let page = Url::parse(
            "cobalt-uge://resolved/?url=https%3A%2F%2Fcdn.example.com%2Farticle&title=Hi",
        )
        .unwrap();
        assert_eq!(
            parse_callback_url(&page).unwrap_err(),
            "Manual extraction returned a non-media URL"
        );
    }

    #[test]
    fn overlay_strings_follow_the_app_locale() {
        assert!(UGE_SCRIPT.contains("I18N.empty"));
        assert!(UGE_SCRIPT.contains("I18N.mse"));
        assert!(UGE_SCRIPT.contains("I18N.title"));
        assert!(!UGE_SCRIPT.contains("No media yet — try playing the video.</div>"));

        let english = super::uge_initialization_script("en");
        assert!(english.contains("window.__cobaltUgeI18n"));
        assert!(english.contains("No media yet"));
        assert!(english.contains("Manual extraction"));

        let chinese = super::uge_overlay_messages("zh-CN");
        assert_eq!(
            chinese.get("empty").and_then(|value| value.as_str()),
            Some("还没有检测到媒体，试着播放视频。")
        );
        let russian = super::uge_overlay_messages("ru");
        assert!(russian
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .contains("найденные"));
        // An unknown locale stays on the English catalog.
        assert_eq!(
            super::uge_overlay_messages("de").get("empty"),
            super::uge_overlay_messages("en").get("empty")
        );
    }
}
