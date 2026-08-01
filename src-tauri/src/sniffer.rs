use std::{collections::HashMap, io::{BufRead, BufReader, Write}, path::PathBuf, process::{Child, ChildStdin, Command, Stdio}, sync::{Arc, Mutex}, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedResource {
    pub id: String,
    pub title: String,
    pub source: String,
    pub kind: String,
    pub mime_type: String,
    pub size: i64,
    pub extension: String,
    pub captured_at: String,
}

#[derive(Debug, Clone)]
pub struct DownloadDescriptor {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub filename: String,
    pub kind: String,
    pub decode_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnifferViewState {
    pub status: String,
    pub port: u16,
    pub message: Option<String>,
    pub captures: Vec<CapturedResource>,
    pub supported_sources: Vec<&'static str>,
    pub certificate_path: Option<String>,
    pub certificate_fingerprint: Option<String>,
    pub certificate_installed: bool,
    pub proxy_active: bool,
    pub wechat_hooks: u32,
    pub wechat_callbacks: u32,
}

pub struct SnifferState {
    pub status: String,
    pub port: u16,
    pub message: Option<String>,
    pub captures: HashMap<String, CapturedResource>,
    pub descriptors: HashMap<String, DownloadDescriptor>,
    stdin: Option<ChildStdin>,
    child: Option<Child>,
    pub data_dir: PathBuf,
    pub certificate_path: Option<PathBuf>,
    pub certificate_fingerprint: Option<String>,
    pub certificate_installed: bool,
    pub proxy_active: bool,
    pub wechat_hooks: u32,
    pub wechat_callbacks: u32,
}

impl SnifferState {
    pub fn new(data_dir: PathBuf) -> Self {
        let cert_path = data_dir.join("cobalt-capture-ca.pem");
        Self { status: "stopped".into(), port: 8899, message: None, captures: HashMap::new(), descriptors: HashMap::new(), stdin: None, child: None, data_dir, certificate_path: Some(cert_path), certificate_fingerprint: None, certificate_installed: false, proxy_active: false, wechat_hooks: 0, wechat_callbacks: 0 }
    }
    pub fn view(&self) -> SnifferViewState {
        let mut captures: Vec<_> = self.captures.values().cloned().collect();
        captures.sort_by(|a, b| b.captured_at.cmp(&a.captured_at));
        SnifferViewState { status:self.status.clone(), port:self.port, message:self.message.clone(), captures, supported_sources: vec!["WeChat Channels", "Douyin", "Bilibili", "Xiaohongshu", "Kuaishou", "QQ Music / KuGou", "Safari / Chrome Web Browsers"], certificate_path:self.certificate_path.as_ref().map(|path| path.to_string_lossy().into_owned()), certificate_fingerprint:self.certificate_fingerprint.clone(), certificate_installed:self.certificate_installed, proxy_active:self.proxy_active, wechat_hooks:self.wechat_hooks, wechat_callbacks:self.wechat_callbacks }
    }
}

fn emit_state(app: &AppHandle, state: &SnifferState) { let _ = app.emit("sniffer-updated", state.view()); }

fn send(state: &mut SnifferState, value: Value) -> Result<(), String> {
    let stdin = state.stdin.as_mut().ok_or("Resource sniffer is not running")?;
    serde_json::to_writer(&mut *stdin, &value).map_err(|e| e.to_string())?;
    stdin.write_all(b"\n").map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())
}

fn resource_binary(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().resolve("binaries/res-sniffer", tauri::path::BaseDirectory::Resource)
        .map_err(|e| e.to_string())
}

pub fn start(app: AppHandle, sniffer: Arc<Mutex<SnifferState>>, upstream_proxy: Option<String>) -> Result<SnifferViewState, String> {
    let binary = resource_binary(&app)?;
    if !binary.exists() { return Err("Resource sniffer is not bundled in this build. Rebuild with Go 1.22+ installed.".into()); }
    let mut guard = sniffer.lock().map_err(|_| "Sniffer state is unavailable")?;
    if guard.status == "running" { return Ok(guard.view()); }
    std::fs::create_dir_all(&guard.data_dir).map_err(|e| e.to_string())?;
    let mut child = Command::new(binary).arg("--data-dir").arg(&guard.data_dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("Unable to start resource sniffer: {e}"))?;
    let stdout = child.stdout.take().ok_or("Resource sniffer did not expose stdout")?;
    guard.stdin = child.stdin.take(); guard.child = Some(child); guard.status = "starting".into(); guard.message = Some("Configure the system proxy, then play media in the target app.".into());
    let port = guard.port; let upstream = upstream_proxy.unwrap_or_default();
    send(&mut guard, json!({"id":"start", "command":"start", "port":port, "upstreamProxy":upstream}))?;
    emit_state(&app, &guard); drop(guard);

    let reader_state = sniffer.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
            let mut state = match reader_state.lock() { Ok(state) => state, Err(_) => return };
            match value.get("type").and_then(Value::as_str) {
                Some("ready") | Some("status") => { if let Some(data) = value.get("data") { if data.get("running").and_then(Value::as_bool) == Some(true) { state.status = "running".into(); } state.certificate_path = data.get("certificatePath").and_then(Value::as_str).map(PathBuf::from).or(state.certificate_path.clone()); state.certificate_fingerprint = data.get("certificateFingerprint").and_then(Value::as_str).map(str::to_string).or(state.certificate_fingerprint.clone()); state.certificate_installed = state.certificate_fingerprint.as_deref().map(certificate_is_installed).unwrap_or(false); state.wechat_hooks = data.get("wechatHooks").and_then(Value::as_u64).map(|count| count as u32).unwrap_or(state.wechat_hooks); state.wechat_callbacks = data.get("wechatCallbacks").and_then(Value::as_u64).map(|count| count as u32).unwrap_or(state.wechat_callbacks); if let Some(message) = data.get("message").and_then(Value::as_str) { state.message = Some(message.to_string()); } } }
                Some("resource") => if let Some(data) = value.get("data") {
                    if let Ok(resource) = serde_json::from_value::<CapturedResource>(data.clone()) { state.captures.insert(resource.id.clone(), resource); state.wechat_callbacks += 1; state.message = Some("WeChat Channels media metadata captured.".into()); }
                },
                Some("response") => if let Some(id) = value.get("id").and_then(Value::as_str) {
                    if let Some(resource_id) = id.strip_prefix("resolve:") {
                        if value.get("ok").and_then(Value::as_bool) == Some(true) {
                            if let Some(data) = value.get("data") {
                                let headers = data.get("headers").and_then(Value::as_object).map(|items| items.iter().filter_map(|(key, value)| value.as_array().and_then(|v| v.first()).and_then(Value::as_str).map(|v| (key.clone(), v.to_string()))).collect()).unwrap_or_default();
                                if let Some(url) = data.get("url").and_then(Value::as_str) { state.descriptors.insert(resource_id.to_string(), DownloadDescriptor { url:url.to_string(), headers, filename:data.get("filename").and_then(Value::as_str).unwrap_or("captured-media.mp4").to_string(), kind:data.get("kind").and_then(Value::as_str).unwrap_or("video").to_string(), decode_key:data.get("decodeKey").and_then(Value::as_str).filter(|key| !key.is_empty()).map(str::to_string) }); }
                            }
                        }
                    }
                },
                Some("error") => { state.status = "error".into(); state.message = value.get("error").and_then(Value::as_str).map(str::to_string); }
                _ => {}
            }
            emit_state(&app, &state);
        }
    });
    Ok(sniffer.lock().map_err(|_| "Sniffer state is unavailable")?.view())
}

