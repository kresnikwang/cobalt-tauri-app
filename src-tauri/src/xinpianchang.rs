use std::time::Duration;

use tauri::{webview::WebviewWindowBuilder, AppHandle, Url, WebviewUrl};

const RESOLVER_SCHEME: &str = "cobalt-xpc";
const RESOLVER_TIMEOUT: Duration = Duration::from_secs(25);

const RESOLVER_SCRIPT: &str = r#"
(() => {
  if (!/(^|\.)xinpianchang\.com$/i.test(window.location.hostname)) return;
  if (window.__cobaltXinpianchangResolver) return;
  window.__cobaltXinpianchangResolver = true;

  let completed = false;
  const report = (kind, values) => {
    if (completed) return;
    completed = true;
    const query = new URLSearchParams(values);
    window.location.replace(`cobalt-xpc://${kind}/?${query.toString()}`);
  };

  const inspect = () => {
    try {
      const nextNode = document.getElementById('__NEXT_DATA__');
      const detail = nextNode
        ? JSON.parse(nextNode.textContent || '{}')?.props?.pageProps?.detail
        : null;
      const video = document.querySelector('video');
      const mediaUrl = video?.currentSrc || video?.src || '';
      if (!detail || !/^https:\/\//i.test(mediaUrl)) return;

      report('resolved', {
        url: mediaUrl,
        title: detail.title || document.title || 'Xinpianchang video',
        articleId: String(detail.id || ''),
        filesize: String(detail.filesize || ''),
        allowDownloadType: String(detail.allow_download_type ?? ''),
        allowDownloadFlag: String(detail.allow_download_flag ?? ''),
      });
    } catch (_) {
      // The page can replace __NEXT_DATA__ while hydrating. Retry until timeout.
    }
  };

  const timer = window.setInterval(inspect, 250);
  window.addEventListener('pagehide', () => window.clearInterval(timer), { once: true });
  window.setTimeout(() => report('error', {
    message: 'The page did not expose an authorized video stream.'
  }), 20000);
  inspect();
})();
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMedia {
    pub url: String,
    pub filename: String,
    pub expected_bytes: u64,
    pub article_id: String,
}

pub fn is_xinpianchang_url(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str().map(|value| value.to_ascii_lowercase()) else {
        return false;
    };
    (host == "xinpianchang.com" || host == "www.xinpianchang.com") && article_id(url).is_some()
}

pub fn article_id(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let mut segments = parsed.path_segments()?.filter(|segment| !segment.is_empty());
    let candidate = segments.next()?;
    if segments.next().is_some()
        || candidate.len() < 2
        || !candidate.starts_with('a')
        || !candidate[1..]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return None;
    }
    Some(candidate.to_string())
}

fn safe_filename(title: &str, article_id: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim();
    let stem = if cleaned.is_empty() {
        format!("xinpianchang_{article_id}")
    } else {
        cleaned.chars().take(120).collect()
    };
    format!("{stem}.mp4")
}

fn parse_callback_url(url: &Url) -> Result<ResolvedMedia, String> {
    if url.scheme() != RESOLVER_SCHEME {
        return Err("Unexpected Xinpianchang resolver callback".into());
    }

    let values: std::collections::HashMap<String, String> =
        url.query_pairs().into_owned().collect();
    match url.host_str() {
        Some("error") => Err(values
            .get("message")
            .cloned()
            .unwrap_or_else(|| "Xinpianchang did not expose a downloadable video".into())),
        Some("resolved") => {
            let media_url = values
                .get("url")
                .filter(|value| value.starts_with("https://"))
                .cloned()
                .ok_or("Xinpianchang returned an invalid media URL")?;
            let media_host = Url::parse(&media_url)
                .ok()
                .and_then(|parsed| parsed.host_str().map(str::to_ascii_lowercase))
                .ok_or("Xinpianchang returned an invalid media URL")?;
            if media_host != "xpccdn.com" && !media_host.ends_with(".xpccdn.com") {
                return Err("Xinpianchang returned an unexpected media host".into());
            }
            let article_id = values
                .get("articleId")
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| "video".into());
            let title = values
                .get("title")
                .map(String::as_str)
                .unwrap_or("Xinpianchang video");
            let expected_bytes = values
                .get("filesize")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            Ok(ResolvedMedia {
                url: media_url,
                filename: safe_filename(title, &article_id),
                expected_bytes,
                article_id,
            })
        }
        _ => Err("Unexpected Xinpianchang resolver callback".into()),
    }
}

