//! After a download fails because the configured loopback proxy is down, look
//! for a proxy the machine is actually using and offer to switch to it.
//!
//! Order: macOS system proxy (`scutil --proxy`, which prints the
//! SystemConfiguration dynamic store), then proxy environment variables, then
//! a short TCP probe of well-known local client ports. Callers decide whether
//! the failure itself was a connection error.

use std::collections::HashSet;

/// A proxy address that is not the one the app is already configured to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyCandidate {
    pub url: String,
    /// `system` (scutil), `env`, or `probe` (a common local port answered TCP).
    pub source: &'static str,
}

struct LocalProxyPort {
    port: u16,
    scheme: &'static str,
}

/// Clash, Surge, and V2Ray-family defaults. HTTP ports are listed before the
/// SOCKS port of the same client so a mixed install prefers an HTTP proxy,
/// which every engine in this app can speak.
const COMMON_LOCAL_PROXIES: &[LocalProxyPort] = &[
    LocalProxyPort {
        port: 7890,
        scheme: "http",
    }, // Clash / ClashX / mihomo
    LocalProxyPort {
        port: 7897,
        scheme: "http",
    }, // Clash Verge
    LocalProxyPort {
        port: 6152,
        scheme: "http",
    }, // Surge HTTP
    LocalProxyPort {
        port: 6153,
        scheme: "socks5",
    }, // Surge SOCKS
    LocalProxyPort {
        port: 1087,
        scheme: "http",
    }, // V2RayU HTTP
    LocalProxyPort {
        port: 1080,
        scheme: "socks5",
    }, // V2RayU / Shadowsocks
    LocalProxyPort {
        port: 10809,
        scheme: "http",
    }, // V2RayN HTTP
    LocalProxyPort {
        port: 10808,
        scheme: "socks5",
    }, // V2RayN SOCKS
];

const PROXY_ENV_KEYS: &[&str] = &[
    "https_proxy",
    "HTTPS_PROXY",
    "http_proxy",
    "HTTP_PROXY",
    "all_proxy",
    "ALL_PROXY",
    "socks_proxy",
    "SOCKS_PROXY",
    "socks5_proxy",
    "SOCKS5_PROXY",
];

pub fn proxy_switch_ack_key(current: &str, suggested: &str) -> String {
    format!(
        "{}|{}",
        canonical_proxy(current),
        canonical_proxy(suggested)
    )
}