pub fn stop(app: &AppHandle, sniffer: &Arc<Mutex<SnifferState>>) -> Result<SnifferViewState, String> {
    let mut state = sniffer.lock().map_err(|_| "Sniffer state is unavailable")?;
    let _ = restore_system_proxy(&mut state);
    let _ = send(&mut state, json!({"id":"stop", "command":"stop"}));
    if let Some(mut child) = state.child.take() { let _ = child.kill(); let _ = child.wait(); }
    state.stdin = None; state.status = "stopped".into(); state.message = None; state.descriptors.clear(); emit_state(app, &state); Ok(state.view())
}

#[derive(Debug, Serialize, Deserialize)]
struct ProxyState { enabled: bool, server: String, port: u16, authenticated: bool }
#[derive(Debug, Serialize, Deserialize)]
struct ProxyServiceSnapshot { service: String, web: ProxyState, secure: ProxyState }

fn proxy_snapshot_path(state: &SnifferState) -> PathBuf { state.data_dir.join("sniffer-proxy-session.json") }
fn command_output(args: &[&str]) -> Result<String, String> { let output = Command::new("networksetup").args(args).output().map_err(|error| error.to_string())?; if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).trim().to_string()); } Ok(String::from_utf8_lossy(&output.stdout).into_owned()) }
fn parse_proxy_state(output: &str) -> Result<ProxyState, String> { let value = |key: &str| output.lines().find_map(|line| line.strip_prefix(key).map(str::trim)).ok_or_else(|| format!("Missing {key} in networksetup output")); let authenticated = value("Authenticated Proxy Enabled:")?.eq_ignore_ascii_case("yes"); if authenticated { return Err("Cobalt will not overwrite an authenticated system proxy. Disable it manually or use a separate network service.".into()); } Ok(ProxyState { enabled:value("Enabled:")?.eq_ignore_ascii_case("yes"), server:value("Server:")?.to_string(), port:value("Port:")?.parse().map_err(|_| "Invalid existing proxy port")?, authenticated }) }
fn active_services() -> Result<Vec<String>, String> {
    let mut active = Vec::new();
    for name in command_output(&["-listallnetworkservices"] )?.lines().map(str::trim).filter(|name| !name.is_empty() && !name.starts_with('*') && !name.contains("Serial Port")) {
        if command_output(&["-getinfo", name]).map(|info| info.contains("IP address:")).unwrap_or(false) { active.push(name.to_string()); }
    }
    if active.is_empty() { return Err("No active macOS network services were found.".into()); }
    Ok(active)
}

