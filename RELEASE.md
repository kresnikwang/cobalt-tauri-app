# Release Checklist

## Local Validation

```bash
pnpm install
pnpm release:check
pnpm prepare:release-bundle
```

If pnpm reports ignored build scripts for `esbuild`, `ffmpeg-static`, or `syscall-napi`, approve the required build scripts once:

```bash
pnpm approve-builds esbuild ffmpeg-static syscall-napi
```

The repository also stores these allow rules in `pnpm-workspace.yaml`. The release bundle script writes the same rules into the temporary bundled API directory before installing production dependencies.

Verify the bundled API can start without the workspace:

```bash
API_URL=http://127.0.0.1:47319 \
API_PORT=47319 \
API_LISTEN_ADDRESS=127.0.0.1 \
YOUTUBE_ALLOW_BETTER_AUDIO=1 \
FORCE_LOCAL_PROCESSING=never \
ENABLE_DEPRECATED_YOUTUBE_HLS=always \
DURATION_LIMIT=86400 \
src-tauri/binaries/node src-tauri/bundled-api/src/cobalt.js
```

Open `http://127.0.0.1:47319/` and confirm it returns Cobalt server info.

## Build

```bash
pnpm release:build
```

`release:build` runs:

```bash
pnpm prepare:release-bundle
pnpm release:check
pnpm tauri build --bundles app
```

The preparation step copies:

- `src-tauri/binaries/node`
- `src-tauri/binaries/ffmpeg`
- `src-tauri/bundled-api`

Keep `src-tauri/binaries/yt-dlp` available as well. It is used for local YouTube and Bilibili downloads. For packaged builds, avoid re-signing `yt-dlp` itself with Developer ID; ad-hoc signing the final `.app` bundle is fine for local testing.

The app bundle is written to:

```text
src-tauri/target/release/bundle/macos/Cobalt.app
```

If DMG creation fails, the `.app` bundle is still usable. Create a zip:

```bash
mkdir -p src-tauri/target/release/bundle/zip
codesign --force --deep --sign - src-tauri/target/release/bundle/macos/Cobalt.app
ditto -c -k --sequesterRsrc --keepParent \
  src-tauri/target/release/bundle/macos/Cobalt.app \
  src-tauri/target/release/bundle/zip/Cobalt_0.1.0_aarch64.app.zip
```

## Public macOS Distribution

For public distribution outside your own Mac, use Apple Developer ID signing and notarization. Ad-hoc signing is useful for local testing, but Gatekeeper will still reject it on other machines.

Required Apple items:

- Apple Developer Program membership
- Developer ID Application certificate
- App Store Connect API key, or notarytool keychain profile

Typical environment variables for Tauri signing:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APPLE_ID="you@example.com"
export APPLE_PASSWORD="app-specific-password"
export APPLE_TEAM_ID="TEAMID"
```

Then rebuild:

```bash
pnpm tauri build
```

After signing/notarization, verify:

```bash
codesign --verify --deep --strict --verbose=2 src-tauri/target/release/bundle/macos/Cobalt.app
spctl --assess --type execute --verbose=4 src-tauri/target/release/bundle/macos/Cobalt.app
```
