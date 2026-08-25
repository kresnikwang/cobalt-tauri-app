# Xinpianchang Link Download Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Download authorized Xinpianchang works from a pasted `xinpianchang.com/a…` link without enabling the resource sniffer.

**Architecture:** Route Xinpianchang URLs to a dedicated local resolver before the generic remote-service path. The resolver opens the supplied article in a hidden Tauri webview so the site sees a real browser context, extracts the signed progressive MP4 URL and safe metadata from the rendered page, closes the webview, then hands the short-lived URL to the existing streaming downloader with `Referer` and `Range` headers. The sniffer allowlist and system proxy are unchanged.

**Tech Stack:** Rust, Tauri v2 WebviewWindow, Tokio, reqwest, Svelte/TypeScript, Cargo unit tests.

---

### Task 1: Add resolver parsing tests

**Files:**
- Create: `src-tauri/src/xinpianchang.rs`

**Step 1: Write failing tests**

Add tests for exact Xinpianchang article URL matching, rejection of lookalike domains, callback parsing, safe filename generation, and rejected/error callbacks.

**Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml xinpianchang`

Expected: FAIL until the module and parsing helpers exist.

**Step 3: Implement the minimal pure helpers**

Implement `is_xinpianchang_url`, `article_id`, `parse_callback_url`, filename sanitization, and the `ResolvedMedia` value returned to the downloader.

**Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml xinpianchang`

Expected: PASS.

### Task 2: Resolve the signed media URL in a real webview

**Files:**
- Modify: `src-tauri/src/xinpianchang.rs`

**Step 1: Add the hidden-webview resolver**

Create a uniquely labelled, non-focused Tauri webview for the requested article. Keep it renderable off-screen because macOS pauses a truly hidden WKWebView, inject an origin-guarded script that waits for `#__NEXT_DATA__` and the page's `<video>` element, selects the progressive MP4 currently authorized by the page, and reports only URL/title/filesize/permission metadata through a blocked custom-scheme navigation.

**Step 2: Add lifecycle and failure handling**

Close the resolver window after success or timeout, reject unexpected hosts and callback schemes, and return actionable errors for missing media, permission failures, or a 20-second timeout.

**Step 3: Compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: PASS.

### Task 3: Connect resolver to the download queue

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Step 1: Write URL-routing assertions**

Assert that canonical and `www` Xinpianchang article links enter the dedicated local path while lookalike hosts do not.

**Step 2: Refactor the direct stream downloader**

Use a small generic direct-media descriptor so both captured resources and link-resolved Xinpianchang media share cancellation, progress, collision-safe output paths, and streaming behavior without coupling the new link mode to sniffer startup.

**Step 3: Add the Xinpianchang queue branch**

Resolve the signed URL, add `Referer: https://www.xinpianchang.com/` and `Range: bytes=0-`, then download immediately. Do not add Xinpianchang domains to the sniffer allowlist.

**Step 4: Run backend tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: PASS.

### Task 4: Add platform presentation and documentation

**Files:**
- Modify: `src/lib/services.ts`
- Modify: `README.md`

**Step 1: Add platform metadata**

Add Xinpianchang to the service list so queued and completed tasks are labelled correctly.

**Step 2: Document behavior**

Document pasted-link support, the hidden local browser resolution step, short-lived signed URLs, and the fact that resource-sniffer setup is not required.

**Step 3: Run frontend validation**

Run: `pnpm check`

Expected: PASS with zero errors.

### Task 5: Final verification

**Files:**
- Verify all modified files.

**Step 1: Run release checks**

Run: `pnpm release:check`

Expected: Svelte and Rust checks pass.

**Step 2: Review the diff**

Run: `git diff --check && git diff --stat`

Expected: no whitespace errors and only the planned files are changed.
