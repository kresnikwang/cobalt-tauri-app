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

The repository also stores these allow rules in `pnpm-workspace.yaml`.

Verify the bundled local runtimes exist:

```bash
test -x src-tauri/binaries/node
test -x src-tauri/binaries/ffmpeg
test -x src-tauri/binaries/yt-dlp
```

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

The preparation step copies or verifies:

- `src-tauri/binaries/node`
- `src-tauri/binaries/ffmpeg`
- `src-tauri/binaries/yt-dlp`

`yt-dlp` is used for local YouTube, Bilibili, and Dailymotion downloads. For packaged builds, avoid re-signing `yt-dlp` itself with Developer ID; ad-hoc signing the final `.app` bundle is fine for local testing.

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
