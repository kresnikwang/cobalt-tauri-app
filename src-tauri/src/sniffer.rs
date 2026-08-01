use std::{collections::HashMap, io::{BufRead, BufReader, Write}, net::{SocketAddr, TcpStream, ToSocketAddrs}, path::PathBuf, process::{Child, ChildStdin, Command, Stdio}, sync::{Arc, Mutex}, time::Duration};

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
    pub cover_url: Option<String>,
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
    pub tun_proxy_detected: Option<String>,
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
    pub upstream: Option<String>,
    pub captures: HashMap<String, CapturedResource>,
    pub descriptors: HashMap<String, DownloadDescriptor>,
    stdin: Option<ChildStdin>,
    pub child: Option<Child>,
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
        Self { status: "stopped".into(), port: 8899, message: None, upstream: None, captures: HashMap::new(), descriptors: HashMap::new(), stdin: None, child: None, data_dir, certificate_path: Some(cert_path), certificate_fingerprint: None, certificate_installed: false, proxy_active: false, wechat_hooks: 0, wechat_callbacks: 0 }
    }
    pub fn view(&self) -> SnifferViewState {
        let mut captures: Vec<_> = self.captures.values().cloned().collect();
        captures.sort_by(|a, b| b.captured_at.cmp(&a.captured_at));
        SnifferViewState { status:self.status.clone(), port:self.port, message:self.message.clone(), tun_proxy_detected:detect_tun_proxy(), captures, supported_sources: vec!["WeChat Channels", "Douyin", "Bilibili", "Xiaohongshu", "Kuaishou", "QQ Music / KuGou", "Safari / Chrome Web Browsers"], certificate_path:self.certificate_path.as_ref().map(|path| path.to_string_lossy().into_owned()), certificate_fingerprint:self.certificate_fingerprint.clone(), certificate_installed:self.certificate_installed, proxy_active:self.proxy_active, wechat_hooks:self.wechat_hooks, wechat_callbacks:self.wechat_callbacks }
    }
}

// Clash Verge / Surge / sing-box etc. in TUN (fake-ip) mode hijack DNS and
// route every packet at the network layer. A local MITM proxy cannot capture
// that traffic: its upstream dials land back in the TUN and time out, and
// apps like WeChat Channels fail to open at all. The tool processes are
// almost always running even when TUN is off, so the real signal is the
// fake-ip route they install while active. Only warn when that route exists.
fn detect_tun_proxy() -> Option<String> {
    if !fake_ip_route_active() { return None; }
    let output = Command::new("/bin/ps").args(["-axo", "comm="]).output().ok()?;
    if !output.status.success() { return None; }
    Some(tun_proxy_name_from_ps(&String::from_utf8_lossy(&output.stdout)).unwrap_or_else(|| "a TUN-mode proxy".to_string()))
}

// TUN proxies in fake-ip mode install a route for the IANA benchmarking range
// (198.18.0.0/15) through their utun device. Presence of that route is live
// proof of interception; the process list alone cannot distinguish "TUN on"
// from "app installed but TUN off".
fn fake_ip_route_active() -> bool {
    match Command::new("/usr/sbin/netstat").args(["-rn", "-f", "inet"]).output() {
        Ok(output) if output.status.success() => fake_ip_route_in_netstat(&String::from_utf8_lossy(&output.stdout)),
        _ => false,
    }
}

fn fake_ip_route_in_netstat(netstat_output: &str) -> bool {
    netstat_output.lines().any(|line| {
        let destination = line.trim_start();
        destination.starts_with("198.18") || destination.starts_with("198.19")
    })
}