pub fn enable_system_proxy(app: &AppHandle, sniffer: &Arc<Mutex<SnifferState>>) -> Result<SnifferViewState, String> {
    let mut state = sniffer.lock().map_err(|_| "Sniffer state is unavailable")?;
    if state.status != "running" { return Err("Start the resource sniffer before enabling its system proxy.".into()); }
    if !state.certificate_installed {
		return Err("Install and verify Cobalt's local capture certificate before enabling the system proxy.".into());
    }
    let mut snapshot = Vec::new();
    for service in active_services()? { let web = parse_proxy_state(&command_output(&["-getwebproxy", &service])?)?; let secure = parse_proxy_state(&command_output(&["-getsecurewebproxy", &service])?)?; snapshot.push(ProxyServiceSnapshot { service, web, secure }); }
    let content = serde_json::to_vec_pretty(&snapshot).map_err(|error| error.to_string())?; std::fs::write(proxy_snapshot_path(&state), content).map_err(|error| error.to_string())?;
    for item in &snapshot { command_output(&["-setwebproxy", &item.service, "127.0.0.1", &state.port.to_string()])?; command_output(&["-setsecurewebproxy", &item.service, "127.0.0.1", &state.port.to_string()])?; command_output(&["-setwebproxystate", &item.service, "on"])?; command_output(&["-setsecurewebproxystate", &item.service, "on"])?; }
    state.proxy_active = true; state.message = Some("System proxy is active. Play a video in WeChat Channels now.".into()); emit_state(app, &state); Ok(state.view())
}