pub async fn resolve(app: &AppHandle, url: &str) -> Result<ResolvedMedia, String> {
    if !is_xinpianchang_url(url) {
        return Err("Unsupported Xinpianchang article URL".into());
    }

    let external_url = Url::parse(url).map_err(|error| error.to_string())?;
    let label = format!(
        "xinpianchang-resolver-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let (callback_tx, mut callback_rx) = tokio::sync::mpsc::unbounded_channel::<Url>();
    let navigation_tx = callback_tx.clone();

    let resolver_window =
        WebviewWindowBuilder::new(app, &label, WebviewUrl::External(external_url))
            .title("Resolving Xinpianchang video")
            // WKWebView throttles or suspends page/video initialization in a truly
            // hidden window. Keep it renderable off-screen without stealing focus.
            .inner_size(1280.0, 720.0)
            .position(-10_000.0, -10_000.0)
            .visible(true)
            .focused(false)
            .focusable(false)
            .skip_taskbar(true)
            .initialization_script(RESOLVER_SCRIPT)
            .on_navigation(move |navigation_url| {
                if navigation_url.scheme() == RESOLVER_SCHEME {
                    let _ = navigation_tx.send(navigation_url.clone());
                    return false;
                }
                if navigation_url.scheme() == "about" {
                    return true;
                }
                matches!(
                    navigation_url.host_str(),
                    Some("xinpianchang.com" | "www.xinpianchang.com")
                )
            })
            .build()
            .map_err(|error| format!("Unable to start the Xinpianchang resolver: {error}"))?;

    let callback = tokio::time::timeout(RESOLVER_TIMEOUT, callback_rx.recv()).await;
    let _ = resolver_window.close();

    match callback {
        Ok(Some(callback_url)) => parse_callback_url(&callback_url),
        Ok(None) => Err("The Xinpianchang resolver closed before finding a video".into()),
        Err(_) => Err("Timed out while resolving the Xinpianchang video".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{article_id, is_xinpianchang_url, parse_callback_url, safe_filename};
    use tauri::Url;

    #[test]
    fn recognizes_only_xinpianchang_article_urls() {
        assert!(is_xinpianchang_url(
            "https://www.xinpianchang.com/a13775532?from=IndexPick"
        ));
        assert!(is_xinpianchang_url("https://xinpianchang.com/a1"));
        assert!(is_xinpianchang_url("https://xinpianchang.com/a1/"));
        assert!(!is_xinpianchang_url("ftp://xinpianchang.com/a1"));
        assert!(!is_xinpianchang_url(
            "https://xinpianchang.example/a13775532"
        ));
        assert!(!is_xinpianchang_url(
            "https://evilxinpianchang.com/a13775532"
        ));
        assert!(!is_xinpianchang_url(
            "https://www.xinpianchang.com/discover/article"
        ));
        assert_eq!(
            article_id("https://www.xinpianchang.com/a13775532"),
            Some("a13775532".into())
        );
    }

    #[test]
    fn parses_a_resolved_media_callback() {
        let callback = Url::parse(
            "cobalt-xpc://resolved/?url=https%3A%2F%2Fus-xpc5-l2.xpccdn.com%2Fvideo.mp4%3Fe%3D1%26s%3D2&title=A%2FB%3A+C&articleId=13775532&filesize=116600857",
        )
        .unwrap();
        let media = parse_callback_url(&callback).unwrap();
        assert_eq!(media.url, "https://us-xpc5-l2.xpccdn.com/video.mp4?e=1&s=2");
        assert_eq!(media.filename, "A_B_ C.mp4");
        assert_eq!(media.expected_bytes, 116_600_857);
        assert_eq!(media.article_id, "13775532");
    }

    #[test]
    fn reports_resolver_errors_and_safe_fallback_names() {
        let callback = Url::parse("cobalt-xpc://error/?message=Video+not+available").unwrap();
        assert_eq!(
            parse_callback_url(&callback).unwrap_err(),
            "Video not available"
        );
        assert_eq!(safe_filename(" /:*? ", "13775532"), "____.mp4");
        assert_eq!(safe_filename("", "13775532"), "xinpianchang_13775532.mp4");

        let unexpected_host = Url::parse(
            "cobalt-xpc://resolved/?url=https%3A%2F%2Fevil.example%2Fvideo.mp4&articleId=13775532",
        )
        .unwrap();
        assert_eq!(
            parse_callback_url(&unexpected_host).unwrap_err(),
            "Xinpianchang returned an unexpected media host"
        );
    }
}
