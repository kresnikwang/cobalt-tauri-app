# Send to Cobalt — Browser Extension

One-click "send this page/link/video to Cobalt Desktop" for Chrome, Edge, Brave
and Firefox. It hands the URL to the app through the `cobalt://download?url=…`
deep link the app already registers — **no app changes required**.

## Features

- **Toolbar button** → popup shows the active tab URL and a "Download with Cobalt" button.
- **Right-click menu** → "Send this page / link / video to Cobalt".

## Install (unpacked / development)

### Chrome / Edge / Brave

1. Go to `chrome://extensions` (or `edge://extensions`).
2. Enable **Developer mode** (top right).
3. Click **Load unpacked** and select this `browser-extension/` folder.
4. Pin the extension, then click it on any video page → **Download with Cobalt**.

### Firefox (121+)

1. Go to `about:debugging#/runtime/this-firefox`.
2. Click **Load Temporary Add-on…** and select `manifest.json` in this folder.
3. (Temporary until you restart; for a permanent install the extension must be signed by Mozilla.)

## Requirements

- Cobalt Desktop must be **installed and running** (it owns the `cobalt://` scheme).
- First time on a page, macOS may ask to allow opening the `cobalt` link — approve it.

## How it works

The extension never parses media itself. It just forwards the URL:

```
cobalt://download?url=<url-encoded page/link/video URL>
```

The app's deep-link handler (`register_deep_link` in `src-tauri/src/lib.rs`)
receives it and enqueues the download exactly like a paste would.