pub fn restore_system_proxy(state: &mut SnifferState) -> Result<(), String> {
    let path = proxy_snapshot_path(state); if !path.exists() { state.proxy_active = false; return Ok(()); }
    let snapshots: Vec<ProxyServiceSnapshot> = serde_json::from_slice(&std::fs::read(&path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    for item in snapshots { for (kind, saved) in [("web", item.web), ("secure", item.secure)] { let set = if kind == "web" { "-setwebproxy" } else { "-setsecurewebproxy" }; let toggle = if kind == "web" { "-setwebproxystate" } else { "-setsecurewebproxystate" }; if saved.enabled { command_output(&[set, &item.service, &saved.server, &saved.port.to_string()])?; command_output(&[toggle, &item.service, "on"])?; } else { command_output(&[toggle, &item.service, "off"])?; } } }
    let _ = std::fs::remove_file(path); state.proxy_active = false; Ok(())
}

pub fn install_certificate(app: &AppHandle, sniffer: &Arc<Mutex<SnifferState>>) -> Result<SnifferViewState, String> {
    let mut state = sniffer.lock().map_err(|_| "Sniffer state is unavailable")?;
    let certificate = state.certificate_path.clone().unwrap_or_else(|| state.data_dir.join("cobalt-capture-ca.pem"));
    let keychain = dirs_home_keychain();
    let output = Command::new("security").args(["add-trusted-cert", "-r", "trustRoot", "-p", "ssl", "-k", &keychain, &certificate.to_string_lossy()]).output().map_err(|error| error.to_string())?;
    if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).trim().to_string()); }
	let fingerprint = state.certificate_fingerprint.as_deref().ok_or("The sniffer has not reported its certificate fingerprint yet. Stop and start capture, then retry.")?;
	if !certificate_is_installed(fingerprint) {
		return Err("macOS did not report the current Cobalt certificate as trusted. Open Keychain Access and verify its SSL trust setting.".into());
	}
    state.certificate_path = Some(certificate);
    state.certificate_installed = true;
    state.message = Some("Local capture certificate installed and verified in the login keychain.".into());
    emit_state(app, &state);
    Ok(state.view())
}
fn dirs_home_keychain() -> String { std::env::var("HOME").map(|home| format!("{home}/Library/Keychains/login.keychain-db")).unwrap_or_else(|_| "/Library/Keychains/login.keychain".into()) }
fn certificate_is_installed(fingerprint: &str) -> bool { let output = Command::new("security").args(["find-certificate", "-a", "-Z", "-c", "Cobalt Local Resource Capture CA"]).output(); match output { Ok(output) if output.status.success() => keychain_contains_fingerprint(&String::from_utf8_lossy(&output.stdout), fingerprint), _ => false } }
fn keychain_contains_fingerprint(output: &str, fingerprint: &str) -> bool { output.lines().filter_map(|line| line.strip_prefix("SHA-256 hash:")).any(|value| value.trim().eq_ignore_ascii_case(fingerprint)) }

pub fn request_descriptor(sniffer: &Arc<Mutex<SnifferState>>, resource_id: &str) -> Result<DownloadDescriptor, String> {
    { let mut state = sniffer.lock().map_err(|_| "Sniffer state is unavailable")?; state.descriptors.remove(resource_id); send(&mut state, json!({"id":format!("resolve:{resource_id}"), "command":"resolve_download", "resourceId":resource_id}))?; }
    for _ in 0..30 { std::thread::sleep(Duration::from_millis(100)); if let Some(descriptor) = sniffer.lock().ok().and_then(|mut state| state.descriptors.remove(resource_id)) { return Ok(descriptor); } }
    Err("The captured URL expired before it could be queued. Play the media again and retry.".into())
}

pub fn clear(app: &AppHandle, sniffer: &Arc<Mutex<SnifferState>>) -> Result<SnifferViewState, String> {
    let mut state = sniffer.lock().map_err(|_| "Sniffer state is unavailable")?; state.captures.clear(); state.descriptors.clear(); if state.status == "running" { let _ = send(&mut state, json!({"id":"clear", "command":"clear"})); }; emit_state(app, &state); Ok(state.view())
}

#[cfg(test)]
mod tests {
    use super::{keychain_contains_fingerprint, parse_proxy_state};

    #[test]
    fn parses_a_non_authenticated_proxy_snapshot() {
        let state = parse_proxy_state("Enabled: Yes\nServer: proxy.example\nPort: 8080\nAuthenticated Proxy Enabled: No\n").unwrap();
        assert!(state.enabled);
        assert_eq!(state.server, "proxy.example");
        assert_eq!(state.port, 8080);
    }

    #[test]
    fn refuses_to_replace_an_authenticated_proxy() {
        assert!(parse_proxy_state("Enabled: Yes\nServer: proxy.example\nPort: 8080\nAuthenticated Proxy Enabled: Yes\n").is_err());
    }

    #[test]
    fn matches_only_the_current_certificate_fingerprint() {
        let output = "SHA-256 hash: AABBCC\nSHA-1 hash: 112233\nSHA-256 hash: DDEEFF\n";
        assert!(keychain_contains_fingerprint(output, "ddeeff"));
        assert!(!keychain_contains_fingerprint(output, "001122"));
    }
}
