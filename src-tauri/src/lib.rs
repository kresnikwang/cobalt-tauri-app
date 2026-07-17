use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use serde::{Serialize, Deserialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use futures_util::StreamExt;
use tauri::{Manager, Emitter};
use tauri::path::BaseDirectory;

fn default_api_url() -> String {
    "http://43.156.122.169".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub save_path: String,
    #[serde(default = "default_api_url")]
    pub api_url: String,
    pub download_mode: String,
    pub video_quality: String,
    pub audio_format: String,
    pub clipboard_monitoring: bool,
    pub max_parallel_downloads: u32,
    pub proxy_enabled: bool,
    pub proxy_url: String,
}

impl Settings {
    fn default_with_download_dir(download_dir: PathBuf) -> Self {
        Self {
            save_path: download_dir.to_string_lossy().into_owned(),
            api_url: default_api_url(),
            download_mode: "video".to_string(),
            video_quality: "720".to_string(),
            audio_format: "best".to_string(),
            clipboard_monitoring: true,
            max_parallel_downloads: 3,
            proxy_enabled: true,
            proxy_url: "http://127.0.0.1:7897".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub title: String,
    pub status: String, // "queued" | "analyzing" | "downloading" | "merging" | "completed" | "failed" | "cancelled"
    pub progress: f64,       // 0–1
    pub speed: String,          // e.g. "4.5 MB/s"
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub eta: String,
    pub error: Option<String>,
    pub output_path: Option<String>,
}

fn save_tasks(tasks: &HashMap<String, DownloadTask>, path: &std::path::Path) {
    if let Ok(content) = serde_json::to_string_pretty(tasks) {
        let _ = std::fs::write(path, content);
    }
}

pub struct AppState {
    pub settings: Settings,
    pub tasks: HashMap<String, DownloadTask>,
    pub cancellations: HashMap<String, tokio::sync::oneshot::Sender<()>>,
    pub settings_path: PathBuf,
    pub tasks_path: PathBuf,
}

// -----------------------------------------------------------
// Helpers
// -----------------------------------------------------------
fn format_bytes(bytes: f64) -> String {
    let k: f64 = 1024.0;
    let sizes = ["B", "KB", "MB", "GB", "TB"];
    if bytes <= 0.0 {
        return "0 B".to_string();
    }
    let i = (bytes.ln() / k.ln()).floor() as usize;
    let i = std::cmp::min(i, sizes.len() - 1);
    format!("{:.2} {}", bytes / k.powi(i as i32), sizes[i])
}

fn is_youtube_url(url: &str) -> bool {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return host == "youtu.be" || host.ends_with(".youtube.com") || host == "youtube.com";
        }
    }
    false
}

fn is_bilibili_url(url: &str) -> bool {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if let Some(host) = parsed.host_str().map(|h| h.trim_start_matches("www.").to_ascii_lowercase()) {
            return host == "bilibili.com"
                || host.ends_with(".bilibili.com")
                || host == "b23.tv";
        }
    }
    false
}

fn is_dailymotion_url(url: &str) -> bool {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if let Some(host) = parsed.host_str().map(|h| h.trim_start_matches("www.").to_ascii_lowercase()) {
            return host == "dailymotion.com"
                || host.ends_with(".dailymotion.com")
                || host == "dai.ly";
        }
    }
    false
}

fn is_local_ytdlp_url(url: &str) -> bool {
    is_youtube_url(url) || is_bilibili_url(url) || is_dailymotion_url(url)
}

fn should_relay_download_through_server(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return true;
    };
    let Some(host) = parsed.host_str().map(|h| h.trim_start_matches("www.").to_ascii_lowercase()) else {
        return true;
    };

    // These services commonly use signed ranges, HLS/DASH fragments, bot checks, or
    // CDN rules that are more reliable when the same server both resolves and fetches.
    host == "youtu.be"
        || host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "bilibili.com"
        || host.ends_with(".bilibili.com")
        || host == "b23.tv"
        || host == "instagram.com"
        || host.ends_with(".instagram.com")
        || host == "ddinstagram.com"
        || host == "twitter.com"
        || host.ends_with(".twitter.com")
        || host == "x.com"
        || host.ends_with(".x.com")
        || host == "vxtwitter.com"
        || host == "fixvx.com"
        || host == "dailymotion.com"
        || host.ends_with(".dailymotion.com")
        || host == "dai.ly"
}

fn normalized_api_url(settings: &Settings) -> String {
    let trimmed = settings.api_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        default_api_url()
    } else {
        trimmed.to_string()
    }
}

fn absolute_api_url(settings: &Settings, path_or_url: &str) -> String {
    if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        path_or_url.to_string()
    } else {
        format!("{}{}", normalized_api_url(settings), path_or_url)
    }
}

async fn request_media_service(payload: serde_json::Value, settings: &Settings, client: &reqwest::Client) -> Result<serde_json::Value, reqwest::Error> {
    let res = client.post(normalized_api_url(settings))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;
        
    res.json::<serde_json::Value>().await
}

fn build_media_service_payload(
    url: &str,
    settings: &Settings,
    youtube_hls: bool,
    innertube_client: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "url": url,
        "videoQuality": settings.video_quality,
        "downloadMode": if settings.download_mode == "video" { "auto" } else { "audio" },
        "audioFormat": settings.audio_format,
        "filenameStyle": "pretty",
        "youtubeVideoCodec": "h264",
        "youtubeHLS": youtube_hls,
        "youtubeBetterAudio": true,
        "localProcessing": "disabled",
        "alwaysProxy": should_relay_download_through_server(url),
    });

    if let Some(client) = innertube_client {
        payload["innertubeClient"] = serde_json::Value::String(client.to_string());
    }

    payload
}

