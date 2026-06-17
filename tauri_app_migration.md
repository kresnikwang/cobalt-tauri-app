# Cobalt Media Downloader: Electron to Tauri Migration Details

This document outlines the architectural details and code structure of the Tauri rebuild for the Cobalt media downloader.

---

## 🏗️ Rebuilt Architecture

In the original Electron app, the frontend UI and the Cobalt API server ran within the same process environment, causing severe V8 pointer compression crashes under macOS 26 (with 64KB pages) and binary packaging/notarization complexities.

The Tauri rebuild decouples these components into a native compiled Rust core and a sandboxed system Webview (WebKit on macOS):

```
┌────────────────────────────────────────────────────────┐
│                      Tauri App                         │
│                                                        │
│   ┌───────────────────┐            ┌───────────────┐   │
│   │   Svelte 5 UI     │ <--IPC---> │ Rust Backend  │   │
│   │ (macOS WebKit)    │            │  (lib.rs)     │   │
│   └───────────────────┘            └───────┬───────┘   │
└────────────────────────────────────────────┼───────────┘
                                             │
                       Spawns Child Process  │ (Standard I/O)
                                             ▼
                                     ┌───────────────┐
                                     │ Local Node.js │
                                     │ Cobalt API    │ (Port 47301)
                                     └───────────────┘
```

### 1. The Core Rust Process (`lib.rs`)
The Tauri main process is written in Rust, which is completely independent of the browser engine's memory constraints.
* **Cobalt API Spawns Natively**: On startup, Rust spawns `node api/src/cobalt.js` (relative to the executable or dev directory) as a child process. Rust monitors its stdin/stdout, overrides proxy env variables, and guarantees that the server process is killed (`std::process::Child::kill`) when the Tauri window is closed or crashed.
* **Native Downloads**: Rust handles the entire downloading stream (sending the POST request to port 47301, reading redirects/picker response, sending GET requests to YouTube CDNs, streaming bytes to a local file stream). Progress and speeds are computed in Rust and emitted to Svelte.
* **System Integrations**: Select folder dialogue (`rfd` crate), clipboard URLs monitoring (`arboard` crate), opening files, and revealing files in finder are executed natively in Rust. Svelte needs no filesystem permissions.

### 2. The Svelte 5 Frontend
* Renders using Svelte 5 runes (`$state`, `$derived`).
* Uses Tauri's core `@tauri-apps/api` to invoke Rust commands and register event listeners.
* Packaged as a static single page application (SPA) using `@sveltejs/adapter-static` with fallback `index.html`.

---

## 📂 File Layout

Here is how the ported files are structured inside this folder:

```
tauri-app/
├── package.json               # SvelteKit and Tauri CLI node dependencies
├── svelte.config.js           # Static adapter SPA setup
├── vite.config.js             # Vite development server settings
├── src/
│   ├── app.html               # Main entry HTML
│   ├── app.css                # Global glassmorphic dark-theme styles
│   ├── routes/
│   │   ├── +layout.svelte     # Applies app.css wrapper layout
│   │   ├── +layout.ts         # Disable SSR (SSR = false for SPA mode)
│   │   └── +page.svelte       # App UI, tabs navigation, task list
│   └── lib/
│       ├── i18n.svelte.ts     # Svelte 5 dynamic translation store
│       └── i18n/              # Language translations (en, zh, ru)
└── src-tauri/
    ├── Cargo.toml             # Rust dependencies (reqwest, tokio, rfd, arboard)
    ├── tauri.conf.json        # Window sizes, transparency, app credentials
    ├── build.rs               # Tauri app compiler script
    └── src/
        ├── main.rs            # Rust entry point
        └── lib.rs             # Task queue, downloader, child server controller
```

---

## 🔗 IPC Command & Event Bindings

The communication interface between Svelte and Rust is mapped as follows:

| IPC Type | Svelte Trigger | Rust Command / Event | Action |
| :--- | :--- | :--- | :--- |
| **Invoke** | `invoke('get_settings')` | `get_settings()` | Returns user save path, proxy configuration, etc. |
| **Invoke** | `invoke('save_settings')` | `save_settings()` | Writes settings to `settings.json` in AppData. |
| **Invoke** | `invoke('download_url')` | `download_url()` | Places URL into Rust's scheduler queue. |
| **Invoke** | `invoke('cancel_task')` | `cancel_task()` | Signals task abort receiver & deletes partial file. |
| **Invoke** | `invoke('select_directory')` | `select_directory()` | Opens native RFD folder picker. |
| **Invoke** | `invoke('reveal_in_finder')` | `reveal_in_finder()` | Opens macOS finder or Windows explorer. |
| **Invoke** | `invoke('restart_app')` | `restart_app()` | Restarts Tauri window and kills/respawns child process. |
| **Event** | `listen('task-updated')` | `app_handle.emit(...)` | Streams live speed, downloaded bytes, and status. |
| **Event** | `listen('clipboard-detected')`| `app_handle.emit(...)` | Background clipboard loop detects copied HTTP link. |
