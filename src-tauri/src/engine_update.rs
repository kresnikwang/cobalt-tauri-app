// Runtime self-update for the bundled yt-dlp engine.
//
// yt-dlp is versioned with the app bundle, but it fights site changes with
// frequent releases. A pinned sidecar therefore goes stale between app
// releases, which shows up as "every site suddenly stopped working". This
// module downloads the official standalone binary into the writable app data
// directory, verifies it against the release's SHA2-256SUMS, and lets
// `resolve_ytdlp_path()` prefer it over the bundled copy (which stays as the
// offline fallback).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const RELEASE_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const RELEASE_DOWNLOAD: &str = "https://github.com/yt-dlp/yt-dlp/releases/download";
const ENGINE_DIR: &str = "engines";
const ENGINE_FILE: &str = "yt-dlp";

/// Release asset for the running platform. macOS ships a universal binary that
/// covers both arm64 and x86_64.
fn asset_name() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) => "yt-dlp_macos",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("linux", _) => "yt-dlp_linux",
        ("windows", _) => "yt-dlp.exe",
        _ => "yt-dlp_linux",
    }
}

pub fn updated_binary_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(ENGINE_DIR)
}

/// Path of the runtime-updated engine, if one has been installed.
pub fn updated_binary_path(app_data_dir: &Path) -> PathBuf {
    updated_binary_dir(app_data_dir).join(ENGINE_FILE)
}

/// Run `<binary> --version`. Returns the trimmed version string.
pub async fn installed_version(path: &Path) -> Option<String> {
    installed_version_with_timeout(path, Duration::from_secs(5)).await
}

async fn installed_version_with_timeout(path: &Path, timeout: Duration) -> Option<String> {
    if !path.exists() {
        return None;
    }
    // A cold standalone engine can take seconds to unpack. Never wait on the
    // UI thread, and kill a hung probe when the timeout drops its future.
    let child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| eprintln!("Could not inspect engine {}: {error}", path.display()))
        .ok()?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

fn request_client(proxy_url: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120));
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|error| format!("Invalid proxy URL '{proxy_url}': {error}"))?;
        builder =
            builder.proxy(proxy.no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1")));
    }
    builder
        .build()
        .map_err(|error| format!("Failed to build HTTP client: {error}"))
}

/// Latest published yt-dlp release tag, e.g. `2026.08.19`.
pub async fn latest_version(proxy_url: Option<&str>) -> Result<String, String> {
    let client = request_client(proxy_url)?;
    let response = tokio::time::timeout(Duration::from_secs(30), client.get(RELEASE_API).send())
        .await
        .map_err(|_| "yt-dlp release check timed out".to_string())?
        .map_err(|error| format!("yt-dlp release check failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "yt-dlp release check returned {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("error")
        ));
    }
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("Could not read the yt-dlp release payload: {error}"))?;
    payload
        .get("tag_name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "yt-dlp release payload had no tag_name".to_string())
}

