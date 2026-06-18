# Cobalt Desktop (Tauri v2 + Svelte 5)

[English](#english) | [中文说明](#中文说明)

---

## 中文说明

Cobalt Desktop 是一款基于 **Tauri v2** 和 **Svelte 5** 构建的轻量级、高颜值的媒体下载器（类似于 Downie）。当前版本采用 **本地 yt-dlp + FFmpeg 优先**、**远程媒体服务辅助** 的混合架构，重点优化 YouTube、Bilibili 等容易受登录态和防下载机制影响的平台。

本项目通过将应用解耦为编译型的 **Rust 原生核心**（处理系统交互、并发下载、剪贴板监控等）和 **Svelte 5**（处理玻璃拟态响应式暗色 UI 界面），彻底解决了 Electron 原本在现代操作系统中的诸多缺陷与内存瓶颈。

### 🏗️ 架构设计

在原版 Electron 架构中，前端 UI 和 Cobalt API 在同一个进程环境中运行，这在 macOS 26（使用 64KB 内存页的 ARM64 设备）下经常由于 V8 指针压缩限制导致虚拟机虚拟内存分配崩溃，同时还会引入复杂的 C++ 原生二进制签名/公证问题。

Tauri 重构版采取了**解耦进程架构**：
1. **Svelte 5 界面**：运行在系统轻量级内置 Webview（macOS 上为 WebKit）中，不拥有任何直接文件系统读写权限，仅通过 Tauri 安全 IPC 管道与后端通信。
2. **Rust 后端核心**：独立编译成原生二进制，内存开销极低。负责管理多线程下载队列、代理环境注入、本地工具进程与任务生命周期。
3. **本地 yt-dlp / FFmpeg 工具链**：YouTube 与 Bilibili 默认走本地下载路径，读取本机浏览器 Cookie，并使用内置 FFmpeg 合并 DASH 音视频或抽取音频。
4. **远程 Cobalt 媒体服务**：对 Instagram、X、Pinterest 等更适合服务端解析的平台，应用会连接配置的远程服务获取媒体或代理下载流。

### ✨ 已完成功能

- **🚀 突破 macOS ARM64 崩溃**：通过 Tauri (WKWebView) 构建，彻底避开 Electron 在 macOS ARM64 (64KB page) 下的 V8 CodeRange 虚拟内存崩溃限制。
- **📦 零原生 C++ 模块**：移除了 `isolated-vm` 等 C++ node 原生绑定，完全消除 ABI 不匹配与编译冲突问题。
- **🔄 后台进程生命周期管理**：Rust 后端负责本地工具与辅助服务进程的启动、监控和清理，避免孤儿后台进程占用资源或端口。
- **⚡ 多线程并发下载队列**：使用 Rust 原生多线程调度下载任务，可自由配置最大并行任务数，并向前端实时发送下载进度、实时网速和剩余时间 (ETA)。
- **📋 智能剪贴板监听**：通过 `arboard` 库在 Rust 开启后台轻量线程监控系统剪贴板。当复制包含视频或音频的 URL 链接时，应用内部会平滑弹出精美 Toast 提示，支持一键加入下载队列。
- **🎯 YouTube 本地优先下载**：内置 `yt-dlp` 与 Node JS runtime，优先在本机解析和下载 YouTube，支持浏览器 Cookie 登录态、实时进度、速度和 ETA。
- **📺 Bilibili 本地 Cookie 下载**：Bilibili / b23.tv 默认走本地 `yt-dlp`，优先尝试 `chrome:Default` 及其他 Chrome profile，减少线上 Cookie 过期带来的失败。
- **🎧 真正的音频提取**：音频模式使用 `yt-dlp --extract-audio` 与内置 FFmpeg，输出真正的 m4a/mp3/ogg/wav/opus 音频文件，而不是仅改扩展名。
- **🧩 DASH 进度聚合**：针对 Bilibili 的视频流 + 音频流 + 合并流程做阶段进度映射，避免进度条回退或跳动。
- **🌐 远程服务辅助解析**：对 Instagram、X、Pinterest、Dailymotion、Twitch 等平台保留服务端辅助下载路径，降低本地解析复杂度。
- **🖧 完整 HTTP 代理支持**：支持注入 Clash / Clash Verge 等本地代理服务，所有请求及下载分片均经过代理转发，确保顺畅拉取外网资源。
- **🌐 动态多语言支持**：支持 英文 (English)、中文 (简体中文) 和 俄文 (Русский)，采用 Svelte 5 的 Rune 状态机驱动，实现一键无缝切换语言。
- **🎨 深色高级 UI/UX**：基于 Svelte 5 新特性 Runes 构建暗色工作台界面，包含紧凑任务卡、下载上下文提示、平台能力标签与拖拽检测。

### ⚙️ 构建与开发说明

详见 [快速上手](#3-快速上手)。

---

## English

Cobalt Desktop is a lightweight, premium Downie-like media downloader built with **Tauri v2** and **Svelte 5**. The current architecture uses a **local yt-dlp + FFmpeg first** pipeline for sensitive platforms like YouTube and Bilibili, plus a configurable remote media service for server-assisted platforms.

This application decouples the front-end layout from the scrapers by running a compiled **native Rust core** for system integrations (downloads, queuing, clipboard monitor, process control) and a sandboxed **Svelte 5** Single Page App (SPA) for a responsive, glassmorphic dark-mode UI.

### 🏗️ Decoupled Architecture

In the original Electron app, the frontend UI and the Node.js scraper server ran inside the same process space, causing severe V8 pointer compression virtual memory reservation crashes on macOS ARM64 (with 64KB pages) and complicated packaging/codesigning issues.

The Tauri rebuild decouples these components:
1. **Svelte 5 UI**: Rendered inside WebKit (macOS). Safe and sandboxed, communicating with Rust via IPC command invokes and event emitters.
2. **Rust Backend Core (`lib.rs`)**: A compiled binary that manages settings, folder dialogs, download tasks, local tool processes, and task lifecycle.
3. **Local yt-dlp / FFmpeg toolchain**: YouTube and Bilibili are downloaded locally, with browser-cookie support and bundled FFmpeg for DASH merging and real audio extraction.
4. **Remote media service**: Server-assisted parsing remains available for platforms where resolver-side proxying is more reliable.

### ✨ Completed Features

- **🚀 macOS ARM64 Native Compatibility**: Bypasses all Electron virtual memory crashes on 64KB page systems.
- **📦 Zero Local C++ Native Modules**: No C++ node native bindings (`isolated-vm` removed), preventing ABI mismatch compilation errors.
- **🔄 Auto Process Lifecycle Control**: Rust starts, monitors, and cleans up local tools and helper services so background processes do not linger.
- **⚡ Concurrent Download Queue**: Multi-threaded scheduler managing downloads up to your preferred concurrent limit, streaming bytes to a local file, and piping live speeds/progress back to Svelte.
- **📋 Smart Clipboard Monitoring**: A background thread detects copied video/audio URLs and triggers a reactive in-app toast to quickly queue downloads.
- **🎯 Local YouTube first**: Bundled `yt-dlp` and Node runtime handle YouTube locally, with browser-cookie fallback, live progress, speed, and ETA.
- **📺 Local Bilibili first**: Bilibili and b23.tv use local `yt-dlp`, explicitly trying `chrome:Default` and other Chrome profiles before falling back.
- **🎧 Real audio extraction**: Audio-only mode uses `yt-dlp --extract-audio` and bundled FFmpeg, producing real m4a/mp3/ogg/wav/opus files.
- **🧩 DASH-aware progress**: Bilibili's separate video/audio streams and merge phase are mapped into stable progress so the UI does not jump backwards.
- **🌐 Server-assisted fallback**: Remote media service integration remains available for Instagram, X, Pinterest, Dailymotion, Twitch, and similar sites.
- **🖧 Full HTTP Proxy Support**: Seamlessly routes all scrapers and stream downloads through local Clash / Verge proxies.
- **🌐 Dynamic Multi-language**: Translated into English, Chinese (中文), and Russian (Русский).
- **🎨 Compact dark-mode UI**: Built with Svelte 5 runes (`$state`, `$derived`), with task cards, context pills, platform capability labels, and link drag-and-drop detection.

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

This repository includes `pnpm-workspace.yaml` allow-build rules for `esbuild`, `ffmpeg-static`, and `syscall-napi`. If pnpm asks to approve builds after upgrading pnpm, run:
```bash
pnpm approve-builds esbuild ffmpeg-static syscall-napi
```

### 2. Run in Development Mode
Launch the application window with hot-reloading:
```bash
pnpm tauri dev
```
*This starts the SvelteKit dev server, compiles the Rust backend, prepares local helper processes, and opens the window.*

### 3. Build Production Executable (Local)
To package the app locally on your machine:

1. Build the Tauri app:
   ```bash
   pnpm release:build
   ```
`release:build` automatically prepares the bundled API, Node runtime, FFmpeg runtime, runs validation, and then builds the app.

The output executable will be compiled and saved to:
`src-tauri/target/release/bundle/macos/Cobalt.app` (or `.dmg`).

---

## 🤖 CI/CD & Static Landing Page

### 🐙 GitHub Actions (Auto-Release CI)
This repository includes a preconfigured GitHub Actions workflow at [.github/workflows/release.yml](.github/workflows/release.yml).
- **Trigger**: Pushing a tag starting with `v` (e.g., `v0.1.0`) triggers the release runner.
- **Job**: The current workflow creates a GitHub Draft Release for tagged versions. Local signed builds are still produced from your macOS machine with `pnpm release:build`.

### 🌐 GitHub Pages (Project Landing Page)
A beautiful, glassmorphic landing page introducing the project is configured in the [docs/](docs/) folder.
- **How to host on GitHub Pages**:
  1. Go to your repository settings on GitHub.
  2. Under the **Pages** tab, change **Source** to `Deploy from a branch`.
  3. Select your main/master branch, and set the folder to `/docs`.
  4. Save. Your landing page will be live at `https://<your-username>.github.io/<your-repo-name>/`.