fn tun_proxy_name_from_ps(ps_output: &str) -> Option<String> {
    let mut candidates: Vec<String> = ps_output.lines().map(|line| line.trim().rsplit('/').next().unwrap_or("").to_lowercase()).filter(|name| !name.is_empty()).filter(|name| {
        let clash = name.starts_with("clash") || name.contains("mihomo") || name.contains("verge");
        let surge = name.contains("surge");
        let others = ["sing-box", "singbox", "v2ray", "xray", "shadowsocks", "ss-local", "trojan", "quantumult", "stash", "karing", "hiddify", "loon"].iter().any(|marker| name.contains(marker));
        clash || surge || others
    }).collect();
    if candidates.len() > 1 {
        candidates.retain(|name| !name.contains("service"));
    }
    candidates.into_iter().next().map(|name| {
        // Capitalize the first letter for a friendlier display name.
        let mut chars = name.chars();
        match chars.next() {
            Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
            None => name,
        }
    })
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
    let port = guard.port;
    guard.upstream = upstream_proxy.filter(|proxy| !proxy.trim().is_empty());
    let upstream = guard.upstream.clone().unwrap_or_default();
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

// Cobalt only hijacks the network service that owns the default route, never
// every interface with an IP. Touching all services is what made a whole
// machine lose connectivity when the capture proxy was not actually serving.
fn default_route_interface() -> Option<String> {
    let output = Command::new("route").args(["-n", "get", "default"]).output().ok()?;
    if !output.status.success() { return None; }
    String::from_utf8_lossy(&output.stdout).lines().find_map(|line| line.trim().strip_prefix("interface: ").map(str::trim).map(str::to_string))
}

fn service_for_interface(order_output: &str, interface: &str) -> Option<String> {
    let mut pending_service: Option<String> = None;
    for line in order_output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("(Hardware Port:") {
            if let Some(service) = pending_service.take() {
                if trimmed.contains(&format!("Device: {interface}")) {
                    return Some(service);
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix('(') {
            // Service order line like "(1) Wi-Fi"
            if let Some((_, name)) = rest.split_once(')') {
                let name = name.trim();
                if !name.is_empty() {
                    pending_service = Some(name.to_string());
                }
            }
        }
    }
    None
}

fn primary_proxy_services() -> Result<Vec<String>, String> {
    if let Some(interface) = default_route_interface() {
        if let Ok(order) = command_output(&["-listnetworkserviceorder"]) {
            if let Some(service) = service_for_interface(&order, &interface) {
                return Ok(vec![service]);
            }
        }
    }
    // Fallback: the first IP-bearing service in the macOS service order.
    if let Ok(order) = command_output(&["-listnetworkserviceorder"]) {
        for line in order.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix('(') {
                if let Some((_, name)) = rest.split_once(')') {
                    let name = name.trim();
                    if !name.is_empty() && !name.contains("Serial Port") {
                        if command_output(&["-getinfo", name]).map(|info| info.contains("IP address:")).unwrap_or(false) {
                            return Ok(vec![name.to_string()]);
                        }
                    }
                }
            }
        }
    }
    Err("No active macOS network service was found for proxy capture.".into())
}

// The sidecar can report "running" before its listener is actually accepting
// connections. Pointing the whole system at a dead port would cut all traffic.
fn local_port_is_listening(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(400)).is_ok()
}

// The capture proxy forwards everything through Cobalt's configured proxy.
// Enabling the system proxy with an unreachable upstream is how the whole
// network appears to "disconnect", so refuse loudly instead.
fn proxy_endpoint_reachable(proxy_url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(proxy_url).map_err(|error| format!("Invalid proxy URL ({proxy_url}): {error}"))?;
    let host = parsed.host_str().ok_or_else(|| format!("Proxy URL ({proxy_url}) has no host"))?;
    let port = parsed.port_or_known_default().ok_or_else(|| format!("Proxy URL ({proxy_url}) has no port"))?;
    let socket = format!("{host}:{port}").to_socket_addrs().map_err(|error| format!("Cannot resolve proxy {proxy_url}: {error}"))?.next().ok_or_else(|| format!("Cannot resolve proxy {proxy_url}"))?;
    if TcpStream::connect_timeout(&socket, Duration::from_millis(800)).is_err() {
        return Err(format!("Cobalt's configured proxy ({proxy_url}) is not reachable. Start it, or disable \"Use proxy\" in Cobalt settings, before enabling the system proxy."));
    }
    Ok(())
}

pub fn enable_system_proxy(app: &AppHandle, sniffer: &Arc<Mutex<SnifferState>>) -> Result<SnifferViewState, String> {
    let mut state = sniffer.lock().map_err(|_| "Sniffer state is unavailable")?;
    if state.status != "running" { return Err("Start the resource sniffer before enabling its system proxy.".into()); }
    if !state.certificate_installed {
		return Err("Install and verify Cobalt's local capture certificate before enabling the system proxy.".into());
    }
    if let Some(app_name) = detect_tun_proxy() {
        return Err(format!("TUN-mode proxy ({app_name}) is running. It conflicts with local capture: WeChat Channels may fail to open. Turn off TUN mode, or use the Channels web version in a browser instead."));
    }
    if !local_port_is_listening(state.port) {
        return Err(format!("The capture proxy is not actually listening on 127.0.0.1:{}. Stop and start capture, then retry.", state.port));
    }
    if let Some(upstream) = state.upstream.as_deref() {
        proxy_endpoint_reachable(upstream)?;
    }
    let services = primary_proxy_services()?;
    let mut snapshot = Vec::new();
    for service in &services { let web = parse_proxy_state(&command_output(&["-getwebproxy", service])?)?; let secure = parse_proxy_state(&command_output(&["-getsecurewebproxy", service])?)?; snapshot.push(ProxyServiceSnapshot { service: service.clone(), web, secure }); }
    let content = serde_json::to_vec_pretty(&snapshot).map_err(|error| error.to_string())?; std::fs::write(proxy_snapshot_path(&state), content).map_err(|error| error.to_string())?;
    let apply_result = (|| -> Result<(), String> {
        for item in &snapshot { command_output(&["-setwebproxy", &item.service, "127.0.0.1", &state.port.to_string()])?; command_output(&["-setsecurewebproxy", &item.service, "127.0.0.1", &state.port.to_string()])?; command_output(&["-setwebproxystate", &item.service, "on"])?; command_output(&["-setsecurewebproxystate", &item.service, "on"])?; }
        Ok(())
    })();
    if let Err(error) = apply_result {
        // Never leave the machine half-pointed at the capture proxy.
        let mut message = format!("Failed to enable the system proxy: {error}");
        match restore_system_proxy(&mut state) {
            Ok(()) => message.push_str(" The previous proxy configuration was restored."),
            Err(rollback) => message.push_str(&format!(" Rollback also failed: {rollback}")),
        }
        return Err(message);
    }
    state.proxy_active = true; state.message = Some("System proxy is active on the primary network service. Play a video in WeChat Channels now.".into()); emit_state(app, &state); Ok(state.view())
}

pub fn restore_system_proxy(state: &mut SnifferState) -> Result<(), String> {
    let path = proxy_snapshot_path(state); if !path.exists() { state.proxy_active = false; return Ok(()); }
    let snapshots: Vec<ProxyServiceSnapshot> = serde_json::from_slice(&std::fs::read(&path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let mut failures = Vec::new();
    for item in snapshots {
        for (kind, saved) in [("web", item.web), ("secure", item.secure)] {
            let get = if kind == "web" { "-getwebproxy" } else { "-getsecurewebproxy" };
            let set = if kind == "web" { "-setwebproxy" } else { "-setsecurewebproxy" };
            let toggle = if kind == "web" { "-setwebproxystate" } else { "-setsecurewebproxystate" };
            // Only touch services that are still pointed at Cobalt's capture
            // proxy. If the user (or another app) changed a proxy after the
            // snapshot, restoring the stale value would break their setup.
            let current = match command_output(&[get, &item.service]) {
                Ok(output) => match parse_proxy_state(&output) {
                    Ok(current) => current,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            if current.server != "127.0.0.1" || current.port != state.port { continue; }
            let result = if saved.enabled {
                command_output(&[set, &item.service, &saved.server, &saved.port.to_string()])
                    .and_then(|_| command_output(&[toggle, &item.service, "on"]))
            } else {
                command_output(&[toggle, &item.service, "off"])
            };
            if let Err(error) = result { failures.push(format!("{} ({kind}): {error}", item.service)); }
        }
    }
    if failures.is_empty() {
        let _ = std::fs::remove_file(path);
        state.proxy_active = false;
        Ok(())
    } else {
        Err(format!("Could not fully restore the system proxy: {}", failures.join("; ")))
    }
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
    use super::{fake_ip_route_in_netstat, keychain_contains_fingerprint, parse_proxy_state, service_for_interface, tun_proxy_name_from_ps};

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

    #[test]
    fn maps_the_service_order_to_an_interface() {
        let order = "(1) Wi-Fi\n(Hardware Port: Wi-Fi, Device: en0)\n\n(2) USB 10/100/1000 LAN\n(Hardware Port: USB 10/100/1000 LAN, Device: en7)\n";
        assert_eq!(service_for_interface(order, "en0"), Some("Wi-Fi".to_string()));
        assert_eq!(service_for_interface(order, "en7"), Some("USB 10/100/1000 LAN".to_string()));
        assert_eq!(service_for_interface(order, "en5"), None);
    }

    #[test]
    fn detects_tun_mode_proxy_processes() {
        let ps = "/usr/bin/login\n/Applications/Clash Verge.app/Contents/MacOS/clash-verge\n/Applications/Clash Verge.app/Contents/MacOS/verge-mihomo\n/usr/bin/ssh\n";
        assert_eq!(tun_proxy_name_from_ps(ps), Some("Clash-verge".to_string()));
        let clean = "/usr/bin/login\n/sbin/launchd\n/usr/sbin/cfprefsd\n";
        assert_eq!(tun_proxy_name_from_ps(clean), None);
    }

    #[test]
    fn detects_only_a_live_fake_ip_route() {
        let active = "Internet:\nDestination        Gateway            Flags           Netif Expire\ndefault            192.168.0.1        UGScg                 en0\n198.18.0.0/16      198.18.0.1          UGSc              utun4\n";
        assert!(fake_ip_route_in_netstat(active));
        let idle = "Internet:\ndefault            192.168.0.1        UGScg                 en0\n192.168.0          link#14            UCS                   en0\n";
        assert!(!fake_ip_route_in_netstat(idle));
    }
}
