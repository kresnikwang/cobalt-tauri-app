# Release Checklist

> Agent 请先读 [AGENTS.md](./AGENTS.md)（发版 playbook，含失败排查手册）。
> 人类快速发版看本节即可。

## Recommended: GitHub CI Release (green every time)

Push a version tag and CI builds, signs (ad-hoc), and publishes
`.dmg` + `.app.zip` automatically:

```bash
# 1. Align versions (updates package.json, tauri.conf.json, Cargo.toml AND Cargo.lock)
node scripts/sync-versions.mjs 1.0.10
# or: pnpm release:bump 1.0.10

# 2. Sanity check locally (must exit 0)
pnpm release:check

# 3. Commit, tag, push — CI does the rest.
#    NOTE: all FOUR version files are required (Cargo.lock included)!
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: release v1.0.10"
git tag v1.0.10
git push origin main v1.0.10
```

CI (`Release` workflow on `macos-latest`) runs in order:

1. version sync check — `package.json`, `src-tauri/tauri.conf.json`,
   `src-tauri/Cargo.toml` must all equal the tag
2. `pnpm check` (Svelte), `cargo check --locked` (Rust),
   `go vet` + `go test` (res-sniffer), `vitest run` (api)
3. `pnpm prepare:release-bundle` — copies Node/FFmpeg, downloads
   `yt-dlp` automatically on clean runners, builds `res-sniffer`
4. `pnpm tauri build --bundles app,dmg`
5. `node scripts/post-build-bundle.mjs` — ad-hoc signs on CI
   (Developer ID only when `APPLE_SIGNING_IDENTITY` is set)
6. uploads `Cobalt-<ver>-aarch64.app.zip`, `Cobalt-<ver>-aarch64.dmg`,
   `INSTALL.md` to the GitHub Release

Manual trigger without pushing a tag also works: Actions →
`Release` → `Run workflow` → optional `version` (e.g. `1.0.10`)
and `draft` flag. If `version` is given, CI syncs the three files
in the runner before building; the Release is then published under
that tag.

> CI builds are **ad-hoc signed, not notarized**. Users install with
> right-click → Open, or `xattr -dr com.apple.quarantine`.

Every push/PR to `main` also runs the lightweight `CI` workflow
(version sync + all checks/tests above, no packaging), so breakage
is caught before tagging.

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

`yt-dlp` is used for local YouTube, Bilibili, and Dailymotion downloads. The
post-build step signs its PyInstaller launcher with the dedicated
`src-tauri/entitlements/yt-dlp.plist`; without disabled library validation, the
launcher cannot load its embedded Python framework after Developer ID signing.

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
  src-tauri/target/release/bundle/zip/Cobalt_1.0.8_aarch64.app.zip
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