pub fn same_proxy(left: &str, right: &str) -> bool {
    let left = canonical_proxy(left);
    let right = canonical_proxy(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left.eq_ignore_ascii_case(&right) {
        return true;
    }
    match (
        super::loopback_proxy_socket(&left),
        super::loopback_proxy_socket(&right),
    ) {
        (Some(left_addr), Some(right_addr)) => {
            left_addr == right_addr && scheme_of(&left) == scheme_of(&right)
        }
        _ => false,
    }
}

/// True when the text describes a proxy or transport failure. Login, HTTP
/// status, and disk errors stay false even if they mention the network in
/// passing — those are not a reason to rewrite the proxy.
pub fn is_proxy_connection_failure(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    if lower.trim().is_empty() || !has_connection_needle(&lower) {
        return false;
    }
    const EXCLUDE: &[&str] = &[
        "sign in to confirm",
        "not a bot",
        "disk write",
        "disk flush",
        "no url was provided",
        "playlist captures are not supported",
        "video mode only",
        "invalid proxy url",
    ];
    !EXCLUDE.iter().any(|needle| lower.contains(needle))
}

fn has_connection_needle(lower: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "connection refused",
        "connection reset",
        "connection aborted",
        "connect error",
        "error trying to connect",
        "tcp connect",
        "could not connect",
        "unable to connect",
        "failed to connect",
        "cannot connect",
        "can't connect",
        "failed to establish a new connection",
        "network is unreachable",
        "no route to host",
        "host is unreachable",
        "timed out",
        "timeout",
        "operation timed out",
        "name or service not known",
        "temporary failure in name resolution",
        "nodename nor servname",
        "tunnel connection failed",
        "proxyerror",
        "unable to connect to proxy",
        "proxy connection",
        "proxy connect",
        "through the configured proxy",
        "could not reach",
        "errno 61",
        "errno 60",
        "os error 61",
        "os error 60",
        "os error 111",
        "os error 110",
        "os error 101",
        "os error 113",
    ];
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

/// Rebuild a proxy URL we are willing to store. `None` for empty, non-proxy
/// schemes, or addresses with no host/port.
pub fn normalize_switch_target(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let parsed = reqwest::Url::parse(&with_scheme).ok()?;
    let scheme = match parsed.scheme() {
        "http" | "https" | "socks5" => parsed.scheme().to_string(),
        "socks5h" => "socks5".to_string(),
        _ => return None,
    };
    let host = parsed.host_str()?.trim_matches(['[', ']']);
    if host.is_empty() {
        return None;
    }
    let port = parsed.port_or_known_default()?;
    let user = parsed.username();
    if user.contains([':', '@', '/', '?', '#']) {
        return None;
    }
    let auth = if user.is_empty() {
        String::new()
    } else {
        match parsed.password() {
            Some(password) if !password.is_empty() && !password.contains(['@', '/', '?', '#']) => {
                format!("{user}:{password}@")
            }
            _ => format!("{user}@"),
        }
    };
    Some(format_proxy(&scheme, &auth, canonical_host(host), port))
}

/// Pick a replacement for `current`. `open_ports` is the set of loopback ports
/// that accepted a TCP connection; remote system/env proxies are trusted
/// without a probe.
pub fn discover_alternate_proxy(
    current: &str,
    scutil_text: Option<&str>,
    env: &[(&str, &str)],
    open_ports: &HashSet<u16>,
) -> Option<ProxyCandidate> {
    if let Some(url) = first_usable(
        current,
        scutil_candidates(scutil_text.unwrap_or("")),
        open_ports,
    ) {
        return Some(ProxyCandidate {
            url,
            source: "system",
        });
    }
    if let Some(url) = first_usable(current, env_candidates(env), open_ports) {
        return Some(ProxyCandidate { url, source: "env" });
    }
    let current_port = super::loopback_proxy_socket(current).map(|addr| addr.port());
    for entry in COMMON_LOCAL_PROXIES {
        if Some(entry.port) == current_port || !open_ports.contains(&entry.port) {
            continue;
        }
        let url = format_proxy(entry.scheme, "", "127.0.0.1", entry.port);
        if same_proxy(current, &url) {
            continue;
        }
        return Some(ProxyCandidate {
            url,
            source: "probe",
        });
    }
    None
}

/// Live lookup. On macOS this shells out to `scutil --proxy` (SystemConfiguration).
/// Other platforms only see the environment and open local ports.
pub fn discover_live_alternate(current: &str) -> Option<ProxyCandidate> {
    let scutil = macos_system_proxy_text();
    let env_owned = proxy_environment();
    let env: Vec<(&str, &str)> = env_owned
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let mut ports = Vec::new();
    for url in scutil_candidates(scutil.as_deref().unwrap_or(""))
        .into_iter()
        .chain(env_candidates(&env))
    {
        if let Some(addr) = super::loopback_proxy_socket(&url) {
            ports.push(addr.port());
        }
    }
    if let Some(current_port) = super::loopback_proxy_socket(current).map(|addr| addr.port()) {
        ports.retain(|port| *port != current_port);
    }
    for entry in COMMON_LOCAL_PROXIES {
        if super::loopback_proxy_socket(current).map(|addr| addr.port()) != Some(entry.port) {
            ports.push(entry.port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    let open = probe_ports(&ports);
    discover_alternate_proxy(current, scutil.as_deref(), &env, &open)
}

fn first_usable(current: &str, urls: Vec<String>, open_ports: &HashSet<u16>) -> Option<String> {
    urls.into_iter()
        .find(|url| usable(current, url, open_ports))
}

fn usable(current: &str, url: &str, open_ports: &HashSet<u16>) -> bool {
    if same_proxy(current, url) {
        return false;
    }
    match super::loopback_proxy_socket(url) {
        Some(addr) => open_ports.contains(&addr.port()),
        None => true,
    }
}

fn scutil_candidates(text: &str) -> Vec<String> {
    let map = scutil_map(text);
    let mut urls = Vec::new();
    // macOS "HTTPS proxy" is still an HTTP CONNECT proxy; the scheme is http.
    if flag_enabled(&map, "HTTPSEnable") {
        push_endpoint(&mut urls, &map, "HTTPSProxy", "HTTPSPort", "http");
    }
    if flag_enabled(&map, "HTTPEnable") {
        push_endpoint(&mut urls, &map, "HTTPProxy", "HTTPPort", "http");
    }
    if flag_enabled(&map, "SOCKSEnable") {
        push_endpoint(&mut urls, &map, "SOCKSProxy", "SOCKSPort", "socks5");
    }
    urls
}

fn env_candidates(env: &[(&str, &str)]) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    for key in PROXY_ENV_KEYS {
        let Some((_, value)) = env.iter().find(|(name, _)| name == key) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value == "-" || value.eq_ignore_ascii_case("none") {
            continue;
        }
        if let Some(url) = normalize_switch_target(value) {
            if !urls.iter().any(|existing| same_proxy(existing, &url)) {
                urls.push(url);
            }
        }
    }
    urls
}

fn push_endpoint(
    urls: &mut Vec<String>,
    map: &std::collections::HashMap<String, String>,
    host_key: &str,
    port_key: &str,
    scheme: &str,
) {
    let Some(host) = map.get(host_key).map(|value| value.trim()) else {
        return;
    };
    if host.is_empty() || host == "0.0.0.0" || host == "*" {
        return;
    }
    let Ok(port) = map
        .get(port_key)
        .map(|value| value.trim())
        .unwrap_or("")
        .parse::<u16>()
    else {
        return;
    };
    if port == 0 {
        return;
    }
    let url = format_proxy(scheme, "", canonical_host(host), port);
    if !urls.iter().any(|existing| same_proxy(existing, &url)) {
        urls.push(url);
    }
}

fn flag_enabled(map: &std::collections::HashMap<String, String>, key: &str) -> bool {
    matches!(map.get(key).map(String::as_str), Some("1"))
}

fn scutil_map(text: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() || value.starts_with('<') {
            continue;
        }
        map.insert(key.to_string(), value.to_string());
    }
    map
}

fn canonical_host(host: &str) -> &str {
    if host.eq_ignore_ascii_case("localhost") {
        "127.0.0.1"
    } else {
        host
    }
}

fn canonical_proxy(url: &str) -> String {
    super::normalize_proxy_url(url).replace("://localhost:", "://127.0.0.1:")
}

fn scheme_of(url: &str) -> String {
    url.split("://").next().unwrap_or("").to_ascii_lowercase()
}

fn format_proxy(scheme: &str, auth: &str, host: &str, port: u16) -> String {
    let host = host.trim_matches(['[', ']']);
    if host.contains(':') {
        format!("{scheme}://{auth}[{host}]:{port}")
    } else {
        format!("{scheme}://{auth}{host}:{port}")
    }
}

fn proxy_environment() -> Vec<(String, String)> {
    PROXY_ENV_KEYS
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
}

fn macos_system_proxy_text() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/sbin/scutil")
            .arg("--proxy")
            .output()
            .or_else(|_| std::process::Command::new("scutil").arg("--proxy").output())
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn probe_ports(ports: &[u16]) -> HashSet<u16> {
    if ports.is_empty() {
        return HashSet::new();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        for port in ports {
            let tx = tx.clone();
            let port = *port;
            scope.spawn(move || {
                let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
                if std::net::TcpStream::connect_timeout(
                    &addr,
                    std::time::Duration::from_millis(200),
                )
                .is_ok()
                {
                    let _ = tx.send(port);
                }
            });
        }
    });
    drop(tx);
    rx.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(ports: &[u16]) -> HashSet<u16> {
        ports.iter().copied().collect()
    }

    const SCUTIL_CLASH: &str = r#"
<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
  }
  HTTPEnable : 1
  HTTPPort : 7890
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7890
  HTTPSProxy : 127.0.0.1
  SOCKSEnable : 1
  SOCKSPort : 7891
  SOCKSProxy : 127.0.0.1
}
"#;

    #[test]
    fn scutil_prefers_https_proxy_and_ignores_disabled_stale_values() {
        let found = discover_alternate_proxy(
            "http://127.0.0.1:7897",
            Some(SCUTIL_CLASH),
            &[],
            &open(&[7890, 7891]),
        )
        .unwrap();
        assert_eq!(found.source, "system");
        assert_eq!(found.url, "http://127.0.0.1:7890");

        let socks_only = r#"
HTTPSEnable : 0
HTTPSPort : 7897
HTTPSProxy : 127.0.0.1
HTTPEnable : 0
HTTPPort : 7897
HTTPProxy : 127.0.0.1
SOCKSEnable : 1
SOCKSPort : 6153
SOCKSProxy : localhost
"#;
        let found = discover_alternate_proxy(
            "http://127.0.0.1:7897",
            Some(socks_only),
            &[],
            &open(&[6153]),
        )
        .unwrap();
        assert_eq!(found.url, "socks5://127.0.0.1:6153");

        // The enabled HTTPS entry is the dead port already configured; use HTTP.
        let split = "HTTPSEnable : 1\nHTTPSProxy : 127.0.0.1\nHTTPSPort : 7897\nHTTPEnable : 1\nHTTPProxy : localhost\nHTTPPort : 7890\nSOCKSEnable : 0\n";
        let found =
            discover_alternate_proxy("http://127.0.0.1:7897/", Some(split), &[], &open(&[7890]))
                .unwrap();
        assert_eq!(found.source, "system");
        assert_eq!(found.url, "http://127.0.0.1:7890");
    }

    #[test]
    fn dead_system_proxy_falls_through_to_env_then_common_ports() {
        let dead = r#"
HTTPSEnable : 1
HTTPSPort : 8888
HTTPSProxy : 127.0.0.1
HTTPEnable : 0
SOCKSEnable : 0
"#;
        let from_env = discover_alternate_proxy(
            "http://127.0.0.1:7897",
            Some(dead),
            &[("https_proxy", "http://127.0.0.1:7890/")],
            &open(&[7890]),
        )
        .unwrap();
        assert_eq!(from_env.source, "env");
        assert_eq!(from_env.url, "http://127.0.0.1:7890");

        let probed = discover_alternate_proxy(
            "http://127.0.0.1:7897",
            Some(dead),
            &[("https_proxy", "http://127.0.0.1:8888")],
            &open(&[1080, 7890, 6152]),
        )
        .unwrap();
        assert_eq!(probed.source, "probe");
        assert_eq!(probed.url, "http://127.0.0.1:7890");

        assert!(discover_alternate_proxy(
            "http://127.0.0.1:7897",
            Some("ProxyAutoConfigEnable : 1\nHTTPEnable : 0\n"),
            &[],
            &open(&[]),
        )
        .is_none());
    }

    #[test]
    fn probe_skips_the_configured_port_and_prefers_http() {
        let found = discover_alternate_proxy(
            "http://127.0.0.1:7890",
            None,
            &[],
            &open(&[7890, 7897, 6153]),
        )
        .unwrap();
        assert_eq!(found.source, "probe");
        assert_eq!(found.url, "http://127.0.0.1:7897");

        let socks =
            discover_alternate_proxy("http://127.0.0.1:7897", None, &[], &open(&[6153, 1080]))
                .unwrap();
        assert_eq!(socks.url, "socks5://127.0.0.1:6153");
    }

    #[test]
    fn remote_system_proxy_is_suggested_without_a_local_probe() {
        let text = "HTTPEnable : 1\nHTTPProxy : 10.1.2.3\nHTTPPort : 8080\nHTTPSEnable : 0\nSOCKSEnable : 0\n";
        let found =
            discover_alternate_proxy("http://127.0.0.1:7897", Some(text), &[], &open(&[])).unwrap();
        assert_eq!(found.source, "system");
        assert_eq!(found.url, "http://10.1.2.3:8080");
    }

    #[test]
    fn env_socks_and_bare_host_are_normalized_junk_is_dropped() {
        let found = discover_alternate_proxy(
            "http://127.0.0.1:7897",
            None,
            &[
                ("https_proxy", ""),
                ("HTTP_PROXY", "none"),
                ("all_proxy", "socks5://127.0.0.1:1080"),
                ("socks_proxy", "javascript:alert(1)"),
            ],
            &open(&[1080]),
        )
        .unwrap();
        assert_eq!(found.source, "env");
        assert_eq!(found.url, "socks5://127.0.0.1:1080");

        assert_eq!(
            normalize_switch_target("socks5h://127.0.0.1:1080").as_deref(),
            Some("socks5://127.0.0.1:1080")
        );
        assert!(normalize_switch_target("file:///etc/passwd").is_none());
        assert!(normalize_switch_target("http://\n127.0.0.1:1").is_none());
        assert_eq!(
            normalize_switch_target("127.0.0.1:6152").as_deref(),
            Some("http://127.0.0.1:6152")
        );
    }

    #[test]
    fn connection_failures_are_distinct_from_login_and_disk_errors() {
        assert!(is_proxy_connection_failure(
            "ERROR: Unable to download webpage: [Errno 61] Connection refused"
        ));
        assert!(is_proxy_connection_failure(
            "error sending request: client error (Connect): proxy connect: Connection refused (os error 111)"
        ));
        assert!(is_proxy_connection_failure(
            "gallery-dl could not reach the site through the configured proxy. Check the proxy settings and retry."
        ));
        assert!(is_proxy_connection_failure(
            "Media service timed out while connecting"
        ));
        assert!(!is_proxy_connection_failure(
            "YouTube requires local browser cookies. Please log in to YouTube in Chrome/Safari and try again."
        ));
        assert!(!is_proxy_connection_failure(
            "Bilibili rejected this request with HTTP 412. Please open Bilibili in Chrome, refresh the page, then retry."
        ));
        assert!(!is_proxy_connection_failure(
            "Disk write error: No space left on device"
        ));
        assert!(!is_proxy_connection_failure(
            "Invalid proxy URL 'http://[::1]': bad URL"
        ));
        assert!(!is_proxy_connection_failure("HTTP Error 404: Not Found"));
        assert!(!is_proxy_connection_failure(
            "No URL was provided for this download."
        ));
    }

    #[test]
    fn same_proxy_treats_localhost_and_trailing_slash_as_the_configured_port() {
        assert!(same_proxy(
            "http://127.0.0.1:7897/",
            "http://localhost:7897"
        ));
        assert!(!same_proxy(
            "http://127.0.0.1:7897",
            "socks5://127.0.0.1:7897"
        ));
        assert_eq!(
            proxy_switch_ack_key("http://localhost:7897/", "http://127.0.0.1:7890"),
            "http://127.0.0.1:7897|http://127.0.0.1:7890"
        );
    }

    #[test]
    fn probe_ports_sees_a_bound_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let open = probe_ports(&[port, 1]);
        assert!(open.contains(&port));
        assert!(!open.contains(&1));
    }
}
