# Resource Sniffer and WeChat Channels Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add an explicit macOS resource-sniffing mode that captures and downloads media from WeChat Channels, Mini Programs, Douyin, Kuaishou, Xiaohongshu, Kugou, QQ Music, and ordinary HLS/direct-media traffic without weakening the existing URL download workflow.

**Architecture:** Extract and adapt the Apache-2.0 Go proxy core from [`putyy/res-downloader`](https://github.com/putyy/res-downloader) into a headless sidecar. The Tauri Rust core owns the sidecar lifecycle, macOS proxy/certificate setup, exact proxy restoration, sanitized event forwarding, and conversion of captured resources into existing download tasks. Svelte exposes URL mode and Sniffer mode as separate workflows; it never receives raw cookies, authorization headers, or the generated CA private key.

**Tech Stack:** Tauri v2, Rust/Tokio/Reqwest, Svelte 5, Go 1.23, `elazarl/goproxy`, bundled Node.js for the initial WeChat decrypt-key helper, bundled FFmpeg for HLS/DASH, macOS `networksetup` and Keychain tooling.

---

## Decision Summary

- Ship a headless Go sidecar, not the upstream Wails application or Vue frontend.
- Preserve the current URL downloader unchanged and add Sniffer as a second mode.
- Scope the first release to macOS Apple Silicon; add other platforms only after macOS recovery and signing are stable.
- Generate a unique CA for each installation. Never copy the fixed certificate/private key embedded by the upstream project.
- MITM only selected domain presets by default. Do not use the upstream `Rule: "*"` default.
- Save and restore the exact HTTP/HTTPS proxy state for every changed network service. Never restore by simply switching proxies off.
- Keep raw URLs, cookies, tokens, and request headers inside the sidecar/Rust process. Emit only sanitized resource cards to Svelte.
- Reuse the existing outbound HTTP proxy setting as the sniffer's optional upstream proxy. The local sniffer listens on `127.0.0.1:8899` and chains to the configured upstream proxy.
- Treat generic MIME sniffing as best-effort. Only label a platform as supported after a real fixture/manual test passes.
- Attribute the upstream Apache-2.0 work in source, release artifacts, and `THIRD_PARTY_NOTICES.md`.

## User Workflow

1. User switches from **URL** to **Sniffer** mode.
2. On first use, Cobalt explains HTTPS inspection and asks the user to install a per-install certificate.
3. User selects a domain preset and presses **Start Sniffing**.
4. Cobalt snapshots current network proxy settings, starts the sidecar, then sets HTTP/HTTPS system proxy to `127.0.0.1:8899`.
5. User opens WeChat or another target application and plays the media.
6. Sanitized captures appear in Cobalt with type, source, size, resolution, and cover.
7. User presses Download. Rust resolves the private descriptor and queues it in the existing task system.
8. Pressing Stop, quitting Cobalt, or reopening after an interrupted session restores the previous proxy configuration.

## Non-Goals for the First Release

- iOS/Android packet capture.
- Automatic DRM removal or content protected by platform DRM.
- Capturing pinned-TLS applications that reject a user-installed CA.
- Automatic background sniffing at app launch.
- A generic “decrypt everything” mode.
- Storing or synchronizing captured cookies to the remote Cobalt service.
- SOCKS upstream proxies in the first iteration; support HTTP/HTTPS upstream proxy URLs first.

### Task 1: Pin the upstream source and license boundary

**Files:**
- Create: `third_party/res-downloader/LICENSE`
- Create: `third_party/res-downloader/UPSTREAM.md`
- Create: `THIRD_PARTY_NOTICES.md`
- Modify: `README.md`

**Step 1: Record the upstream revision**

Pin upstream version `3.1.3` and commit `45d5e9eddc6c2b0baebc41cd5edced32ec3c67b8` in `UPSTREAM.md`. Record which files are adapted and which are intentionally excluded.

**Step 2: Copy the Apache-2.0 license**

Copy the unmodified upstream `LICENSE` to `third_party/res-downloader/LICENSE`.

**Step 3: Add attribution**

Add this entry to `THIRD_PARTY_NOTICES.md`:

```markdown
## res-downloader

Portions of the resource-sniffing sidecar are adapted from
putyy/res-downloader, licensed under Apache License 2.0.
Upstream: https://github.com/putyy/res-downloader
Pinned revision: 45d5e9eddc6c2b0baebc41cd5edced32ec3c67b8
```

**Step 4: Document the security rewrite**

State that Cobalt does not reuse upstream's embedded CA key, Wails UI, password cache, or blanket MITM default.

**Step 5: Verify notices are packaged**

Run:

```bash
rg -n "res-downloader|Apache" README.md THIRD_PARTY_NOTICES.md third_party/res-downloader
```

Expected: upstream URL, revision, and Apache-2.0 attribution are present.

**Step 6: Commit**

```bash
git add README.md THIRD_PARTY_NOTICES.md third_party/res-downloader
git commit -m "docs: record resource sniffer upstream attribution"
```

### Task 2: Create the headless Go sidecar and JSONL protocol

**Files:**
- Create: `sidecars/res-sniffer/go.mod`
- Create: `sidecars/res-sniffer/cmd/res-sniffer/main.go`
- Create: `sidecars/res-sniffer/internal/protocol/message.go`
- Create: `sidecars/res-sniffer/internal/protocol/message_test.go`
- Create: `sidecars/res-sniffer/README.md`

**Step 1: Write protocol tests**

Cover valid commands, unknown methods, malformed JSON, and messages larger than the 1 MiB limit.

```go
func TestDecodeStartCommand(t *testing.T) {
    raw := `{"id":"1","method":"start","params":{"listen":"127.0.0.1:8899"}}`
    got, err := DecodeCommand(strings.NewReader(raw))
    require.NoError(t, err)
    require.Equal(t, "start", got.Method)
}
```

**Step 2: Run the failing tests**

Run:

```bash
cd sidecars/res-sniffer
go test ./internal/protocol
```

Expected: FAIL because the protocol package does not exist yet.

**Step 3: Implement newline-delimited JSON**

Support these commands:

```json
{"id":"1","method":"status"}
{"id":"2","method":"start","params":{"listen":"127.0.0.1:8899","mitmDomains":["*.qq.com"],"upstreamProxy":""}}
{"id":"3","method":"set_filters","params":{"types":["video","audio","m3u8"]}}
{"id":"4","method":"resolve_download","params":{"captureId":"capture-id"}}
{"id":"5","method":"clear"}
{"id":"6","method":"stop"}
```

Emit:

```json
{"type":"ready","payload":{"listen":"127.0.0.1:8899","certPath":"...","fingerprint":"..."}}
{"type":"resource","payload":{"id":"...","domain":"qq.com","kind":"video","size":1234,"requiresDecrypt":true}}
{"type":"error","payload":{"code":"PORT_IN_USE","message":"..."}}
```

**Step 4: Add protocol safety**

- Bind only to loopback.
- Limit one JSON message to 1 MiB.
- Write logs to stderr and protocol messages to stdout.
- Never print headers, cookies, query tokens, CA private keys, or proxy credentials.

**Step 5: Run tests**

Run:

```bash
cd sidecars/res-sniffer
go test ./...
```

Expected: PASS.

**Step 6: Commit**

```bash
git add sidecars/res-sniffer
git commit -m "feat: add headless resource sniffer sidecar"
```

### Task 3: Generate a unique CA and restrict MITM domains

**Files:**
- Create: `sidecars/res-sniffer/internal/cert/authority.go`
- Create: `sidecars/res-sniffer/internal/cert/authority_test.go`
- Create: `sidecars/res-sniffer/internal/rules/rules.go`
- Create: `sidecars/res-sniffer/internal/rules/rules_test.go`
- Create: `sidecars/res-sniffer/internal/proxy/server.go`

**Step 1: Write certificate tests**

Verify two fresh app-data directories generate different CA keys and stable fingerprints on subsequent loads.

**Step 2: Write rule tests**

Test exact domains, wildcard subdomains, excluded domains, host-with-port handling, and rejection of `*` unless an explicit advanced flag is set.

**Step 3: Run failing tests**

Run:

```bash
cd sidecars/res-sniffer
go test ./internal/cert ./internal/rules
```

Expected: FAIL because implementations are missing.

**Step 4: Implement per-install CA storage**

- Generate RSA-3072 or ECDSA P-256 CA with `crypto/x509`.
- Store `sniffer-ca.crt` as `0644`.
- Store `sniffer-ca.key` as `0600`.
- Place files under the app-data directory provided by Rust.
- Return only certificate path and SHA-256 fingerprint over JSONL.

**Step 5: Implement safe domain presets**

Recommended initial preset:

```text
channels.weixin.qq.com
*.weixin.qq.com
*.wx.qq.com
*.qq.com
*.douyin.com
*.kuaishou.com
*.xiaohongshu.com
*.kugou.com
*.qqmusic.qq.com
```

Keep banking, identity, password-manager, software-update, and Apple account traffic outside MITM.

**Step 6: Start goproxy**

Adapt `core/proxy.go` from upstream but remove Wails globals, fixed CA material, and `Rule: "*"`.

**Step 7: Run tests**

Run:

```bash
cd sidecars/res-sniffer
go test ./...
go vet ./...
```

Expected: PASS with no vet diagnostics.

**Step 8: Commit**

```bash
git add sidecars/res-sniffer/internal
git commit -m "feat: add per-install sniffer CA and scoped MITM rules"
```

### Task 4: Add sanitized generic media capture

**Files:**
- Create: `sidecars/res-sniffer/internal/capture/model.go`
- Create: `sidecars/res-sniffer/internal/capture/store.go`
- Create: `sidecars/res-sniffer/internal/capture/store_test.go`
- Create: `sidecars/res-sniffer/internal/plugins/default.go`
- Create: `sidecars/res-sniffer/internal/plugins/default_test.go`

**Step 1: Write MIME classification tests**

Cover direct MP4/WebM, audio, HLS, DASH, FLV, images, missing content length, 206 responses, duplicates, and non-media responses.

**Step 2: Define public and private models**

```go
type PublicCapture struct {
    ID              string `json:"id"`
    Domain          string `json:"domain"`
    Kind            string `json:"kind"`
    Size            int64  `json:"size"`
    ContentType     string `json:"contentType"`
    Description     string `json:"description,omitempty"`
    CoverURL        string `json:"coverUrl,omitempty"`
    RequiresDecrypt bool   `json:"requiresDecrypt"`
}

type DownloadDescriptor struct {
    URL       string
    Headers   http.Header
    Suffix    string
    DecodeKey string
}
```

Only `PublicCapture` may be emitted to Svelte. `DownloadDescriptor` stays in the in-memory sidecar store.

**Step 3: Filter sensitive data**

- Do not persist `Authorization`, `Cookie`, `Set-Cookie`, signed URLs, or decode keys.
- Store private descriptors only in memory.
- Clear descriptors on Stop, Clear, or process exit.
- Deduplicate by normalized URL hash without logging the URL.

**Step 4: Adapt the upstream default plugin**

Reuse content-type classification and original request headers, but enforce a configurable minimum size and type filters.

**Step 5: Run tests**

Run:

```bash
cd sidecars/res-sniffer
go test ./internal/capture ./internal/plugins
```

Expected: PASS.

**Step 6: Commit**

```bash
git add sidecars/res-sniffer/internal/capture sidecars/res-sniffer/internal/plugins
git commit -m "feat: capture sanitized media resources"
```

### Task 5: Add WeChat Channels metadata extraction and decryption

**Files:**
- Create: `sidecars/res-sniffer/internal/plugins/wechat.go`
- Create: `sidecars/res-sniffer/internal/plugins/wechat_test.go`
- Create: `sidecars/res-sniffer/testdata/wechat/feed.js`
- Create: `scripts/wechat-decrypt.mjs`
- Create: `scripts/wechat-decrypt.test.mjs`
- Modify: `scripts/prepare-release-bundle.mjs`

**Step 1: Create synthetic fixtures**

Create minimal JavaScript fixtures containing the method signatures used by the upstream injection. Do not commit full WeChat production bundles.

**Step 2: Write injection tests**

Verify:

- matching WeChat JS is modified once;
- unrelated JS is unchanged;
- already-modified content is not modified twice;
- compressed responses are decoded/re-encoded correctly;
- injection failure emits a diagnostic instead of breaking the page.

**Step 3: Adapt the upstream QQ/WeChat plugin**

Preserve the object-description extraction behavior from `core/plugins/plugin.qq.com.go`, but use a loopback callback endpoint owned by the sidecar instead of a public-looking `wxapp.tc.qq.com` URL.

**Step 4: Add decrypt-key helper tests**

Create one fixed `decodeKey` test vector and expected first-block XOR output.

**Step 5: Extract the upstream WxIsaac64 helper**

Adapt the Apache-2.0 `frontend/src/assets/js/decrypt.js` into a backend-only Node script:

```bash
node scripts/wechat-decrypt.mjs --key "$FIXTURE_KEY"
```

Output base64 to stdout. Never pass the key in logs. Prefer stdin in production so it does not appear in the process list.

**Step 6: Package the helper**

Update `prepare-release-bundle.mjs` to copy the helper beside bundled Node and verify it can process the test vector.

**Step 7: Run tests**

Run:

```bash
cd sidecars/res-sniffer
go test ./internal/plugins
cd ../..
node --test scripts/wechat-decrypt.test.mjs
```

Expected: PASS.

**Step 8: Commit**

```bash
git add sidecars/res-sniffer scripts
git commit -m "feat: detect and decode WeChat Channels media"
```

### Task 6: Add the Rust sidecar supervisor

**Files:**
- Create: `src-tauri/src/sniffer/mod.rs`
- Create: `src-tauri/src/sniffer/model.rs`
- Create: `src-tauri/src/sniffer/protocol.rs`
- Create: `src-tauri/src/sniffer/supervisor.rs`
- Create: `src-tauri/src/sniffer/protocol_test.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Write protocol parsing tests**

Test ready/resource/error messages, malformed JSON, sidecar EOF, duplicated capture IDs, and redaction of accidental secrets.

**Step 2: Define Rust state**

```rust
pub struct SnifferState {
    pub status: SnifferStatus,
    pub child: Option<tokio::process::Child>,
    pub captures: HashMap<String, PublicCapture>,
    pub proxy_snapshot: Option<ProxySnapshot>,
}
```

Store it separately from the existing `AppState` mutex to avoid blocking download progress while reading sidecar output.

**Step 3: Resolve and launch the bundled binary**

- Resolve `Resources/binaries/res-sniffer`.
- Pass app-data directory and parent PID as arguments.
- Pipe stdin/stdout/stderr.
- Reject sidecar versions incompatible with the Rust protocol version.
- Emit `sniffer-status` and `sniffer-resource` Tauri events.

**Step 4: Handle failure**

If the sidecar exits unexpectedly:

- mark status failed;
- restore the system proxy;
- clear private capture handles;
- emit a user-facing recovery message.

**Step 5: Add Tauri commands**

```rust
get_sniffer_state
install_sniffer_certificate
remove_sniffer_certificate
start_sniffer
stop_sniffer
clear_sniffer_captures
queue_captured_resource
restore_system_proxy
```

**Step 6: Run tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml sniffer
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

**Step 7: Commit**

```bash
git add src-tauri/src/sniffer src-tauri/src/lib.rs
git commit -m "feat: supervise resource sniffer from Tauri"
```

### Task 7: Make macOS proxy changes recoverable

**Files:**
- Create: `src-tauri/src/sniffer/macos_proxy.rs`
- Create: `src-tauri/src/sniffer/macos_proxy_test.rs`
- Create: `src-tauri/src/sniffer/admin.rs`
- Create: `src-tauri/resources/sniffer-domain-presets.json`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Write parser tests**

Use fixtures for:

```text
Enabled: Yes
Server: 127.0.0.1
Port: 7897
Authenticated Proxy Enabled: 0
```

Test multiple services, disabled proxies, bypass domains, non-ASCII service names, and partially configured HTTP/HTTPS proxies.

**Step 2: Snapshot exact state**

Before changing anything, capture each active service's:

- HTTP proxy enabled/server/port;
- HTTPS proxy enabled/server/port;
- bypass domains;
- network service name.

Persist the snapshot atomically to `sniffer-proxy-session.json`.

**Step 3: Add native authorization flow**

For the first implementation, invoke a fixed, bundled admin helper through a macOS authorization prompt. The helper must accept a validated operation file, not arbitrary shell text. Never ask for or cache the user's administrator password.

Allowed operations:

```text
install-certificate
remove-certificate
apply-proxy-snapshot
restore-proxy-snapshot
```

**Step 4: Restore exact settings**

On Stop and normal app exit, restore every captured value. Do not call only `-setwebproxystate off`.

**Step 5: Add startup recovery**

If `sniffer-proxy-session.json` exists at app startup, show a blocking recovery banner and attempt restoration before allowing a new sniffing session.

**Step 6: Add emergency recovery**

Expose **Restore Network Settings** even when sidecar startup fails. Keep the snapshot until restoration is verified.

**Step 7: Run tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml macos_proxy
```

Expected: PASS without changing the developer machine's live network settings.

**Step 8: Commit**

```bash
git add src-tauri/src/sniffer src-tauri/resources src-tauri/src/lib.rs
git commit -m "feat: safely manage and restore macOS proxy settings"
```

### Task 8: Convert captures into the existing download queue

**Files:**
- Create: `src-tauri/src/download/captured.rs`
- Create: `src-tauri/src/download/hls.rs`
- Create: `src-tauri/src/download/captured_test.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Add an internal task source**

Keep the public task model compatible while adding:

```rust
enum TaskSource {
    Url(String),
    Capture { capture_id: String },
}
```

Do not persist private descriptors. Persist only `capture://<id>` for display/restart diagnostics.

**Step 2: Resolve a capture at queue time**

`queue_captured_resource` asks the sidecar for the private descriptor, validates URL scheme and header limits, and stores it in an in-memory task context.

**Step 3: Reuse direct download progress**

For direct files:

- replay required request headers;
- use the configured upstream proxy when enabled;
- preserve cancel/progress/speed/ETA behavior;
- remove partial files on cancellation.

**Step 4: Add HLS/DASH through FFmpeg**

For m3u8/mpd descriptors, run bundled FFmpeg with sanitized headers and proxy environment. Parse progress into the existing `DownloadTask` events.

**Step 5: Add WeChat decryption**

After direct download:

1. send `decodeKey` to bundled Node over stdin;
2. read the base64 XOR array;
3. XOR only the required leading bytes;
4. verify the resulting media signature;
5. atomically replace the encrypted temporary file.

**Step 6: Handle expired captures**

Return a specific `CAPTURE_EXPIRED` error and instruct the user to replay the media. Do not silently retry through the remote server.

**Step 7: Run tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml captured
pnpm release:check
```

Expected: PASS.

**Step 8: Commit**

```bash
git add src-tauri/src/download src-tauri/src/lib.rs
git commit -m "feat: download captured direct and streaming media"
```

### Task 9: Add Sniffer mode to the Svelte interface

**Files:**
- Create: `src/lib/sniffer/types.ts`
- Create: `src/lib/sniffer/store.svelte.ts`
- Create: `src/lib/components/SnifferPanel.svelte`
- Create: `src/lib/components/CaptureCard.svelte`
- Create: `src/lib/components/SnifferSetupDialog.svelte`
- Modify: `src/routes/+page.svelte`
- Modify: `src/app.css`

**Step 1: Add typed event models**

Define `SnifferStatus`, `PublicCapture`, and command response types. Remove `any` from new code.

**Step 2: Add a mode control**

Use a segmented control:

```text
[ URL Download ] [ Resource Sniffer ]
```

Default to URL Download. Do not start sniffing when the mode changes.

**Step 3: Build onboarding**

Explain:

- Cobalt will temporarily inspect HTTPS traffic for selected media domains;
- a local CA certificate is required;
- existing proxy settings will be restored;
- the user should download only content they are authorized to save.

Actions: **Install Certificate**, **Start Sniffing**, **Cancel**.

**Step 4: Build the active sniffer panel**

Show:

- stopped/starting/listening/restoring/error state;
- Start/Stop button;
- local port;
- selected domain preset;
- upstream proxy status;
- filters for Video, Audio, HLS/DASH, Images;
- **Restore Network Settings** emergency action.

**Step 5: Build capture cards**

Each card shows source domain, media type, size, resolution if known, cover if available, encryption badge, and Download/Remove actions.

Do not show raw signed URLs, cookies, or headers.

**Step 6: Add accessible interactions**

- 40 px minimum hit targets;
- keyboard focus states;
- confirmation before certificate removal during an active session;
- disabled states while starting/stopping/restoring;
- no dead platform cards.

**Step 7: Add component tests**

Test stopped, first-run, active, capture list, empty list, expired capture, and recovery-required states.

**Step 8: Run frontend checks**

Run:

```bash
pnpm check
pnpm build
```

Expected: zero errors and zero warnings.

**Step 9: Commit**

```bash
git add src/lib/sniffer src/lib/components src/routes/+page.svelte src/app.css
git commit -m "feat: add resource sniffer workflow"
```

### Task 10: Add proxy settings, domain presets, and translations

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/i18n/en.json`
- Modify: `src/lib/i18n/zh.json`
- Modify: `src/lib/i18n/ru.json`
- Modify: `src/routes/+page.svelte`
- Modify: `src/lib/services.ts`

**Step 1: Separate proxy concepts in copy**

Rename the existing setting to **Upstream Download Proxy**. Explain that it is also used by the sniffer sidecar when forwarding restricted traffic.

**Step 2: Add sniffer settings**

```rust
pub sniffer_port: u16,                 // default 8899
pub sniffer_auto_system_proxy: bool,   // default true
pub sniffer_domain_preset: String,     // default "recommended"
pub sniffer_capture_types: Vec<String>
```

Validate port range, loopback binding, and upstream URL scheme in Rust.

**Step 3: Add presets**

- Recommended media domains.
- WeChat only.
- Chinese social platforms.
- Music platforms.
- Custom advanced allowlist.

Require an explicit warning before an all-domain advanced mode.

**Step 4: Add translations**

Add all setup, status, recovery, certificate, capture, proxy, and error strings in Chinese, English, and Russian.

**Step 5: Update supported-platform labels**

Add WeChat Channels only after its end-to-end test passes. Mark the remaining sites as **Sniffer preview** until verified individually.

**Step 6: Run checks**

Run:

```bash
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

**Step 7: Commit**

```bash
git add src-tauri/src/lib.rs src/lib src/routes/+page.svelte
git commit -m "feat: add sniffer proxy settings and translations"
```

### Task 11: Package and sign the sidecar

**Files:**
- Modify: `package.json`
- Modify: `scripts/prepare-release-bundle.mjs`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `RELEASE.md`
- Modify: `.github/workflows/release.yml`

**Step 1: Add build scripts**

```json
{
  "build:sniffer": "cd sidecars/res-sniffer && CGO_ENABLED=0 GOOS=darwin GOARCH=arm64 go build -trimpath -ldflags='-s -w' -o ../../src-tauri/binaries/res-sniffer ./cmd/res-sniffer",
  "check:sniffer": "cd sidecars/res-sniffer && go test ./... && go vet ./..."
}
```

**Step 2: Verify reproducible inputs**

Pin Go module versions and run `go mod verify`. Record sidecar protocol and upstream revision in `--version` output.

**Step 3: Bundle resources**

Add:

```json
"binaries/res-sniffer",
"resources/sniffer-domain-presets.json",
"../THIRD_PARTY_NOTICES.md"
```

Do not package the upstream private key or Wails assets.

**Step 4: Update release validation**

`release:check` must run Svelte checks, Rust tests/checks, Go tests/vet, Node decrypt-vector tests, and sidecar `--self-test`.

**Step 5: Sign nested executables**

For Developer ID builds:

1. sign `res-sniffer`, Node, FFmpeg, and yt-dlp individually;
2. sign the outer `Cobalt.app`;
3. verify with `codesign --verify --deep --strict`;
4. notarize and staple;
5. verify with `spctl --assess`.

**Step 6: Build**

Run:

```bash
pnpm release:build
```

Expected: `Cobalt.app` contains an executable `Resources/binaries/res-sniffer`.

**Step 7: Commit**

```bash
git add package.json scripts src-tauri/tauri.conf.json RELEASE.md .github/workflows/release.yml
git commit -m "build: package resource sniffer sidecar"
```

### Task 12: End-to-end verification and staged rollout

**Files:**
- Create: `sidecars/res-sniffer/internal/integration/proxy_test.go`
- Create: `docs/sniffer.md`
- Create: `docs/sniffer-recovery.md`
- Modify: `README.md`
- Modify: `docs/index.html`

**Step 1: Add local integration tests**

Spin up local HTTP/HTTPS fixture servers and route a test client through the sidecar. Verify direct MP4, audio, HLS manifest, duplicate suppression, upstream proxy chaining, and private-header redaction.

**Step 2: Test proxy restoration**

On a disposable macOS test account:

- start with no proxy;
- start with Clash at `127.0.0.1:7897`;
- use Wi-Fi and Ethernet service names;
- force-kill sidecar;
- force-kill Cobalt;
- reboot/reopen with a stale recovery file.

Expected: previous network configuration can always be restored.

**Step 3: Test certificate lifecycle**

Verify install, trust, fingerprint display, removal, CA regeneration, and rejection of a mismatched private key.

**Step 4: Run platform matrix**

Record actual results instead of assuming support:

| Platform | Capture | Download | Decrypt/Merge | Upstream proxy |
| --- | --- | --- | --- | --- |
| WeChat Channels | Required | Required | Required | Required |
| WeChat Mini Program | Required | Required | If applicable | Required |
| Douyin | Required | Required | If applicable | Required |
| Kuaishou | Required | Required | If applicable | Required |
| Xiaohongshu | Required | Required | If applicable | Required |
| Kugou | Required | Required | If applicable | Required |
| QQ Music | Required | Required | If applicable | Required |

Only change a row to supported after all required cells pass.

**Step 5: Document recovery**

`docs/sniffer-recovery.md` must show how to stop sniffing, restore settings inside Cobalt, manually inspect macOS proxies, and remove the generated certificate.

**Step 6: Run the full release gate**

Run:

```bash
pnpm release:check
pnpm release:build
codesign --verify --deep --strict src-tauri/target/release/bundle/macos/Cobalt.app
```

Expected: all automated checks pass and the signed app launches on a clean Mac.

**Step 7: Release as preview**

Ship behind a **Resource Sniffer (Preview)** label for one release. Collect failure codes locally without collecting URLs, headers, cookies, or media metadata.

**Step 8: Commit**

```bash
git add sidecars/res-sniffer/internal/integration docs README.md
git commit -m "docs: add resource sniffer verification and recovery guide"
```

## Recommended Delivery Order

1. **Technical spike:** Tasks 1-4. Prove generic HTTPS capture through a local fixture.
2. **WeChat MVP:** Tasks 5-8. Prove one real WeChat Channels download and decryption.
3. **Product UI:** Tasks 9-10. Add onboarding, captures, proxy controls, and translations.
4. **Release hardening:** Tasks 11-12. Sign, notarize, test restoration, then ship as Preview.

## Acceptance Criteria

- Existing URL downloads behave exactly as before when Sniffer mode is unused.
- Sniffer never starts without explicit user action.
- No fixed CA private key exists in source or release artifacts.
- Svelte never receives raw request headers, cookies, signed media URLs, decode keys, or proxy credentials.
- Existing Clash/system proxy settings are restored exactly after Stop and after recoverable failures.
- An upstream HTTP proxy can be chained without creating a loop through `127.0.0.1:8899`.
- One WeChat Channels video can be captured, downloaded, decrypted, opened, and shown with stable progress.
- Direct MP4/audio and HLS downloads work from local integration fixtures.
- Developer ID signing and notarization include all nested executables.
- Documentation explains certificate trust, network impact, recovery, attribution, and lawful-use expectations.

## Main Risks

- WeChat JS signatures can change and break the injection plugin. Keep it isolated, fixture-tested, and versioned.
- Some apps use certificate pinning and cannot be intercepted by a user CA.
- A crash while the system proxy is active can disrupt networking. Exact snapshots and startup recovery are release blockers.
- Signed URLs and cookies are sensitive. They must remain process-local and ephemeral.
- Go/Node/FFmpeg nested binaries complicate Developer ID signing and notarization.
- “Supported platform” claims can decay quickly. Maintain a release-by-release test matrix.