fn hex_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// Download the binary for `version` into the app data directory.
///
/// The archive checksum is verified against the release's own SHA2-256SUMS
/// before anything is installed, and the downloaded binary must answer
/// `--version` with the expected release tag before it replaces the current
/// engine. On any failure the previously installed engine keeps working.
pub async fn download_version(
    app_data_dir: &Path,
    version: &str,
    proxy_url: Option<&str>,
) -> Result<String, String> {
    let asset = asset_name();
    let client = request_client(proxy_url)?;
    let engine_dir = updated_binary_dir(app_data_dir);
    std::fs::create_dir_all(&engine_dir)
        .map_err(|error| format!("Could not create {}: {error}", engine_dir.display()))?;

    let binary_url = format!("{RELEASE_DOWNLOAD}/{version}/{asset}");
    let response = tokio::time::timeout(Duration::from_secs(300), client.get(&binary_url).send())
        .await
        .map_err(|_| "Downloading the yt-dlp engine timed out".to_string())?
        .map_err(|error| format!("Downloading the yt-dlp engine failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "yt-dlp engine download returned {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("error")
        ));
    }
    let binary = response
        .bytes()
        .await
        .map_err(|error| format!("Reading the yt-dlp engine download failed: {error}"))?;
    if binary.len() < 1024 * 512 {
        return Err("Downloaded yt-dlp engine looks truncated".to_string());
    }

    // Verify against the release's own checksums when they are reachable; a
    // fetch failure here must not block an otherwise sound update.
    match tokio::time::timeout(
        Duration::from_secs(60),
        client
            .get(format!("{RELEASE_DOWNLOAD}/{version}/SHA2-256SUMS"))
            .send(),
    )
    .await
    {
        Ok(Ok(response)) if response.status().is_success() => {
            if let Ok(sums) = response.text().await {
                let expected = sums.lines().find_map(|line| {
                    let mut parts = line.split_whitespace();
                    let digest = parts.next()?;
                    let name = parts.next()?;
                    (name == asset || name == format!("*{asset}"))
                        .then(|| digest.trim().to_ascii_lowercase())
                });
                if let Some(expected) = expected {
                    let actual = hex_digest(&binary);
                    if actual != expected {
                        return Err(format!(
                            "yt-dlp engine checksum mismatch (expected {expected}, got {actual})"
                        ));
                    }
                }
            }
        }
        _ => {
            eprintln!("yt-dlp SHA2-256SUMS unavailable; installing the downloaded binary unchecked")
        }
    }

    // Stage, then promote: a mid-write crash must not leave a half-installed
    // engine that the app treats as the update.
    let staged = engine_dir.join(format!(".{ENGINE_FILE}.staged"));
    std::fs::write(&staged, &binary)
        .map_err(|error| format!("Could not stage the yt-dlp engine: {error}"))?;
    set_executable(&staged)?;

    let staged_version = installed_version(&staged)
        .await
        .ok_or_else(|| "Downloaded yt-dlp engine failed its smoke test".to_string())?;
    if !staged_version.trim().eq_ignore_ascii_case(version.trim()) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!(
            "Downloaded yt-dlp engine reported version {staged_version}, expected {version}"
        ));
    }

    let final_path = updated_binary_path(app_data_dir);
    let previous = engine_dir.join(format!(".{ENGINE_FILE}.previous"));
    if final_path.exists() {
        let _ = std::fs::remove_file(&previous);
        std::fs::rename(&final_path, &previous)
            .map_err(|error| format!("Could not replace the yt-dlp engine: {error}"))?;
    }
    std::fs::rename(&staged, &final_path)
        .map_err(|error| format!("Could not install the yt-dlp engine: {error}"))?;
    let _ = std::fs::remove_file(&previous);

    Ok(staged_version)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("Could not mark {} executable: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct Probe(PathBuf);
    static PROBE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    impl Probe {
        fn new(script: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let sequence = PROBE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "cobalt-engine-probe-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("probe");
            std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            set_executable(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    #[tokio::test]
    async fn version_probe_reads_success_and_rejects_invalid_output() {
        let valid = Probe::new("printf ' 2026.10.10\\n'");
        assert_eq!(
            installed_version(&valid.0).await.as_deref(),
            Some("2026.10.10")
        );
        let failed = Probe::new("printf '2026.10.10\\n'; exit 1");
        assert!(installed_version(&failed.0).await.is_none());
        let empty = Probe::new("printf '  \\n'");
        assert!(installed_version(&empty.0).await.is_none());
        assert!(installed_version(&empty.0.with_file_name("missing"))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn hung_version_probe_times_out_without_blocking_or_leaking_child() {
        let hung = Probe::new("echo $$ > \"$0.pid\"\nexec /bin/sleep 30");
        let started = std::time::Instant::now();
        let probe = installed_version_with_timeout(&hung.0, Duration::from_secs(3));
        let heartbeat = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "probe blocked the async executor"
            );
        };
        let (version, ()) = tokio::join!(probe, heartbeat);
        assert!(version.is_none());
        assert!(started.elapsed() < Duration::from_secs(5));

        let pid = std::fs::read_to_string(hung.0.with_extension("pid")).unwrap();
        // kill_on_drop sends the signal immediately; allow the OS to reap it.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let alive = Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(!alive.success(), "timed-out probe was left running");
    }
}
