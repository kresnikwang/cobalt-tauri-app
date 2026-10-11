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

/// Media extensions the picker offers. HLS/DASH manifests (`.m3u8`, `.mpd`) and
/// raw segments (`.ts` + fMP4/init parts, Annex B, raw AAC) are deliberately
/// included as well: although the direct-stream downloader fetches a single
/// URL, surfacing these lets the user hand an m3u8/mpd/ts back to yt-dlp
/// (which natively demuxes them), which is the only way to recover a stream
/// the sniffer can otherwise see but not reconstruct. Kept as a Rust constant
/// so it stays unit-testable in sync with the injected script's own
/// `PROGRESSIVE`/`AUDIO_RE`/`MANIFEST`/`TS_SEGMENT`/`RAW_SEGMENT` regexes.
const PROGRESSIVE_EXT: &[&str] = &[
    "mp4", "m4v", "webm", "mkv", "mk3d", "mks", "mov", "flv", "f4v", "3gp", "3g2", "mp3", "m4a",
    "m4b", "aac", "ogg", "oga", "opus", "wav", "flac", "weba", "wma", "m3u8", "mpd", "ts", "m4s",
    "cmfv", "cmfa", "h264", "265", "avc", "hevc", "adts",
];

/// Directly playable container/audio extensions (a strict subset of
/// `PROGRESSIVE_EXT`, which additionally lists manifests + raw segments that
/// only yt-dlp can demux).
const DIRECT_MEDIA_EXT: &[&str] = &[
    "mp4", "m4v", "webm", "mkv", "mk3d", "mks", "mov", "flv", "f4v", "3gp", "3g2", "mp3", "m4a",
    "m4b", "aac", "ogg", "oga", "opus", "wav", "flac", "weba", "wma",
];

/// Manifest container formats that yt-dlp natively demuxes (and therefore
/// should be routed to it instead of the single-URL direct downloader).
const MANIFEST_EXT: &[&str] = &["m3u8", "mpd"];

/// Is this resolved URL an HLS/DASH manifest that needs yt-dlp to demux?
/// The caller is expected to have resolved `Ok(ResolvedUge)` first; the
/// caller-visible filename still ends in `.mp4`, only the fetcher differs.
pub fn is_manifest_url(url: &str) -> bool {
    extension_from_url(url)
        .map(|ext| MANIFEST_EXT.contains(&ext.as_str()))
        .unwrap_or(false)
}

/// Is this a direct playable media URL (progressive container or audio),
/// as opposed to a manifest/segment that only yt-dlp can demux? Used by the
/// generic-extractor fallback so a bare page scraped by `generic` (no formats,
/// no media extension) is still rejected.
pub fn extension_is_direct_media(url: &str) -> bool {
    extension_from_url(url)
        .map(|ext| DIRECT_MEDIA_EXT.contains(&ext.as_str()))
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct ResolvedUge {
    pub url: String,
    pub filename: String,
    pub expected_bytes: u64,
}

/// Manual extraction only makes sense for a real web page.
pub fn is_candidate(url: &str) -> bool {
    matches!(Url::parse(url).map(|parsed| parsed.scheme().to_owned()), Ok(scheme) if scheme == "http" || scheme == "https")
}

/// Best-effort container extension from a media URL's path (ignoring query).
fn extension_from_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next()?;
    if !last.contains('.') {
        return None;
    }
    let ext = last.rsplit('.').next()?;
    if (2..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(ext.to_ascii_lowercase())
    } else {
        None
    }
}

/// Does a URL carry a media file extension the picker should surface? Mirrors
/// the injected script's `isMedia()` URL branch (progressive **and** HLS/DASH
/// manifests + raw `.ts`/`.m4s`/`.aac` segments) so the Rust-side filename
/// logic and the JS sniffer agree on what counts as media. Unit-tested to
/// keep them in sync.
pub fn is_uge_media_url(url: &str) -> bool {
    extension_from_url(url)
        .map(|ext| PROGRESSIVE_EXT.contains(&ext.as_str()))
        .unwrap_or(false)
}