fn youtube_video_id(url: &str) -> Option<String> {
    if let Some(pos) = url.find("v=") {
        let value = &url[pos + 2..];
        let id = value
            .split(|c| c == '&' || c == '#' || c == '/')
            .next()
            .unwrap_or("");
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }

    if let Some(pos) = url.find("youtu.be/") {
        let value = &url[pos + "youtu.be/".len()..];
        let id = value
            .split(|c| c == '?' || c == '#' || c == '/')
            .next()
            .unwrap_or("");
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }

    None
}

fn bilibili_video_id(url: &str) -> Option<String> {
    for marker in ["BV", "av"] {
        if let Some(pos) = url.find(marker) {
            let value = &url[pos..];
            let id = value
                .split(|c: char| c == '?' || c == '#' || c == '/' || c == '&' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if id.len() > marker.len() {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn dailymotion_video_id(url: &str) -> Option<String> {
    if let Some(pos) = url.find("/video/") {
        let value = &url[pos + "/video/".len()..];
        let id = value
            .split(|c: char| c == '?' || c == '#' || c == '/' || c == '_' || c.is_whitespace())
            .next()
            .unwrap_or("");
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }

    if let Some(pos) = url.find("dai.ly/") {
        let value = &url[pos + "dai.ly/".len()..];
        let id = value
            .split(|c: char| c == '?' || c == '#' || c == '/' || c.is_whitespace())
            .next()
            .unwrap_or("");
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }

    None
}

fn unique_output_path(save_dir: &std::path::Path, filename: &str) -> PathBuf {
    let safe_filename = std::path::Path::new(filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let mut output_path = save_dir.join(safe_filename);
    let ext = output_path.extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{}", s))
        .unwrap_or_else(|| "".to_string());
    let base = output_path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(safe_filename)
        .to_string();

    let mut dup_index = 1;
    while output_path.exists() {
        output_path = save_dir.join(format!("{} ({}){}", base, dup_index, ext));
        dup_index += 1;
    }

    output_path
}

const YTDLP_MEDIA_EXTS: &[&str] = &[
    "mkv", "mp4", "webm", "m4a", "mp3", "opus", "ogg", "wav", "flac", "aac",
];

fn ytdlp_stem_has_output(save_dir: &std::path::Path, stem: &str) -> bool {
    YTDLP_MEDIA_EXTS.iter().any(|ext| save_dir.join(format!("{}.{}", stem, ext)).exists())
}

/// Pick a collision-free basename for yt-dlp's `%(ext)s` template.
fn unique_ytdlp_stem(save_dir: &std::path::Path, base: &str) -> String {
    let safe_base = std::path::Path::new(base)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(base)
        .to_string();
    let mut stem = safe_base.clone();
    let mut dup_index = 1;
    while ytdlp_stem_has_output(save_dir, &stem) {
        stem = format!("{} ({})", safe_base, dup_index);
        dup_index += 1;
    }
    stem
}

fn resolve_ytdlp_output(save_dir: &std::path::Path, stem: &str) -> Option<PathBuf> {
    YTDLP_MEDIA_EXTS
        .iter()
        .map(|ext| save_dir.join(format!("{}.{}", stem, ext)))
        .find(|path| {
            path.exists()
                && std::fs::metadata(path)
                    .map(|meta| meta.len() > 0)
                    .unwrap_or(false)
        })
}

fn remove_ytdlp_outputs(save_dir: &std::path::Path, stem: &str) {
    let prefix = format!("{}.", stem);
    if let Ok(entries) = std::fs::read_dir(save_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // Final file (stem.ext) or yt-dlp intermediates (stem.f137.mp4, etc.).
            if name == stem || name.starts_with(&prefix) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

fn resolve_ytdlp_path(app_handle: &tauri::AppHandle) -> PathBuf {
    let candidates = [
        app_handle.path().resolve("binaries/yt-dlp", BaseDirectory::Resource).ok(),
        Some(PathBuf::from("src-tauri/binaries/yt-dlp")),
        Some(PathBuf::from("/opt/homebrew/bin/yt-dlp")),
        Some(PathBuf::from("/usr/local/bin/yt-dlp")),
        Some(PathBuf::from("yt-dlp")),
    ];

    candidates
        .into_iter()
        .flatten()
        .find(|path| path.exists() || path.to_string_lossy() == "yt-dlp")
        .unwrap_or_else(|| PathBuf::from("yt-dlp"))
}

fn resolve_node_path(app_handle: &tauri::AppHandle) -> Option<PathBuf> {
    [
        app_handle.path().resolve("binaries/node", BaseDirectory::Resource).ok(),
        Some(PathBuf::from("src-tauri/binaries/node")),
        Some(PathBuf::from("/usr/local/bin/node")),
        Some(PathBuf::from("/opt/homebrew/bin/node")),
    ]
    .into_iter()
    .flatten()
    .find(|path| path.exists())
}

fn resolve_ffmpeg_path(app_handle: &tauri::AppHandle) -> Option<PathBuf> {
    [
        app_handle.path().resolve("binaries/ffmpeg", BaseDirectory::Resource).ok(),
        Some(PathBuf::from("src-tauri/binaries/ffmpeg")),
        Some(PathBuf::from("/opt/homebrew/bin/ffmpeg")),
        Some(PathBuf::from("/usr/local/bin/ffmpeg")),
    ]
    .into_iter()
    .flatten()
    .find(|path| path.exists())
}

fn chrome_cookie_sources() -> Vec<String> {
    let mut sources = vec!["chrome:Default".to_string()];
    if let Some(home) = std::env::var_os("HOME") {
        let chrome_root = PathBuf::from(home).join("Library/Application Support/Google/Chrome");
        if let Ok(entries) = std::fs::read_dir(chrome_root) {
            let mut profiles: Vec<String> = entries
                .flatten()
                .filter_map(|entry| {
                    let file_type = entry.file_type().ok()?;
                    if !file_type.is_dir() {
                        return None;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with("Profile ") {
                        Some(format!("chrome:{}", name))
                    } else {
                        None
                    }
                })
                .collect();
            profiles.sort();
            sources.extend(profiles);
        }
    }
    sources.push("chrome".to_string());
    sources.dedup();
    sources
}

fn ytdlp_audio_format(settings: &Settings) -> String {
    match settings.audio_format.as_str() {
        "mp3" | "ogg" | "wav" | "opus" | "m4a" => settings.audio_format.clone(),
        _ => "m4a".to_string(),
    }
}

fn ytdlp_format(settings: &Settings) -> String {
    if settings.download_mode == "audio" {
        return "bestaudio/best".to_string();
    }

    // Prefer H.264 + AAC so macOS QuickTime / Finder can play natively (mp4).
    // Fall back to any adaptive best (VP9/AV1 + opus) when H.264 is unavailable.
    // bestvideo* allows formats yt-dlp would otherwise deprioritize.
    match settings.video_quality.as_str() {
        "max" => {
            "bestvideo*[vcodec^=avc1]+bestaudio[acodec^=mp4a]/bestvideo*[vcodec^=avc1]+bestaudio/bestvideo*+bestaudio/best"
                .to_string()
        }
        quality => format!(
            "bestvideo*[vcodec^=avc1][height<={quality}]+bestaudio[acodec^=mp4a]/bestvideo*[vcodec^=avc1][height<={quality}]+bestaudio/bestvideo*[height<={quality}]+bestaudio/best[height<={quality}]/best"
        ),
    }
}

fn ytdlp_error_text(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("sign in to confirm") || lower.contains("not a bot") {
        return "YouTube requires local browser cookies. Please log in to YouTube in Chrome/Safari and try again.".to_string();
    }
    if lower.contains("cookies are no longer valid") {
        return "Local YouTube cookies are expired. Please refresh browser login and try again.".to_string();
    }
    if lower.contains("http error 412") || lower.contains("precondition failed") {
        return "Bilibili rejected this request with HTTP 412. Please open Bilibili in Chrome, refresh the page, then retry.".to_string();
    }
    if lower.contains("login") || lower.contains("not logged in") || lower.contains("请先登录") || lower.contains("登录") {
        return "Bilibili login cookies were not accepted. Please make sure Bilibili is logged in with the active Chrome profile.".to_string();
    }
    stderr.lines()
        .rev()
        .find(|line| line.contains("ERROR:"))
        .or_else(|| stderr.lines().rev().find(|line| !line.trim().is_empty()))
        .unwrap_or("Local yt-dlp download failed")
        .trim()
        .to_string()
}

#[derive(Debug, Clone)]
struct YtdlpProgress {
    progress: f64,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed: Option<String>,
    eta: Option<String>,
}

fn parse_size_to_bytes(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
        return None;
    }

    let number_part: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let unit_part = trimmed[number_part.len()..]
        .trim()
        .trim_end_matches("/s")
        .trim()
        .to_ascii_lowercase();

    let number = number_part.parse::<f64>().ok()?;
    let multiplier = match unit_part.as_str() {
        "" | "b" | "bytes" => 1.0,
        "kib" | "kb" | "k" => 1024.0,
        "mib" | "mb" | "m" => 1024.0_f64.powi(2),
        "gib" | "gb" | "g" => 1024.0_f64.powi(3),
        "tib" | "tb" | "t" => 1024.0_f64.powi(4),
        _ => return None,
    };

    Some((number * multiplier).round() as u64)
}

fn parse_ytdlp_progress_line(line: &str) -> Option<YtdlpProgress> {
    let compact = line.replace('\r', "");
    let text = compact.trim();
    if !text.contains("[download]") || !text.contains('%') {
        return None;
    }

    let percent_end = text.find('%')?;
    let percent_start = text[..percent_end]
        .rfind(|c: char| c.is_whitespace())
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let percent = text[percent_start..percent_end].trim().parse::<f64>().ok()?;
    let progress = (percent / 100.0).clamp(0.0, 0.99);

    let total_bytes = if let Some(of_pos) = text.find(" of ") {
        let after_of = &text[of_pos + 4..];
        let size_token = after_of
            .split_whitespace()
            .next()
            .unwrap_or("");
        parse_size_to_bytes(size_token).unwrap_or(0)
    } else {
        0
    };

    let downloaded_bytes = if total_bytes > 0 {
        ((total_bytes as f64) * progress).round() as u64
    } else {
        0
    };

    let speed = text
        .split(" at ")
        .nth(1)
        .and_then(|part| part.split(" ETA ").next())
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else if trimmed.ends_with("/s") {
                Some(trimmed.to_string())
            } else {
                Some(format!("{}/s", trimmed))
            }
        });

    let eta = text
        .split(" ETA ")
        .nth(1)
        .and_then(|value| value.split_whitespace().next())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Some(YtdlpProgress {
        progress,
        downloaded_bytes,
        total_bytes,
        speed,
        eta,
    })
}

async fn try_local_ytdlp_download(
    id: String,
    url: &str,
    settings: &Settings,
    state: &Arc<Mutex<AppState>>,
    app_handle: &tauri::AppHandle,
) -> Result<bool, String> {
    if !is_local_ytdlp_url(url) {
        return Ok(false);
    }

    let is_youtube = is_youtube_url(url);
    let is_bilibili = is_bilibili_url(url);
    let source_name = if is_youtube {
        "YouTube"
    } else if is_bilibili {
        "Bilibili"
    } else {
        "Dailymotion"
    };
    let source_prefix = if is_youtube {
        "youtube"
    } else if is_bilibili {
        "bilibili"
    } else {
        "dailymotion"
    };
    let video_id = if is_youtube {
        youtube_video_id(url)
    } else if is_bilibili {
        bilibili_video_id(url)
    } else {
        dailymotion_video_id(url)
    }.unwrap_or_else(|| id.clone());
    let audio_format = ytdlp_audio_format(settings);
    let save_path = PathBuf::from(&settings.save_path);
    if !save_path.exists() {
        std::fs::create_dir_all(&save_path).ok();
    }

    // Use yt-dlp's %(ext)s so best-quality AV1/VP9 merges can land as mkv/webm, not forced .mp4.
    let output_stem = unique_ytdlp_stem(
        &save_path,
        &format!("{}_{}", source_prefix, video_id),
    );
    let output_template = save_path.join(format!("{}.%(ext)s", output_stem));
    let provisional_filename = if settings.download_mode == "audio" {
        format!("{}.{}", output_stem, audio_format)
    } else {
        // Prefer mp4 when H.264+AAC is available; %(ext)s may still resolve to mkv/webm.
        format!("{}.mp4", output_stem)
    };
    let provisional_path = save_path.join(&provisional_filename);

    let mut abort_rx = {
        let mut state_lock = state.lock().unwrap();
        if let Some(task) = state_lock.tasks.get_mut(&id) {
            task.title = provisional_filename.clone();
            task.status = "downloading".to_string();
            task.output_path = Some(provisional_path.to_string_lossy().into_owned());
            task.total_bytes = 0;
            task.progress = 0.0;
            task.speed = format!("Local {}", source_name);
            task.eta = "--:--".to_string();
            let _ = app_handle.emit("task-updated", task.clone());
            save_tasks(&state_lock.tasks, &state_lock.tasks_path);
        }

        let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
        state_lock.cancellations.insert(id.clone(), abort_tx);
        abort_rx
    };

    let ytdlp_path = resolve_ytdlp_path(app_handle);
    let node_path = resolve_node_path(app_handle);
    let ffmpeg_path = resolve_ffmpeg_path(app_handle);
    let format = ytdlp_format(settings);
    let cookie_attempts: Vec<Option<String>> = if is_youtube {
        let mut sources = vec![None];
        sources.extend(chrome_cookie_sources().into_iter().map(Some));
        sources.extend([Some("safari".to_string()), Some("firefox".to_string())]);
        sources
    } else if is_bilibili {
        let mut sources: Vec<Option<String>> = chrome_cookie_sources().into_iter().map(Some).collect();
        sources.extend([Some("safari".to_string()), Some("firefox".to_string()), None]);
        sources
    } else {
        let mut sources = vec![None];
        sources.extend(chrome_cookie_sources().into_iter().map(Some));
        sources.extend([Some("safari".to_string()), Some("firefox".to_string())]);
        sources
    };
    let mut last_error = String::from("Local yt-dlp download failed");

    for browser in cookie_attempts {
        remove_ytdlp_outputs(&save_path, &output_stem);

        let mut cmd = TokioCommand::new(&ytdlp_path);
        cmd.arg("--no-playlist")
            .arg("--newline")
            .arg("--no-part")
            .arg("-f")
            .arg(&format)
            .arg("-o")
            .arg(&output_template)
            .arg(url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if is_youtube {
            // Use yt-dlp defaults (android_vr etc.) so adaptive 720p–4K streams are available.
            // Forcing mweb/web without PO tokens often collapses to progressive ~360p only.
            cmd.arg("--remote-components")
                .arg("ejs:github");
        }

        if settings.download_mode == "video" {
            // Prefer mp4 for macOS playback when codecs allow (H.264/AAC).
            // mkv/webm remain as fallbacks for AV1/VP9 + opus merges.
            if is_youtube {
                cmd.arg("--merge-output-format").arg("mp4/mkv/webm");
            } else {
                cmd.arg("--merge-output-format").arg("mp4");
            }
        }

        if settings.download_mode == "audio" {
            cmd.arg("--extract-audio")
                .arg("--audio-format")
                .arg(&audio_format)
                .arg("--audio-quality")
                .arg("0");
        }

        if let Some(ffmpeg_path) = &ffmpeg_path {
            cmd.arg("--ffmpeg-location")
                .arg(ffmpeg_path);
        }

        if let Some(node_path) = &node_path {
            cmd.arg("--js-runtimes")
                .arg(format!("node:{}", node_path.to_string_lossy()));
        }

        if let Some(browser) = browser.as_deref() {
            cmd.arg("--cookies-from-browser").arg(browser);
        }

        if settings.proxy_enabled && !settings.proxy_url.trim().is_empty() {
            cmd.env("HTTP_PROXY", settings.proxy_url.trim());
            cmd.env("HTTPS_PROXY", settings.proxy_url.trim());
        }

        let mut child = cmd.spawn()
            .map_err(|e| format!("Failed to start local yt-dlp: {}", e))?;
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let progress_state = state.clone();
        let progress_app_handle = app_handle.clone();
        let progress_id = id.clone();
        // Aggregate multi-stage progress for any DASH merge (YouTube/Bilibili video).
        let aggregate_dash_progress = settings.download_mode == "video"
            && (is_bilibili || is_youtube);
        let progress_task = stdout_pipe.take().map(|stdout_reader| {
            tauri::async_runtime::spawn(async move {
                let reader = BufReader::new(stdout_reader);
                let mut lines = reader.lines();
                let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
                let mut download_stage = 0usize;
                let mut destination_count = 0usize;
                let mut last_mapped_progress = 0.0f64;

                while let Ok(Some(line)) = lines.next_line().await {
                    let trimmed = line.trim();
                    let mut next_update: Option<DownloadTask> = None;

                    if trimmed.contains("[download] Destination:") {
                        download_stage = destination_count;
                        destination_count += 1;
                    } else if let Some(progress) = parse_ytdlp_progress_line(trimmed) {
                        let now = std::time::Instant::now();
                        let mapped_progress = if aggregate_dash_progress {
                            match download_stage {
                                0 => progress.progress * 0.55,
                                1 => 0.55 + progress.progress * 0.35,
                                _ => 0.90 + progress.progress * 0.08,
                            }
                        } else {
                            progress.progress
                        }.max(last_mapped_progress).min(0.99);

                        if now.duration_since(last_emit).as_millis() < 120 && mapped_progress < 0.99 {
                            continue;
                        }

                        let mut state_lock = progress_state.lock().unwrap();
                        if let Some(task) = state_lock.tasks.get_mut(&progress_id) {
                            task.status = "downloading".to_string();
                            task.progress = mapped_progress;
                            if progress.total_bytes > 0 && !aggregate_dash_progress {
                                task.total_bytes = progress.total_bytes;
                                task.downloaded_bytes = progress.downloaded_bytes;
                            }
                            if let Some(speed) = progress.speed {
                                task.speed = speed;
                            }
                            if let Some(eta) = progress.eta {
                                task.eta = eta;
                            }
                            next_update = Some(task.clone());
                        }
                        last_mapped_progress = mapped_progress;
                        last_emit = now;
                    } else if trimmed.contains("[Merger]") || trimmed.contains("[ExtractAudio]") || trimmed.contains("Merging formats") {
                        let mut state_lock = progress_state.lock().unwrap();
                        if let Some(task) = state_lock.tasks.get_mut(&progress_id) {
                            task.status = "merging".to_string();
                            task.progress = task.progress.max(0.99);
                            task.speed = "FFmpeg".to_string();
                            task.eta = "--:--".to_string();
                            next_update = Some(task.clone());
                        }
                    }

                    if let Some(task) = next_update {
                        let _ = progress_app_handle.emit("task-updated", task);
                    }
                }
            })
        });
        let stderr_task = tauri::async_runtime::spawn(async move {
            let mut stderr = String::new();
            if let Some(stderr_reader) = stderr_pipe.as_mut() {
                let _ = stderr_reader.read_to_string(&mut stderr).await;
            }
            stderr
        });

        let wait_result = tokio::select! {
            result = child.wait() => result,
            _ = &mut abort_rx => {
                let _ = child.kill().await;
                if let Some(handle) = progress_task {
                    handle.abort();
                }
                remove_ytdlp_outputs(&save_path, &output_stem);
                return Ok(true);
            }
        };

        if let Some(handle) = progress_task {
            let _ = handle.await;
        }

        match wait_result {
            Ok(status) if status.success() => {
                if let Some(final_path) = resolve_ytdlp_output(&save_path, &output_stem) {
                    let downloaded_bytes = std::fs::metadata(&final_path)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    if downloaded_bytes > 0 {
                        let final_name = final_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&provisional_filename)
                            .to_string();
                        let mut state_lock = state.lock().unwrap();
                        state_lock.cancellations.remove(&id);
                        if let Some(task) = state_lock.tasks.get_mut(&id) {
                            task.title = final_name;
                            task.output_path = Some(final_path.to_string_lossy().into_owned());
                            task.status = "completed".to_string();
                            task.progress = 1.0;
                            task.downloaded_bytes = downloaded_bytes;
                            task.total_bytes = downloaded_bytes;
                            task.speed = "0 B/s".to_string();
                            task.eta = "Done".to_string();
                            let _ = app_handle.emit("task-updated", task.clone());
                            save_tasks(&state_lock.tasks, &state_lock.tasks_path);
                        }
                        return Ok(true);
                    }
                }
                last_error = "Local yt-dlp produced a 0-byte file".to_string();
            }
            Ok(_) => {
                let stderr = stderr_task.await.unwrap_or_default();
                last_error = ytdlp_error_text(&stderr);
            }
            Err(e) => {
                last_error = format!("Local yt-dlp failed: {}", e);
            }
        }
    }

    remove_ytdlp_outputs(&save_path, &output_stem);
    {
        let mut state_lock = state.lock().unwrap();
        state_lock.cancellations.remove(&id);
    }
    Err(last_error)
}

async fn request_media_service_with_fallbacks(
    url: &str,
    settings: &Settings,
    client: &reqwest::Client,
) -> Result<serde_json::Value, String> {
    if !is_youtube_url(url) {
        return request_media_service(
            build_media_service_payload(url, settings, false, None),
            settings,
            client,
        )
        .await
        .map_err(|e| format!("Media service error: {}", e));
    }

    // Cookie-backed servers are most reliable with web-like clients for videos
    // that trigger YouTube's login/bot checks. Keep Android/iOS as fallbacks for
    // public videos where those clients still expose better media URLs.
    let attempts = [
        ("MWEB", false, Some("MWEB")),
        ("WEB_CREATOR", false, Some("WEB_CREATOR")),
        ("YTMUSIC", false, Some("YTMUSIC")),
        ("ANDROID_VR", false, Some("ANDROID_VR")),
        ("IOS", false, Some("IOS")),
        ("ANDROID", false, Some("ANDROID")),
        ("HLS_IOS", true, Some("IOS")),
    ];

    let mut last_error = String::from("Unknown media service error");

    for (label, youtube_hls, innertube_client) in attempts {
        let payload = build_media_service_payload(url, settings, youtube_hls, innertube_client);
        match request_media_service(payload, settings, client).await {
            Ok(result) => {
                if result.get("status").and_then(|s| s.as_str()) != Some("error") {
                    println!("YouTube media service request succeeded with {}.", label);
                    return Ok(result);
                }

                last_error = result.get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| result.get("error").and_then(|e| e.get("code")).and_then(|c| c.as_str()))
                    .unwrap_or("Unknown media service error")
                    .to_string();
                println!("YouTube media service request with {} failed: {}", label, last_error);
            }
            Err(e) => {
                last_error = format!("Media service error with {}: {}", label, e);
                println!("{}", last_error);
            }
        }
    }

    Err(last_error)
}

// -----------------------------------------------------------
// Queue & Download Task Executer
// -----------------------------------------------------------
fn process_queue(state: Arc<Mutex<AppState>>, app_handle: tauri::AppHandle) {
    let state_lock = state.lock().unwrap();
    let limit = state_lock.settings.max_parallel_downloads as usize;
    
    let running_statuses = vec!["analyzing", "downloading", "merging"];
    let running_count = state_lock.tasks.values()
        .filter(|t| running_statuses.contains(&t.status.as_str()))
        .count();
        
    if running_count >= limit {
        return;
    }
    
    let next_task_id = state_lock.tasks.iter()
        .find(|(_, t)| t.status == "queued")
        .map(|(id, _)| id.clone());
        
    if let Some(id) = next_task_id {
        drop(state_lock);
        
        let mut state_mut = state.lock().unwrap();
        if let Some(task) = state_mut.tasks.get_mut(&id) {
            task.status = "analyzing".to_string();
            let updated_task = task.clone();
            let _ = app_handle.emit("task-updated", updated_task);
            save_tasks(&state_mut.tasks, &state_mut.tasks_path);
        }
        drop(state_mut);
        
        let state_clone = state.clone();
        let app_handle_clone = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            run_download_task(id, state_clone, app_handle_clone).await;
        });
        
        process_queue(state, app_handle);
    }
}

async fn run_download_task(id: String, state: Arc<Mutex<AppState>>, app_handle: tauri::AppHandle) {
    let (url_to_download, settings) = {
        let state_lock = state.lock().unwrap();
        let url = state_lock.tasks.get(&id).map(|task| task.url.clone()).unwrap_or_default();
        let settings = state_lock.settings.clone();
        (url, settings)
    };
    
    if url_to_download.is_empty() {
        return;
    }

    if is_local_ytdlp_url(&url_to_download) {
        let local_source_name = if is_youtube_url(&url_to_download) {
            "YouTube"
        } else if is_bilibili_url(&url_to_download) {
            "Bilibili"
        } else {
            "Dailymotion"
        };
        match try_local_ytdlp_download(
            id.clone(),
            &url_to_download,
            &settings,
            &state,
            &app_handle,
        ).await {
            Ok(true) => {
                process_queue(state, app_handle);
                return;
            }
            Ok(false) => {}
            Err(e) => {
                if local_source_name == "Dailymotion" {
                    println!("Local Dailymotion download failed, falling back to remote service: {}", e);
                } else {
                    update_task_failed(id, format!("Local {} download failed: {}", local_source_name, e), &state, &app_handle);
                    return;
                }
            }
        }
    }
    
    let mut client_builder = reqwest::Client::builder();
    if settings.proxy_enabled && !settings.proxy_url.trim().is_empty() {
        if let Ok(proxy) = reqwest::Proxy::all(settings.proxy_url.trim()) {
            let proxy = proxy.no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1"));
            client_builder = client_builder.proxy(proxy);
        }
    }
    let client = match client_builder.build() {
        Ok(c) => c,
        Err(e) => {
            update_task_failed(id, format!("Failed to build HTTP client: {}", e), &state, &app_handle);
            return;
        }
    };
    
    let result = match request_media_service_with_fallbacks(&url_to_download, &settings, &client).await {
        Ok(res) => res,
        Err(e) => {
            update_task_failed(id, e, &state, &app_handle);
            return;
        }
    };
    
    let status = result.get("status").and_then(|s| s.as_str()).unwrap_or("");
    if status == "error" {
        let err_msg = result.get("text")
            .and_then(|t| t.as_str())
            .or_else(|| result.get("error").and_then(|e| e.get("code")).and_then(|c| c.as_str()))
            .unwrap_or("Unknown media service error");
        update_task_failed(id, err_msg.to_string(), &state, &app_handle);
        return;
    }
    
    let download_url;
    let mut filename = result.get("filename")
        .and_then(|f| f.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("cobalt_{}", id));

    if status == "redirect" || status == "tunnel" {
        let url_str = result.get("url").and_then(|u| u.as_str()).unwrap_or("");
        if url_str.starts_with("http") {
            download_url = url_str.to_string();
        } else {
            download_url = absolute_api_url(&settings, url_str);
        }
    } else if status == "picker" {
        if let Some(picker) = result.get("picker").and_then(|p| p.as_array()) {
            if picker.is_empty() {
                update_task_failed(id, "Picker response returned no items".to_string(), &state, &app_handle);
                return;
            }
            let chosen = picker.iter()
                .find(|item| item.get("type").and_then(|t| t.as_str()) == Some("video"))
                .unwrap_or(&picker[0]);
            download_url = absolute_api_url(
                &settings,
                chosen.get("url").and_then(|u| u.as_str()).unwrap_or("")
            );
            if result.get("filename").is_none() {
                let ext = if chosen.get("type").and_then(|t| t.as_str()) == Some("photo") { "jpg" } else { "mp4" };
                filename = format!("cobalt_{}.{}", id, ext);
            }
        } else {
            update_task_failed(id, "Invalid picker response format".to_string(), &state, &app_handle);
            return;
        }
    } else {
        update_task_failed(id, format!("Unsupported response status: {}", status), &state, &app_handle);
        return;
    }
    
    let save_path = std::path::PathBuf::from(&settings.save_path);
    if !save_path.exists() {
        std::fs::create_dir_all(&save_path).ok();
    }
    let output_path = unique_output_path(&save_path, &filename);
    
    let mut abort_rx = {
        let mut state_lock = state.lock().unwrap();
        if let Some(task) = state_lock.tasks.get_mut(&id) {
            task.title = filename.clone();
            task.status = "downloading".to_string();
            task.output_path = Some(output_path.to_string_lossy().into_owned());
            let updated_task = task.clone();
            let _ = app_handle.emit("task-updated", updated_task);
            save_tasks(&state_lock.tasks, &state_lock.tasks_path);
        }
        
        let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
        state_lock.cancellations.insert(id.clone(), abort_tx);
        abort_rx
    };
    
    let media_res = match client.get(&download_url).send().await {
        Ok(res) => res,
        Err(e) => {
            update_task_failed(id, format!("Media server error: {}", e), &state, &app_handle);
            return;
        }
    };
    
    let media_status = media_res.status();
    if !media_status.is_success() {
        let error_text = media_res.text().await.unwrap_or_default();
        let detail = if error_text.trim().is_empty() {
            media_status.to_string()
        } else {
            format!("{}: {}", media_status, error_text.trim())
        };
        update_task_failed(id, format!("Media server error: {}", detail), &state, &app_handle);
        return;
    }
    
    let total_bytes = media_res.content_length().unwrap_or(0);
    {
        let mut state_lock = state.lock().unwrap();
        if let Some(task) = state_lock.tasks.get_mut(&id) {
            task.total_bytes = total_bytes;
            let _ = app_handle.emit("task-updated", task.clone());
        }
    }
    
    let mut file = match tokio::fs::File::create(&output_path).await {
        Ok(f) => f,
        Err(e) => {
            update_task_failed(id, format!("Disk write error: {}", e), &state, &app_handle);
            return;
        }
    };
    
    let mut stream = media_res.bytes_stream();
    let mut downloaded_bytes = 0u64;
    let mut last_speed_time = std::time::Instant::now();
    let mut last_speed_bytes = 0u64;
    let mut last_ui_update = std::time::Instant::now();
    
    loop {
        tokio::select! {
            _ = &mut abort_rx => {
                drop(file);
                let _ = std::fs::remove_file(&output_path);
                return;
            }
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        if let Err(e) = file.write_all(&bytes).await {
                            update_task_failed(id, format!("Disk write error: {}", e), &state, &app_handle);
                            return;
                        }
                        downloaded_bytes += bytes.len() as u64;
                        
                        let now = std::time::Instant::now();
                        let ui_elapsed = now.duration_since(last_ui_update).as_millis();
                        
                        if ui_elapsed >= 120 {
                            let speed_elapsed = now.duration_since(last_speed_time).as_millis();
                            let mut speed_str = "0 B/s".to_string();
                            let mut eta_str = "--:--".to_string();
                            
                            if speed_elapsed >= 1000 {
                                let speed_bps = ((downloaded_bytes - last_speed_bytes) as f64 / (speed_elapsed as f64)) * 1000.0;
                                speed_str = format!("{}/s", format_bytes(speed_bps));
                                
                                if total_bytes > 0 && speed_bps > 0.0 {
                                    let eta_secs = ((total_bytes - downloaded_bytes) as f64 / speed_bps) as u32;
                                    eta_str = format!("{}:{:02}", eta_secs / 60, eta_secs % 60);
                                }
                                
                                last_speed_time = now;
                                last_speed_bytes = downloaded_bytes;
                            }
                            
                            let mut state_lock = state.lock().unwrap();
                            if let Some(task) = state_lock.tasks.get_mut(&id) {
                                task.downloaded_bytes = downloaded_bytes;
                                if total_bytes > 0 {
                                    task.progress = (downloaded_bytes as f64 / total_bytes as f64).min(0.99);
                                }
                                if speed_elapsed >= 1000 {
                                    task.speed = speed_str;
                                    task.eta = eta_str;
                                }
                                let updated_task = task.clone();
                                let _ = app_handle.emit("task-updated", updated_task);
                            }
                            last_ui_update = now;
                        }
                    }
                    Some(Err(e)) => {
                        update_task_failed(id, format!("Download error: {}", e), &state, &app_handle);
                        return;
                    }
                    None => break,
                }
            }
        }
    }
    
    if let Err(e) = file.flush().await {
        update_task_failed(id, format!("Disk flush error: {}", e), &state, &app_handle);
        return;
    }
    drop(file);
    
    if downloaded_bytes == 0 {
        update_task_failed(id, "No data received from media server (0-byte file)".to_string(), &state, &app_handle);
        return;
    }
    
    {
        let mut state_lock = state.lock().unwrap();
        state_lock.cancellations.remove(&id);
        if let Some(task) = state_lock.tasks.get_mut(&id) {
            task.status = "completed".to_string();
            task.progress = 1.0;
            task.downloaded_bytes = downloaded_bytes;
            task.speed = "0 B/s".to_string();
            task.eta = "Done".to_string();
            let updated_task = task.clone();
            let _ = app_handle.emit("task-updated", updated_task);
            save_tasks(&state_lock.tasks, &state_lock.tasks_path);
        }
    }
    
    process_queue(state, app_handle);
}

