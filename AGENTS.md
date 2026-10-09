# Agent Release Playbook（发版唯一正确流程）

> 给未来接手发版的 Agent：**只走这份文档**，不要自己发明流程。
> 已用 v1.0.10 端到端验证通过（CI 3分钟 + Release 9分钟全绿）。

## 1. 前提检查（必做）

```bash
git status --porcelain=v1 -b        # 工作区必须干净，否则先处理
node scripts/sync-versions.mjs --check   # 三处版本必须一致
gh auth status                       # 必须已登录，否则后续 push 失败
```

## 2. 标准发版（以 X.Y.Z 为例）

```bash
# Step 1: 对齐版本（自动更新 package.json / tauri.conf.json / Cargo.toml / Cargo.lock）
node scripts/sync-versions.mjs X.Y.Z

# Step 2: 本地全绿门禁（约 1 分钟，必须 EXIT=0）
pnpm release:check

# Step 3: 提交版本文件（四个文件缺一不可！）
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: release vX.Y.Z"

# Step 4: 打 tag 并推送（tag 必须带 v 前缀）
git tag vX.Y.Z
git push origin main vX.Y.Z
```

## 3. 盯 CI + Release（必做）

```bash
gh run list --limit 3    # 应看到 CI（main）和 Release（tag）两个 run 在跑
```

- `CI` 约 3-4 分钟，`Release` 约 9-10 分钟。
- 用 `gh run list --limit 2` 轮询（**不要用 `gh run watch`，它会占满 30s 工具超时**）。
- 两个都 `completed success` 才算发版成功。
- 验证产物：`gh release view vX.Y.Z` 应看到
  `Cobalt-X.Y.Z-aarch64.app.zip`、`Cobalt-X.Y.Z-aarch64.dmg`、`INSTALL.md`。

## 4. 失败排查手册（按历史故障排序）

| 现象 | 根因 | 修法 |
|---|---|---|
| `cargo check --locked` 报 `cannot update the lock file` | 升版后没提交 `Cargo.lock` | 跑一次 `cargo check --manifest-path src-tauri/Cargo.toml` 更新锁文件，补提交（新版 `sync-versions.mjs` 已自动做这步） |
| `resource path binaries/... doesn't exist` | CI 干净检出无 `binaries/`，`cargo check` 先于 `prepare` 跑了 | workflow 里 `pnpm prepare:release-bundle` 必须在 `cargo check` **之前**（已固定顺序，不要调换） |
| `Unable to build the resource sniffer ... --help` | `res-sniffer` 没有 `--help` flag | 已改用 stdio `status` 冒烟测试，不要改回 `--help` |
| `go test` 报 `missing LC_UUID` | Go <1.23 在新 macOS runner 上的 bug | workflow 的 Go 版本保持 `1.23`+，`go build` 不要加 `-ldflags "-s -w"` |
| `could not read Username for 'https://github.com'` | HTTPS remote 无认证 | 本仓库 `origin` 已切到 SSH（`git@github.com:...`），**不要改回 HTTPS** |
| `gh run watch` 工具超时 | watch 会阻塞超 30s | 改用 `gh run list --limit N` 轮询 |

## 5. 禁止事项

1. **不要**手动改三处版本号 —— 只用 `node scripts/sync-versions.mjs X.Y.Z`。
2. **不要**漏提交 `src-tauri/Cargo.lock` —— 发版提交固定是 4 个文件。
3. **不要**把 tag 打成不带 `v` 的（如 `1.0.10`）—— workflow 只认 `v*`。
4. **不要**改 tag 指向已发布的版本 —— 如需重发，先 `git tag -d` + `git push origin :refs/tags/vX.Y.Z` 删远端 tag，再重打。
5. **不要**在 CI workflow 里把 `cargo check` 移到 `prepare` 前面。
6. **不要**给 `go build` 加回 `-s -w`。
7. 发版期间**不要**用 `git commit -a` —— 会把本地 `binaries/` 等意外文件带进去（已被 gitignore，但仍需显式 `git add` 四个文件）。

## 6. 背景知识（读一遍即可）

- 双 workflow：`CI`（push/PR 门禁，不打包）+ `Release`（tag/手动触发，真机打包发布）。
- CI 产物是 **ad-hoc 签名、未公证**，用户首次启动需右键 → 打开。
- `src-tauri/binaries/` 被 gitignore，CI 靠 `prepare-release-bundle.mjs` 自动下载 `yt-dlp`、复制 Node/FFmpeg、编译 `res-sniffer`。
- `workflow_dispatch` 可手动发版（Actions → Release → Run workflow，填 `version`），但推荐走 tag。