/// Build a safe, readable filename from the page title + media URL.
///
/// The container extension is taken from the URL only when it is a real
/// *container* (mp4/mkv/…). HLS/DASH manifests (`.m3u8`/`.mpd`) and raw
/// segments (`.ts`) are **not** playable containers on their own — they are
/// handed to yt-dlp, which demuxes them — so those resolve to an `mp4` target
/// rather than saving manifest text.
pub fn safe_uge_filename(title: &str, url: &str) -> String {
    let ext = extension_from_url(url)
        .filter(|ext| DIRECT_MEDIA_EXT.contains(&ext.as_str()))
        .unwrap_or_else(|| "mp4".to_string());
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
            for (key, value) in callback.query_pairs() {
                match key.as_ref() {
                    "url" => media = Some(value.into_owned()),
                    "title" => title = Some(value.into_owned()),
                    _ => {}
                }
            }
            let media = media
                .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
                .ok_or_else(|| "Manual extraction returned no media URL".to_string())?;
            // The picker only offers sniffed media URLs, but a hand-crafted
            // callback (or a future panel bug) could hand back a bare page —
            // fail loudly instead of downloading HTML as ".mp4".
            if !is_uge_media_url(&media) {
                return Err("Manual extraction returned a non-media URL".to_string());
            }
            let filename = safe_uge_filename(title.as_deref().unwrap_or(""), &media);
            Ok(ResolvedUge {
                url: media,
                filename,
                expected_bytes: 0,
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

/// Open a real webview at `url`, inject the media sniffer, and wait for the
/// user to pick a stream (or cancel / time out).
pub async fn start_uge(app_handle: &AppHandle, url: &str) -> Result<ResolvedUge, String> {
    let parsed = Url::parse(url).map_err(|error| format!("Invalid URL: {error}"))?;

    // A previous run may still hold the label; close it so the rebuild succeeds.
    if let Some(existing) = app_handle.get_webview_window("uge") {
        let _ = existing.close();
    }

    let (navigation_tx, mut navigation_rx) = tokio::sync::mpsc::unbounded_channel::<Url>();
    let window = WebviewWindowBuilder::new(app_handle, "uge", WebviewUrl::External(parsed))
        .title("Cobalt · Manual extraction")
        .inner_size(1000.0, 760.0)
        .user_agent(UGE_USER_AGENT)
        .initialization_script(UGE_SCRIPT)
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

  const PROGRESSIVE = /\.(mp4|m4v|webm|mkv|mk3d|mks|mov|flv|f4v|3gp|3g2)(\?|#|$)/i;
  const AUDIO_RE = /\.(mp3|m4a|m4b|aac|ogg|oga|opus|wav|flac|weba|wma)(\?|#|$)/i;
  const MANIFEST = /\.(m3u8|mpd)(\?|#|$)/i;
  // A raw transport-stream segment is almost never the playable asset, but
  // surfacing it gives yt-dlp (which demuxes it) a chance when nothing else
  // is visible. Require a ?/#/$ terminator so query keys like `?ts=123`
  // don't false-positive.
  const TS_SEGMENT = /\.ts(\?|#|$)/i;
  // Supplementary raw formats worth a second-class slot: raw H.264 Annex B,
  // fragmented MP4 init/segment parts, and raw AAC/ADTS.
  const RAW_SEGMENT = /\.(m4s|cmfv|cmfa|h264|265|avc|hevc|aac|adts)(\?|#|$)/i;
  const GENERIC_MEDIA_HINT = /\b(m3u8|mpd|mime=video|mime=audio|videoplayback|\/video\/|\/audio\/|\/media\/|get_video|get_audio|playurl|play_url|stream)/i;
  const ABSOLUTE = /^https?:/i;
  const found = new Map();
  // Browsers hide the real stream behind blob:/MediaSource URLs; remembering
  // which media elements hit that wall lets the picker say so explicitly
  // instead of showing "No media yet" while a video is visibly playing.
  const BLOB_ELEMENTS = new Set();

  const isManifestType = (type) => {
    if (!type) return false;
    return /mpegurl|dash\+xml/i.test(String(type));
  };

  const resolveUrl = (base, u) => {
    if (!u) return '';
    u = String(u).trim();
    if (ABSOLUTE.test(u)) return u;
    try { return new URL(u, base).href; } catch (e) { return ''; }
  };
  const guessType = (url) => {
    const clean = url.split('?')[0].split('#')[0];
    const m = clean.match(/\.([a-z0-9]{2,5})$/i);
    return m ? m[1].toLowerCase() : 'media';
  };
  const isMedia = (url, type) => {
    if (!url || !ABSOLUTE.test(url)) return false;
    if (isManifestType(type)) return true;             // HLS/DASH manifest
    if (type) {
      const t = String(type).toLowerCase();
      if (/^(video|audio)\//.test(t)) return true;     // any real media mime
    }
    const clean = url.split('?')[0].split('#')[0];
    return PROGRESSIVE.test(clean) || AUDIO_RE.test(clean) || MANIFEST.test(clean) || TS_SEGMENT.test(clean) || RAW_SEGMENT.test(clean);
  };

  // Higher-priority provenance wins so a URL first seen as a bare network fetch
  // gets upgraded when the player actually plays it or a manifest references it.
  const viaRank = { player: 3, body: 2, resource: 1, network: 1 };
  const kindOf = (url, type) => {
    if (isManifestType(type) || MANIFEST.test(url)) return 'manifest';
    if (AUDIO_RE.test(url)) return 'audio';
    if (TS_SEGMENT.test(url) || RAW_SEGMENT.test(url)) return 'segment';
    return 'video';
  };
  const add = (url, type, via) => {
    if (!isMedia(url, type)) return;
    const realMime = type && String(type).indexOf('/') !== -1;
    const existing = found.get(url);
    if (existing) {
      if (realMime) existing.type = type;                  // guess -> real mime
      if (viaRank[via] > viaRank[existing.via]) existing.via = via;
      return;
    }
    found.set(url, {
      url,
      type: realMime ? type : guessType(url),
      via,
    });
    render();
  };

  // Pull media URLs out of an m3u8/mpd/JSON response body (relative paths
  // resolved against the response URL). This is how we recover the real
  // playlist/segment URLs when a site fetches an extension-less API endpoint.
  // JSON bodies are parsed structurally (preferred: no regex noise); plain
  // text falls back to a bounded absolute-URL scan.
  const JSON_URL_KEYS = ['url','src','source','file','stream_url','play_url','video_url','audio_url','hls_url','dash_url','manifest','playlist'];
  const scanJsonValue = (value, base, depth) => {
    if (value == null || depth > 4) return;
    if (typeof value === 'string') {
      const s = value.trim();
      if (s.length > 16 && s.length < 4096 && ABSOLUTE.test(s)) pickGeneric(s, 'network');
      return;
    }
    if (Array.isArray(value)) {
      for (let i = 0; i < Math.min(value.length, 40); i++) scanJsonValue(value[i], base, depth + 1);
      return;
    }
    if (typeof value === 'object') {
      const keys = Object.keys(value);
      for (let i = 0; i < Math.min(keys.length, 60); i++) {
        const raw = value[keys[i]];
        if (typeof raw === 'string') {
          const s = raw.trim();
          // Prefer known player-config keys; anything else must carry a media
          // extension hint. This keeps config/thumbnail noise out of the list.
          const k = keys[i].toLowerCase();
          if (s.length > 16 && s.length < 4096 && ABSOLUTE.test(s)
              && (JSON_URL_KEYS.indexOf(k) >= 0 || /\.(mp4|m4v|webm|mkv|mov|m3u8|mpd|ts|mp3|m4a|aac|ogg|oga|opus|wav|flac)([?#]|$)/i.test(s))) {
            pickGeneric(s, 'network');
          }
        } else {
          scanJsonValue(raw, base, depth + 1);
        }
      }
    }
  };
  const pickGeneric = (u, source) => {
    const url = resolveUrl(location.href, u);
    if (!url || found.has(url)) return;
    const clean = url.split('?')[0].split('#')[0];
    if (PROGRESSIVE.test(clean) || AUDIO_RE.test(clean) || MANIFEST.test(clean) || TS_SEGMENT.test(clean) || RAW_SEGMENT.test(clean)) {
      add(url, null, source);
    } else if (GENERIC_MEDIA_HINT.test(url)) {
      add(url, null, source);
    }
  };
  const scanTextForUrls = (text, base) => {
    if (!text) return;
    if (typeof text === 'string') {
      let parsed = null;
      try { parsed = JSON.parse(text); } catch (e) { parsed = null; }
      if (parsed && typeof parsed === 'object') { scanJsonValue(parsed, base, 0); return; }
      if (text.length > 512000) return;
    }
    String(text).split(/\r?\n/).forEach(function (line) {
      if (!line || line.charAt(0) === '#' || line.charAt(0) === '<') return;
      add(resolveUrl(base, line), null, 'body');
    });
    const re = /https?:\/\/[^\s"'<>\\]+/gi;
    let m;
    let guard = 0;
    while ((m = re.exec(text)) && guard++ < 60) pickGeneric(m[0], 'body');
  };

  // Rank so the best/clearest stream surfaces first: a progressive container
  // beats a manifest beats a raw segment; something the player used beats a
  // background fetch; a higher resolution hint in the URL breaks ties.
  const scoreOf = (it) => {
    const clean = it.url.split('?')[0];
    let s = 0;
    if (isManifestType(it.type) || MANIFEST.test(clean)) s += 40;
    // Raw segments are last-resort crumbs: surface them, but never above a
    // playable container or manifest.
    else if (TS_SEGMENT.test(clean) || RAW_SEGMENT.test(clean)) s += 5;
    else s += 60;                                          // progressive container
    if (it.via === 'player') s += 30;
    else if (it.via === 'body') s += 10;
    const h = clean.match(/(\d{3,4})[pP]/);
    if (h) s += Math.min(parseInt(h[1], 10) || 0, 4000) / 100;
    return s;
  };

  let report = (kind, params) => {
    const query = new URLSearchParams(params).toString();
    window.location.replace('cobalt-uge://' + kind + '/?' + query);
  };

  try {
    const origFetch = window.fetch;
    window.fetch = function () {
      const args = Array.prototype.slice.call(arguments);
      try {
        const input = args[0];
        const url = typeof input === 'string' ? input : (input && input.url) || '';
        add(url, null, 'network');
        const result = origFetch.apply(this, args);
        if (result && result.then) {
          result.then(function (res) {
            try {
              const ct = res.headers.get('content-type');
              add(url, ct, 'network');
              // HLS/DASH manifests are often fetched from extension-less API
              // endpoints, so scan the body for the real playlist/segment URLs.
              if (/mpegurl|dash\+xml|json|text\/plain/i.test(String(ct))) {
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
      const rest = Array.prototype.slice.call(arguments, 3);
      add(url, null, 'network');
      this.addEventListener('load', function () {
        try {
          const ct = this.getResponseHeader('content-type');
          add(url, ct, 'network');
          if (/mpegurl|dash\+xml|json|text\/plain/i.test(String(ct))) {
            scanTextForUrls(this.responseText, url);
          }
        } catch (e) {}
      });
      return origOpen.call(this, method, url, rest);
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
      list.getEntries().forEach(function (entry) { add(entry.name, null, 'resource'); });
    });
    po.observe({ type: 'resource', buffered: true });
    performance.getEntriesByType('resource').forEach(function (e) { add(e.name, null, 'resource'); });
  } catch (e) {}

  const scanMedia = function () {
    document.querySelectorAll('video,audio').forEach(function (el) {
      if (el.currentSrc) {
        if (/^blob:/i.test(el.currentSrc)) BLOB_ELEMENTS.add(el);
        else add(el.currentSrc, null, 'player');
      }
      if (el.src && el.src !== el.currentSrc) {
        if (/^blob:/i.test(el.src)) BLOB_ELEMENTS.add(el);
        else add(el.src, null, 'player');
      }
      el.querySelectorAll('source').forEach(function (s) { if (s.src) add(s.src, s.type, 'player'); });
      // <track> cues (WebVTT) often share the media prefix — an extra crumb.
      el.querySelectorAll('track').forEach(function (t) { if (t.src) pickGeneric(t.src, 'player'); });
    });
    // Scan open shadow roots once per pass: several players bury their
    // <video> inside a web component.
    document.querySelectorAll('*').forEach(function (host) {
      let root = null;
      try { root = host.shadowRoot; } catch (e) { root = null; }
      if (!root) return;
      root.querySelectorAll('video,audio').forEach(function (el) {
        if (el.currentSrc && !/^blob:/i.test(el.currentSrc)) add(el.currentSrc, null, 'player');
        if (el.src && el.src !== el.currentSrc && !/^blob:/i.test(el.src)) add(el.src, null, 'player');
        el.querySelectorAll('source').forEach(function (s) { if (s.src) add(s.src, s.type, 'player'); });
      });
    });
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
      '<div class="h"><span>Cobalt · detected media</span><button id="__cobalt_uge_close">✕</button></div>' +
      '<div id="__cobalt_uge_list"></div>' +
      '</div>';
    document.documentElement.appendChild(host);
    shadow.getElementById('__cobalt_uge_close').onclick = function () {
      report('error', { message: 'Cancelled' });
    };
    return shadow;
  };

  const render = function () {
    const shadow = ensurePanel();
    const list = shadow.getElementById('__cobalt_uge_list');
    const items = Array.from(found.values());
    if (!items.length) {
      const blobCount = (typeof BLOB_ELEMENTS !== 'undefined' && BLOB_ELEMENTS.size) || 0;
      const mseCount = (typeof BLOB_MIMES !== 'undefined' && BLOB_MIMES.length) || 0;
      if (blobCount > 0 || mseCount > 0) {
        list.innerHTML = '<div class="none">This player streams inside the browser (MSE/blob) — no direct file URL is visible. Try playing the video, or pick a manifest/segment below for yt-dlp.</div>';
      } else {
        list.innerHTML = '<div class="none">No media yet — try playing the video.</div>';
      }
      return;
    }
    // Best stream first so the user usually only needs the top entry.
    items.sort(function (a, b) { return scoreOf(b) - scoreOf(a); });
    list.innerHTML = '';
    items.forEach(function (it, i) {
      const div = document.createElement('div');
      div.className = 'item';
      const u = document.createElement('div');
      u.className = 'u';
      u.textContent = (i === 0 ? '★ ' : '') + (it.url.length > 96 ? it.url.slice(0, 96) + '…' : it.url);
      const m = document.createElement('div');
      m.className = 'm';
      const label = it.type && it.type.indexOf('/') !== -1 ? it.type.split('/')[1] : it.type;
      m.textContent = label + ' · ' + it.via;
      div.appendChild(u);
      div.appendChild(m);
      div.onclick = function () {
        report('resolved', { url: it.url, title: document.title || 'media' });
      };
      list.appendChild(div);
    });
  };
})();
"##;

#[cfg(test)]
mod tests {
    use super::{
        extension_from_url, extension_is_direct_media, is_candidate, is_manifest_url,
        is_uge_media_url, parse_callback_url, safe_uge_filename,
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
        assert!(is_uge_media_url("https://cdn.example.com/hls/index.m3u8?token=abc"));
        assert!(is_uge_media_url("https://cdn.example.com/dash/manifest.mpd"));
        assert!(is_uge_media_url("https://cdn.example.com/seg/00042.ts"));
        assert!(is_uge_media_url("https://cdn.example.com/v.mp4"));
        // fMP4/DASH init parts + raw segments recovered by later passes.
        assert!(is_uge_media_url("https://cdn.example.com/seg/init.mp4?range=0-712"));
        assert!(is_uge_media_url("https://cdn.example.com/seg/chunk.m4s"));
        assert!(is_uge_media_url("https://cdn.example.com/seg/part.cmfv"));
        assert!(is_uge_media_url("https://cdn.example.com/a.weba"));
        assert!(is_uge_media_url("https://cdn.example.com/movie.mk3d"));
        assert!(!is_uge_media_url("https://cdn.example.com/page"));
        assert!(!is_uge_media_url("https://cdn.example.com/api/playlist"));
        assert!(!is_uge_media_url("not a url"));
    }

    #[test]
    fn manifest_urls_are_detected_for_ytdlp_routing() {
        assert!(is_manifest_url("https://cdn.example.com/hls/index.m3u8"));
        assert!(is_manifest_url("https://cdn.example.com/dash/manifest.mpd?token=x"));
        assert!(!is_manifest_url("https://cdn.example.com/v.mp4"));
        assert!(!is_manifest_url("https://cdn.example.com/seg/00042.ts"));
        assert!(!is_manifest_url("https://cdn.example.com/page"));
    }

    #[test]
    fn direct_media_detection_excludes_manifests_and_segments() {
        // Playable containers pass; manifests/segments need yt-dlp demux.
        assert!(extension_is_direct_media("https://cdn.example.com/v.mp4"));
        assert!(extension_is_direct_media("https://cdn.example.com/a.weba"));
        assert!(extension_is_direct_media("https://cdn.example.com/movie.mk3d"));
        assert!(!extension_is_direct_media("https://cdn.example.com/hls/index.m3u8"));
        assert!(!extension_is_direct_media("https://cdn.example.com/dash/manifest.mpd"));
        assert!(!extension_is_direct_media("https://cdn.example.com/seg/00042.ts"));
        assert!(!extension_is_direct_media("https://cdn.example.com/seg/chunk.m4s"));
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
        assert_eq!(
            safe_uge_filename("Segment", "https://cdn.example.com/seg/00001.ts"),
            "Segment.mp4"
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
}