fn update_task_failed(id: String, error_msg: String, state: &Arc<Mutex<AppState>>, app_handle: &tauri::AppHandle) {
    let mut state_lock = state.lock().unwrap();
    state_lock.cancellations.remove(&id);
    if let Some(task) = state_lock.tasks.get_mut(&id) {
        task.status = "failed".to_string();
        task.error = Some(error_msg);
        let updated_task = task.clone();
        let _ = app_handle.emit("task-updated", updated_task);
        save_tasks(&state_lock.tasks, &state_lock.tasks_path);
    }
    drop(state_lock);
    
    process_queue(state.clone(), app_handle.clone());
}

// -----------------------------------------------------------
// Commands implementation
// -----------------------------------------------------------
#[tauri::command]
fn get_settings(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Settings {
    let state = state.lock().unwrap();
    state.settings.clone()
}

#[tauri::command]
fn save_settings(new_settings: Settings, state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<Settings, String> {
    let mut state = state.lock().unwrap();
    state.settings = new_settings.clone();
    
    if let Err(e) = std::fs::write(&state.settings_path, serde_json::to_string_pretty(&state.settings).unwrap()) {
        return Err(format!("Failed to save settings: {}", e));
    }
    Ok(new_settings)
}

#[tauri::command]
fn select_directory() -> Option<String> {
    let folder = rfd::FileDialog::new()
        .pick_folder();
    folder.map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn reveal_in_finder(path: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open")
            .args(&["-R", &path])
            .spawn();
        Ok(true)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("explorer")
            .args(&[format!("/select,\"{}\"", path)])
            .spawn();
        Ok(true)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = Command::new("xdg-open")
                .arg(parent)
                .spawn();
        }
        Ok(true)
    }
}

#[tauri::command]
fn open_file(path: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(&path).spawn();
        Ok(true)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd").args(&["/c", "start", "", &path]).spawn();
        Ok(true)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = Command::new("xdg-open").arg(&path).spawn();
        Ok(true)
    }
}

