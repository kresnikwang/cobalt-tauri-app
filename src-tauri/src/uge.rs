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

/// Media extensions the picker offers. HLS/DASH manifests (`.m3u8`, `.mpd`) and
/// raw segments (`.ts`) are intentionally excluded: the direct-stream
/// downloader fetches a single URL, so a manifest would just save its text.
const PROGRESSIVE_EXT: &[&str] = &[
    "mp4", "m4v", "webm", "mkv", "mov", "flv", "f4v", "3gp", "mp3", "m4a", "m4b", "aac", "ogg",
    "oga", "opus", "wav", "flac",
];

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

/// Build a safe, readable filename from the page title + media URL.
pub fn safe_uge_filename(title: &str, url: &str) -> String {
    let ext = extension_from_url(url)
        .filter(|ext| PROGRESSIVE_EXT.contains(&ext.as_str()))
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

  const PROGRESSIVE = /\.(mp4|m4v|webm|mkv|mov|flv|f4v|3gp|mp3|m4a|m4b|aac|ogg|oga|opus|wav|flac)(\?|#|$)/i;
  const MEDIA_MIME = /^(video\/|audio\/)/i;
  const found = new Map();

  const guessType = (url) => {
    const clean = url.split('?')[0].split('#')[0];
    const m = clean.match(/\.([a-z0-9]{2,5})$/i);
    return m ? m[1].toLowerCase() : 'media';
  };
  const isMedia = (url, type) => {
    if (!url || !/^https?:/i.test(url)) return false;
    if (type && MEDIA_MIME.test(type)) {
      if (/mpegurl|dash\+xml/i.test(type)) return false;
      return true;
    }
    const clean = url.split('?')[0].split('#')[0];
    return PROGRESSIVE.test(clean);
  };

  const add = (url, type, via) => {
    if (!isMedia(url, type)) return;
    if (found.has(url)) return;
    found.set(url, { url, type: type || guessType(url), via });
    render();
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
            try { add(url, res.headers.get('content-type'), 'network'); } catch (e) {}
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
        try { add(url, this.getResponseHeader('content-type'), 'network'); } catch (e) {}
      });
      return origOpen.call(this, method, url, rest);
    };
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
      if (el.currentSrc) add(el.currentSrc, null, 'player');
      if (el.src) add(el.src, null, 'player');
      el.querySelectorAll('source').forEach(function (s) { if (s.src) add(s.src, s.type, 'player'); });
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
      list.innerHTML = '<div class="none">No media yet — try playing the video.</div>';
      return;
    }
    list.innerHTML = '';
    items.forEach(function (it) {
      const div = document.createElement('div');
      div.className = 'item';
      const u = document.createElement('div');
      u.className = 'u';
      u.textContent = it.url.length > 96 ? it.url.slice(0, 96) + '…' : it.url;
      const m = document.createElement('div');
      m.className = 'm';
      m.textContent = it.type + ' · ' + it.via;
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
    use super::{extension_from_url, is_candidate, parse_callback_url, safe_uge_filename};
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
    }
}
