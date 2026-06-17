# Cobalt Desktop (Tauri + Svelte 5)

A lightweight, premium Downie-like media downloader built with **Tauri v2** and **Svelte 5**, powered by Cobalt Core.

This application is decoupled into a compiled Rust core for system integrations (downloads, queuing, clipboard monitor) and Svelte 5 for a responsive, glassmorphic dark-mode UI.

---

## ✨ Features

- **macOS 26 Compatible**: Built with Tauri (WKWebView) to completely bypass Electron's V8 CodeRange virtual memory reservation crashes on macOS ARM64 with 64KB pages.
- **Zero Local C++ Native Modules**: No C++ node native bindings (`isolated-vm` is removed), eliminating ABI mismatch issues.
- **Concurrent Download Queue**: Multi-threaded scheduler managing downloads up to your preference limit.
- **Smart Retries**: Automatic fallback to alternative InnerTube clients (e.g. `WEB_EMBEDDED` without HLS) if YouTube restricts HLS streams.
- **Clipboard Monitoring**: Auto-detects media links on copy and slides up a quick download toast.
- **HTTP Proxy Support**: Integrated with Clash/Verge proxies for smooth scraping.
- **Multilingual**: Supports English, Chinese (中文), and Russian (Русский).

---

## 🛠️ Prerequisites

To run or build the application, you need the following developer tools installed on your system:

1. **Rust & Cargo**:
   Install the Rust compiler toolchain:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
   *Restart your terminal or run `source "$HOME/.cargo/env"` after installation.*

2. **Node.js** (v18+ recommended) and **pnpm**:
   Install Node and pnpm:
   ```bash
   brew install node
   npm install -g pnpm
   ```

---

## 🚀 Getting Started

### 1. Install Node Dependencies
Run in the project directory:
```bash
pnpm install
```

### 2. Run in Development Mode
Launch the application window with live-reload support:
```bash
pnpm tauri dev
```
*This starts the SvelteKit development server, compiles the Rust backend, launches the Cobalt Node API on port 47301, and opens the app.*

### 3. Build Production Executable
Package the app into a signed macOS `.app` bundle:
```bash
pnpm tauri build
```
The output executable will be built and saved to:
`src-tauri/target/release/bundle/macos/Cobalt.app`

---

## 📝 Configuration Note (Important)

In development mode, this app expects the **Cobalt API server** (`api/` directory containing `api/src/cobalt.js`) to be present in the workspace parent folder (`../api/src/cobalt.js`).

If you are splitting this repository out permanently to a standalone directory:
1. Make sure to copy the `api/` package folder (along with its Node.js configuration) alongside this directory, or
2. Update the environment variables/URLs in [src-tauri/src/lib.rs](src-tauri/src/lib.rs) to connect directly to an online self-hosted Cobalt API server instead of spawning the server locally.