#[tauri::command]
fn get_tasks(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Vec<DownloadTask> {
    let state = state.lock().unwrap();
    state.tasks.values().cloned().collect()
}

#[tauri::command]
fn cancel_task(id: String, state: tauri::State<'_, Arc<Mutex<AppState>>>, app_handle: tauri::AppHandle) -> bool {
    let mut state_lock = state.lock().unwrap();
    if let Some(tx) = state_lock.cancellations.remove(&id) {
        let _ = tx.send(());
    }
    
    if let Some(task) = state_lock.tasks.get_mut(&id) {
        if vec!["downloading", "analyzing", "queued"].contains(&task.status.as_str()) {
            task.status = "cancelled".to_string();
            task.speed = "0 B/s".to_string();
            task.progress = 0.0;
            task.eta = "--:--".to_string();
            
            if let Some(ref path) = task.output_path {
                let _ = std::fs::remove_file(path);
            }
            
            let _ = app_handle.emit("task-updated", task.clone());
            save_tasks(&state_lock.tasks, &state_lock.tasks_path);
            drop(state_lock);
            
            process_queue(state.inner().clone(), app_handle);
            return true;
        }
    }
    false
}

#[tauri::command]
fn delete_task(id: String, state: tauri::State<'_, Arc<Mutex<AppState>>>, app_handle: tauri::AppHandle) -> bool {
    let mut state_lock = state.lock().unwrap();
    if let Some(tx) = state_lock.cancellations.remove(&id) {
        let _ = tx.send(());
    }
    
    let task = state_lock.tasks.remove(&id);
    if let Some(t) = task {
        if vec!["downloading", "analyzing", "queued"].contains(&t.status.as_str()) {
            if let Some(ref path) = t.output_path {
                let _ = std::fs::remove_file(path);
            }
        }
        save_tasks(&state_lock.tasks, &state_lock.tasks_path);
        drop(state_lock);
        process_queue(state.inner().clone(), app_handle);
        return true;
    }
    false
}

#[tauri::command]
fn clear_completed(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Vec<DownloadTask> {
    let mut state = state.lock().unwrap();
    state.tasks.retain(|_, task| !vec!["completed", "cancelled", "failed"].contains(&task.status.as_str()));
    save_tasks(&state.tasks, &state.tasks_path);
    state.tasks.values().cloned().collect()
}

#[tauri::command]
fn download_url(url: String, state: tauri::State<'_, Arc<Mutex<AppState>>>, app_handle: tauri::AppHandle) -> DownloadTask {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();
        
    let task = DownloadTask {
        id: id.clone(),
        url,
        title: "Analyzing URL...".to_string(),
        status: "queued".to_string(),
        progress: 0.0,
        speed: "0 B/s".to_string(),
        downloaded_bytes: 0,
        total_bytes: 0,
        eta: "--:--".to_string(),
        error: None,
        output_path: None,
    };
    
    let mut state_lock = state.lock().unwrap();
    state_lock.tasks.insert(id, task.clone());
    save_tasks(&state_lock.tasks, &state_lock.tasks_path);
    drop(state_lock);
    
    let _ = app_handle.emit("task-updated", task.clone());
    
    process_queue(state.inner().clone(), app_handle);
    
    task
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::current_dir().unwrap());
            std::fs::create_dir_all(&app_data_dir).ok();
            let settings_path = app_data_dir.join("settings.json");
            let tasks_path = app_data_dir.join("tasks.json");
            
            let settings = if settings_path.exists() {
                let content = std::fs::read_to_string(&settings_path).unwrap_or_default();
                serde_json::from_str::<Settings>(&content).unwrap_or_else(|_| {
                    let download_dir = app.path().download_dir().unwrap_or_else(|_| std::env::current_dir().unwrap());
                    Settings::default_with_download_dir(download_dir)
                })
            } else {
                let download_dir = app.path().download_dir().unwrap_or_else(|_| std::env::current_dir().unwrap());
                Settings::default_with_download_dir(download_dir)
            };
            
            // Save initial defaults if missing
            if !settings_path.exists() {
                let _ = std::fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap());
            }

            // Load persisted tasks
            let mut tasks = if tasks_path.exists() {
                let content = std::fs::read_to_string(&tasks_path).unwrap_or_default();
                serde_json::from_str::<HashMap<String, DownloadTask>>(&content).unwrap_or_default()
            } else {
                HashMap::new()
            };

            // Reset stuck tasks to failed status on startup since the app restarted
            for task in tasks.values_mut() {
                if vec!["downloading", "analyzing", "queued", "merging"].contains(&task.status.as_str()) {
                    task.status = "failed".to_string();
                    task.error = Some("App restarted during download".to_string());
                    task.speed = "0 B/s".to_string();
                    task.eta = "--:--".to_string();
                }
            }

            let app_state = Arc::new(Mutex::new(AppState {
                settings,
                tasks,
                cancellations: HashMap::new(),
                settings_path,
                tasks_path,
            }));
            
            app.manage(app_state.clone());
            
            // Spawn background clipboard monitor
            let handle = app.handle().clone();
            let state_clone = app_state.clone();
            tauri::async_runtime::spawn(async move {
                let mut last_clipboard = String::new();
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;
                    
                    let monitoring = {
                        let state = state_clone.lock().unwrap();
                        state.settings.clipboard_monitoring
                    };
                    
                    if !monitoring {
                        continue;
                    }
                    
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            let trimmed = text.trim().to_string();
                            if !trimmed.is_empty() && trimmed != last_clipboard && trimmed.starts_with("http") {
                                last_clipboard = trimmed.clone();
                                let _ = handle.emit("clipboard-detected", trimmed);
                            }
                        }
                    }
                }
            });
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            select_directory,
            reveal_in_finder,
            open_file,
            get_tasks,
            cancel_task,
            delete_task,
            clear_completed,
            download_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
