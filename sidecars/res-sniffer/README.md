# Cobalt Resource Sniffer

This is Cobalt's headless, local-only resource capture sidecar. It listens only on
`127.0.0.1`, accepts JSONL commands from the desktop app, and applies HTTPS MITM
only to the explicit domain allowlist in `main.go`. It must never be changed to a
wildcard capture proxy.

The production bundle builds `cmd/res-sniffer` for macOS Apple Silicon and embeds
the resulting executable as `src-tauri/binaries/res-sniffer`.
