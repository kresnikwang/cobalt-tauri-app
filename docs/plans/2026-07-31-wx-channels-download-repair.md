# WeChat Channels Download Repair Implementation Plan

> **For Codex:** Implement this plan in the existing dirty workspace and preserve all unrelated user changes.

**Goal:** Make Cobalt reliably discover and download a playing WeChat Channels video on macOS using the existing local capture sidecar.

**Architecture:** Keep Cobalt's per-install CA and allowlisted local MITM proxy. Serve a same-origin hook from `channels.weixin.qq.com`, report WeChat API results back through a same-origin local endpoint, retain signed URLs and decode keys only in the sidecar, then download and decrypt the 128 KiB encrypted prefix in Rust with the verified ISAAC64 algorithm.

**Tech Stack:** Go 1.22+ sidecar with `goproxy`, Tauri v2/Rust, Svelte 5, macOS Keychain and `networksetup`.

---

### Task 1: Repair metadata capture

**Files:**
- Modify: `sidecars/res-sniffer/cmd/res-sniffer/main.go`
- Modify: `sidecars/res-sniffer/cmd/res-sniffer/main_test.go`

1. Replace the cross-origin inline callback with a same-origin external hook.
2. Patch the known WeChat bridge methods to report their returned payloads.
3. Read callback bodies synchronously before the proxy request is released.
4. Parse string and numeric forms of `decodeKey`, `fileSize`, and `mediaType`.
5. Keep encrypted `finder.video.qq.com` streams out of the generic capture list.
6. Add HTML, JavaScript, nested payload, and HTTPS proxy tests.

### Task 2: Repair WeChat prefix decryption

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/sniffer.rs`

1. Port the verified ISAAC64 initialization/mix operations using wrapping arithmetic.
2. XOR the 128 KiB prefix using big-endian keystream bytes.
3. Pass the private decode key from sidecar to Rust without exposing it to Svelte.
4. Add a fixed cross-language test vector and a round-trip file test.

### Task 3: Tighten setup and diagnostics

**Files:**
- Modify: `src-tauri/src/sniffer.rs`
- Modify: `src/lib/i18n/en.json`
- Modify: `src/lib/i18n/ru.json`
- Modify: `src/lib/i18n/zh.json`
- Modify: `src/routes/+page.svelte`
- Modify: `docs/sniffer.md`
- Modify: `THIRD_PARTY_NOTICES.md`
- Create: `third_party/wx_channels_download/LICENSE`
- Create: `third_party/wx_channels_download/UPSTREAM.md`

1. Verify the generated certificate by fingerprint instead of file existence.
2. Require verified trust before enabling the system proxy.
3. Explain that WeChat must be reopened after the proxy is enabled.
4. Add source and license attribution for the implementation research.

### Task 4: Verify the release path

1. Run `go test ./...` for the sidecar.
2. Run `cargo test --manifest-path src-tauri/Cargo.toml`.
3. Run `pnpm check`.
4. Run the release preparation and Tauri build path.
5. Perform a real logged-in WeChat playback test; confirm one metadata capture, a completed download, and a playable MP4.
