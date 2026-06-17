# Cobalt Desktop (Tauri v2 + Svelte 5)

[English](#english) | [中文说明](#中文说明)

---

## 中文说明

Cobalt Desktop 是一款基于 **Tauri v2** 和 **Svelte 5** 构建的轻量级、高颜值的媒体下载器（类似于 Downie），底层由 **Cobalt Core** 强力驱动。

本项目通过将应用解耦为编译型的 **Rust 原生核心**（处理系统交互、并发下载、剪贴板监控等）和 **Svelte 5**（处理玻璃拟态响应式暗色 UI 界面），彻底解决了 Electron 原本在现代操作系统中的诸多缺陷与内存瓶颈。

### 🏗️ 架构设计

在原版 Electron 架构中，前端 UI 和 Cobalt API 在同一个进程环境中运行，这在 macOS 26（使用 64KB 内存页的 ARM64 设备）下经常由于 V8 指针压缩限制导致虚拟机虚拟内存分配崩溃，同时还会引入复杂的 C++ 原生二进制签名/公证问题。

Tauri 重构版采取了**解耦进程架构**：
1. **Svelte 5 界面**：运行在系统轻量级内置 Webview（macOS 上为 WebKit）中，不拥有任何直接文件系统读写权限，仅通过 Tauri 安全 IPC 管道与后端通信。
2. **Rust 后端核心**：独立编译成原生二进制，内存开销极低。负责管理多线程下载队列、代理环境注入，并作为父进程启动与监视本地 API 进程的生命周期。
3. **Cobalt Node.js API**：应用启动时，Rust 原生进程会自动在后台拉起 `api/src/cobalt.js`（占用端口 `47301`），并在 Tauri 窗口关闭时自动将其销毁（Kill），实现对用户透明的零配置运行。

### ✨ 已完成功能

- **🚀 突破 macOS ARM64 崩溃**：通过 Tauri (WKWebView) 构建，彻底避开 Electron 在 macOS ARM64 (64KB page) 下的 V8 CodeRange 虚拟内存崩溃限制。
- **📦 零原生 C++ 模块**：移除了 `isolated-vm` 等 C++ node 原生绑定，完全消除 ABI 不匹配与编译冲突问题。
- **🔄 后台服务自生命周期管理**：Rust 后端在应用启动时自动检测并拉起 Node.js 本地 Cobalt 服务，在应用退出或异常崩溃时，通过 `Drop` 机制强行清理子进程，杜绝孤儿后台进程占用端口。
- **⚡ 多线程并发下载队列**：使用 Rust 原生多线程调度下载任务，可自由配置最大并行任务数，并向前端实时发送下载进度、实时网速和剩余时间 (ETA)。
- **📋 智能剪贴板监听**：通过 `arboard` 库在 Rust 开启后台轻量线程监控系统剪贴板。当复制包含视频或音频的 URL 链接时，应用内部会平滑弹出精美 Toast 提示，支持一键加入下载队列。
- **🎛️ 音视频无损重封装 (Remuxer)**：内置独立的 Remux 实用工具页面。用户可直接拖入本地视频或音频文件，通过 FFmpeg 将其快速打包转换为 MP4、WebM、MKV、MP3 等容器格式，**无需重新编码，零画质损失，毫秒级导出**。
- **🎯 智能重试机制**：如果 YouTube 对标准 HLS 流有限制，下载器会自动回退到备用 InnerTube 客户端（如无 HLS 封装的 `WEB_EMBEDDED` 视频流），显著提升下载成功率。
- **🖧 完整 HTTP 代理支持**：支持注入 Clash / Clash Verge 等本地代理服务，所有请求及下载分片均经过代理转发，确保顺畅拉取外网资源。
- **🌐 动态多语言支持**：支持 英文 (English)、中文 (简体中文) 和 俄文 (Русский)，采用 Svelte 5 的 Rune 状态机驱动，实现一键无缝切换语言。
- **🎨 玻璃拟态高级 UI/UX**：基于 Svelte 5 新特性 Runes 构建，全屏玻璃拟态（Glassmorphism）暗黑主题设计，并拥有流畅的微交互与拖拽检测。

### ⚙️ 构建与开发说明

详见 [快速上手](#3-快速上手)。

---

## English

Cobalt Desktop is a lightweight, premium Downie-like media downloader built with **Tauri v2** and **Svelte 5**, powered by the self-hosted **Cobalt Core** API. 

This application decouples the front-end layout from the scrapers by running a compiled **native Rust core** for system integrations (downloads, queuing, clipboard monitor, process control) and a sandboxed **Svelte 5** Single Page App (SPA) for a responsive, glassmorphic dark-mode UI.

### 🏗️ Decoupled Architecture

In the original Electron app, the frontend UI and the Node.js scraper server ran inside the same process space, causing severe V8 pointer compression virtual memory reservation crashes on macOS ARM64 (with 64KB pages) and complicated packaging/codesigning issues.

The Tauri rebuild decouples these components:
1. **Svelte 5 UI**: Rendered inside WebKit (macOS). Safe and sandboxed, communicating with Rust via IPC command invokes and event emitters.
2. **Rust Backend Core (`lib.rs`)**: A compiled binary that manages settings, folder dialogs, download tasks, streams files directly to disk, and controls the Node subprocess.
3. **Local Node.js Cobalt API**: On startup, Rust spawns `node api/src/cobalt.js` natively as a child process on port `47301` and monitors its life cycle. It forces a `.kill()` when the window is closed, ensuring no orphaned processes are left behind.

### ✨ Completed Features

- **🚀 macOS ARM64 Native Compatibility**: Bypasses all Electron virtual memory crashes on 64KB page systems.
- **📦 Zero Local C++ Native Modules**: No C++ node native bindings (`isolated-vm` removed), preventing ABI mismatch compilation errors.
- **🔄 Auto Process Lifecycle Control**: Rust spawns, proxy-injects, and destroys the local Node API server automatically on startup and shutdown.
- **⚡ Concurrent Download Queue**: Multi-threaded scheduler managing downloads up to your preferred concurrent limit, streaming bytes to a local file, and piping live speeds/progress back to Svelte.
- **📋 Smart Clipboard Monitoring**: A background thread detects copied video/audio URLs and triggers a reactive in-app toast to quickly queue downloads.
- **🎛️ FFmpeg Remuxer Tool**: Drag and drop media files to remux them into different containers (e.g. merging audio/video or changing extension like `.webm` to `.mp4`) without re-encoding — completed in milliseconds with zero quality loss.
- **🎯 Intelligent Client Fallbacks**: Automatically falls back to alternative InnerTube clients (e.g., `WEB_EMBEDDED` without HLS) if YouTube restricts HLS streams.
- **🖧 Full HTTP Proxy Support**: Seamlessly routes all scrapers and stream downloads through local Clash / Verge proxies.
- **🌐 Dynamic Multi-language**: Translated into English, Chinese (中文), and Russian (Русский).
- **🎨 Glassmorphic Dark-Mode UI**: Built with Svelte 5 runes (`$state`, `$derived`), presenting a modern, premium look with fluent hover micro-animations and link drag-and-drop detection.

---

## 🛠️ Prerequisites

To run or build the application locally, you need:

1. **Rust & Cargo**:
   Install the Rust compiler toolchain:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **Node.js** (v18+) & **pnpm**:
   Install Node and pnpm:
   ```bash
   brew install node
   npm install -g pnpm
   ```

---

## 🚀 Getting Started

### 1. Install Dependencies
Run in the root monorepo directory:
```bash
pnpm install
```

### 2. Run in Development Mode
Launch the application window with hot-reloading:
```bash
pnpm tauri dev
```
*This starts the SvelteKit dev server, compiles the Rust backend, launches the Cobalt Node API on port `47301`, and opens the window.*

### 3. Build Production Executable (Local)
To package the app locally on your machine:

1. Prepare the self-contained API and Node runtime:
   ```bash
   pnpm prepare:release-bundle
   ```
2. Build the Tauri app:
   ```bash
   pnpm release:build
   ```
The output executable will be compiled and saved to:
`src-tauri/target/release/bundle/macos/Cobalt.app` (or `.dmg`).

---

## 🤖 CI/CD & Static Landing Page

### 🐙 GitHub Actions (Auto-Release CI)
This repository includes a preconfigured GitHub Actions workflow at [.github/workflows/release.yml](.github/workflows/release.yml).
- **Trigger**: Pushing a tag starting with `v` (e.g., `v0.1.0`) triggers the release runner.
- **Job**: The pipeline compiles the code on `macos-latest`, downloads `yt-dlp` dynamically, prepares the Node/API release bundles, compiles the Tauri production app, and publishes a **Draft Release** on GitHub containing the compiled binaries.

### 🌐 GitHub Pages (Project Landing Page)
A beautiful, glassmorphic landing page introducing the project is configured in the [docs/](docs/) folder.
- **How to host on GitHub Pages**:
  1. Go to your repository settings on GitHub.
  2. Under the **Pages** tab, change **Source** to `Deploy from a branch`.
  3. Select your main/master branch, and set the folder to `/docs`.
  4. Save. Your landing page will be live at `https://<your-username>.github.io/<your-repo-name>/`.
