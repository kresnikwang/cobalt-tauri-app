# Vendored `api/` 上游跟踪基线

本目录 `api/` 是 **imputnet/cobalt** monorepo 中 `api/` 子目录的**快照（vendored copy）**，
不是 npm 依赖。本文档记录它对应的上游提交（pin）、本地相对上游的全部改动，以及每季度同步上游的流程。
同步脚本见仓库根的 [`scripts/sync-cobalt-api.mjs`](../scripts/sync-cobalt-api.mjs)。

## 1. Pin 元数据

| 项 | 值 |
|---|---|
| Upstream 仓库 | https://github.com/imputnet/cobalt |
| Upstream 子目录 | `api/`（本仓库 `api/` 对应 monorepo 的 `api/` 子目录，路径前缀一致） |
| 包名 / 版本 | `@imput/cobalt-api` **11.7.1**（见 `api/package.json`） |
| Pinned commit | `a636575b09de1fc55d9b8cd98cac88f5f2f16b42` |
| Pinned 提交说明 | `api/package: revert redis to 4.7.1 (#1536)` |
| Pinned 提交日期 | 2026-04-06（committer，+06:00） |
| 基线记录日期 | 2026-10-09 |
| License | **AGPL-3.0**（见 `api/LICENSE`，同步时不得覆盖/删除） |

> **上游不为 api 11.x 打 release tag**：截至基线记录日，`imputnet/cobalt` 的 tag 只有早期 `v0.x / 1.x`，
> api 的 11.x 版本号只体现在 `api/package.json`，且 `main` 在 2026-04-06 之后**版本号仍停留在 11.7.1**（代码持续演进）。
> 因此本基线**按 commit SHA pin，而不是 tag**。该 pin 即记录日 `upstream/main` 的 HEAD，
> 用「对所有触及 `api/` 的提交做 numstat 树比对、取差异最小者」确定（差异分随历史单调上升，最小为该提交）。

### 本地 upstream remote（不入库）

`upstream` 只读远端只写在本地 `.git/config`，**不要**提交、也不要把 `origin` 改回 HTTPS：

```bash
git remote add upstream git@github.com:imputnet/cobalt.git   # 已在本机配置；新机器需重加
git fetch upstream --tags
```

## 2. 本仓库如何使用 `api/`

- **桌面 App 不在本地起 api**：客户端默认连远程实例 `http://47.241.10.142/cobalt-api`
  （`src-tauri/src/lib.rs` 的 `DEFAULT_API_URL`）。`api/` 快照主要用于**部署远程服务**与保持可追溯。
- **远程部署**：运行在 `/opt/cobalt-api`，独立 systemd 单元，部署/迁移见
  `docs/plans/2026-09-08-cobalt-api-server-migration.md`。同步上游的首要收益是远程实例白嫖新站与修复。
- **发版依赖**：
  - `scripts/prepare-release-bundle.mjs` 的 `resolveFfmpegBinary()` 会从 `api/node_modules`（或根 `node_modules`）
    取 `ffmpeg-static`，所以 `api/` 必须能干净 `pnpm install`。
  - 根 `package.json` 的 `release:check` 会跑 `pnpm --dir api test:unit --run`（vitest，基线时 **58** 个测试）。
- `src-tauri/bundled-api/` 是历史遗留的另一份拷贝：已 gitignore、`tauri.conf.json` 也不再打包它，**只登记、不在同步提交里删**。

## 3. 本地相对上游的改动清单（= 以后每次合并的冲突清单）

> 下列分类基于对 pin 的 `git diff` 树比对。同步时 `git apply -3` 的冲突只会出现在 **C 类（改过的上游文件）**；
> A 类本地新增文件不受影响。**version-info 与 package.json 是每次同步必冲突、必手工保留的硬骨头（见 C-0）。**

### A. 本地新增文件（上游没有，合并时一般不动）

**A-1 为脱离 monorepo 而提交的构建产物（必须保留）**
- `api/version-info/index.js`、`api/version-info/index.d.ts`、`api/version-info/package.json`
  —— 上游用 pnpm workspace（`"@imput/version-info": "workspace:^"`）在构建期生成该包，**不入库**；
  我们只 vendor 了 `api/` 子目录、没有 workspace，故改为 `"file:./version-info"` 并提交一份构建产物，否则 `pnpm install` 无法解析。

**A-2 本地新增的 vitest 单测体系（必须保留，随 `release:check` 运行）**
- `api/vitest.config.ts`
- `api/src/processing/__tests__/url.test.js`
- `api/src/store/__tests__/memory-store.test.js`
- `api/src/security/__tests__/secrets.test.js`
- `api/src/misc/__tests__/crypto.test.js`（**含一个已知偶发用例**：AES-256-CBC 翻转 ciphertext 首字节，
  单块 padding 仍合法概率约 1/256，约 0.4% 概率误过；如要稳定可改为翻转末字节）
- `api/src/misc/__tests__/api-url.test.js`
- 配套测试接线改动：`api/src/misc/run-test.js`、`api/src/util/test.js`

**A-3 本地功能新增**
- `api/src/misc/api-url.js` —— `createAPIURL()`，在反向代理子路径 `/cobalt-api` 下正确拼接端点 URL（保留 base path）。

**A-4 本地调试残留（首次同步时删除，见 §5）**
- `api/src/util/debug-range.js`（硬编码 videoId 的临时调试脚本，无任何引用）
- `api/src/util/test_youtube_ua.js`（孤立 UA 测试草稿，无任何引用）

### B. 本地删除的上游文件

无。同步时不应出现「上游有、本地缺」的常规文件（`version-info/` 属 A-1 的 workspace 差异，不在此列）。

### C. 改过内容的上游文件（冲突高发区，合并后逐项核对）

**C-0 依赖清单 `api/package.json`（每次同步必冲突）**
- 新增脚本 `test:unit`（`vitest run`）、`test:watch`；新增 devDependency `vitest`。
- `@imput/version-info`：`workspace:^` → `file:./version-info`（配合 A-1）。
- 移除 `isolated-vm` 依赖（本地/服务器环境不引入该原生模块）。
- 合并上游后必须**恢复这三处本地改动**，否则 install / 单测会失败。

**C-1 远程「YouTube 走服务端 yt-dlp 隧道」定制（核心本地功能）**
- `api/src/core/api.js`（+161/−9，并被 chmod 755）：
  - YouTube 请求改返回 `tunnel`，指向本服务新增的 `GET /yt-dlp?url=...`；
  - `/yt-dlp` 端点用 `child_process.spawn` 拉起服务端 `yt-dlp`（路径取 `YTDLP_PATH` / `YTDLP_NODE_PATH` /
    `YTDLP_COOKIES` 环境变量），带服务器 cookies、`-f bestvideo*+bestaudio/best`、`-o -` 流式回传，
    并把 cookies 失效映射成 401、其它失败映射成 502；文件名按 YouTube id 生成。
- `api/src/misc/api-url.js`（见 A-3）为其生成子路径感知的 tunnel URL。

**C-2 流式 / 隧道（tunnel）健壮性与反代适配**
- `api/src/stream/internal.js`（+189/−51）：chunked/generic stream 处理重写。
- `api/src/core/itunnel.js`（+10/−3）、`api/src/stream/manage.js`（+14/−6）：
  tunnel URL 已考虑 Redis 写入异步（MemoryStore 同步 / RedisStore 返回 Promise 的差异）与往返时序。
- `api/src/stream/proxy.js`（+10/−7）、`api/src/stream/stream.js`、`api/src/stream/shared.js`：代理/流式细节。
- `api/src/stream/ffmpeg.js`（+21/−9）：支持 `FFMPEG_PATH` 覆盖；调 ffmpeg 前剔除 `http_proxy/https_proxy/all_proxy` 等代理环境变量。

**C-3 处理管线小改**
- `api/src/processing/match.js`、`api/src/processing/match-action.js`、`api/src/processing/request.js`、
  `api/src/processing/schema.js`、`api/src/processing/cookie/manager.js`、
  `api/src/processing/helpers/youtube-session.js`：配合上述隧道/UA/cookie 的小幅接线改动。
- `api/src/misc/crypto.js`（1 行）：`"aes256"` → `"aes-256-cbc"`（显式算法名，二者同一 cipher）。

**C-4 各站点提取器增强（上游升级时最易冲突，合并后重点回归）**
- `api/src/processing/services/bilibili.js`（+99/−47）：补桌面浏览器 UA / `referer` / `origin` 头、支持登录 cookie、清晰度选择。
- `api/src/processing/services/pinterest.js`（+33/−5）：优先选最高分辨率的 `.mp4`。
- `api/src/processing/services/twitter.js`（+19/−9）：`bestQuality()` 按分辨率在 `video/mp4` variants 中择优。
- `api/src/processing/services/youtube.js`（+20/−11）：透传 youtubei 客户端 UA 头。
- `api/src/processing/services/soundcloud.js`（+4/−5）：小幅调整。

### D. 纯文件模式位噪声（无内容差异）

- 基线时有 **89 个**文件被从上游的 `100644` 翻成了 `100755`（整树拷贝时带上的可执行位，内容 sha 与上游完全一致）。
- 这类差异不影响运行，但会污染每次 diff。建议在**首次同步的前置清理提交**里统一 `chmod 644`（Node 源码无需可执行位），见 §5。

## 4. 未跟踪的本地杂物（不入库，仅登记）

- `api/.DS_Store`、`api/node_modules/`
- `api/src-tauri/bundled-api/node_modules/`、`api/src-tauri/bundled-api-test/node_modules/`（历史构建残留）

## 5. 首次同步前的一次性清理（单独提交，**不要**混入合并提交）

```bash
# 1) 删除已入库的调试/临时脚本
git rm api/scratch_test_yt.js api/test_request.js api/src/util/debug-range.js api/src/util/test_youtube_ua.js
# 2) 统一文件模式位（消除 89 个 644->755 噪声）
git diff --name-only --diff-filter=M a636575b09de1fc55d9b8cd98cac88f5f2f16b42 -- api/ \
  | xargs -I{} sh -c 'case "$1" in *.js|*.ts|*.json|*.md|LICENSE) chmod 644 "$1";; esac' sh {}
git add -u api/
git commit -m "chore(api): drop local scratch files and normalize file modes before upstream sync"
```

> 未跟踪杂物（§4）无需提交删除；如需可加入 `.gitignore`。`src-tauri/bundled-api/` 的处置另开任务，不在同步里顺手删。

## 6. 季度同步 Runbook（建议每季度首周：1 / 4 / 7 / 10 月）

用脚本（默认 **dry-run**，只打印不改动）：

```bash
node scripts/sync-cobalt-api.mjs                 # 预检：fetch、显示 OLD->NEW、列出将变更文件与冲突点
node scripts/sync-cobalt-api.mjs --apply         # 真正生成并尝试三方应用子目录补丁
```

脚本完成后（或手工执行时）的完整步骤：

1. `git fetch upstream`，读本文件的旧 pin `$OLD`，选定 `$NEW`（默认 `upstream/main`；如上游未来开始打 api release tag，优先跟 tag 以降低波动）。
2. 生成子目录补丁并三方应用：
   ```bash
   git diff "$OLD" "$NEW" -- api/ > /tmp/cobalt-api.patch
   git apply -3 /tmp/cobalt-api.patch     # 在仓库根执行，路径前缀已是 api/
   ```
   冲突只会出现在 §3-C 的本地补丁处；A 类本地独有文件不受影响。
3. **恢复 C-0 本地依赖改动**：确认 `api/package.json` 仍是 `file:./version-info`、含 vitest、无 `isolated-vm`；
   确认 `api/version-info/` 三个文件还在（若上游改了 version-info 的生成逻辑，需重新生成该包）。
4. 安装与单测（必须全绿）：
   ```bash
   pnpm --dir api install
   pnpm --dir api test:unit --run
   ```
5. 更新本文件「Pin 元数据」与下方「同步历史」表（commit/日期/变更摘要/冲突文件）。
6. 部署远程：按 `docs/plans/2026-09-08-cobalt-api-server-migration.md` 更新 `/opt/cobalt-api`
   （独立 systemd、不碰其它服务），冒烟 `GET` 根路径的 `services` 列表 + 一次真实解析（重点回归 §3-C-4 的 B 站 / Pinterest / X / YouTube）。
7. 提交：**只提交 `api/` 与本文件相关变更**，走正常发版或单独 PR。
8. **AGPL-3.0 合规**：我们对外提供网络服务且仓库已开源，沿用现有开源披露；确认 `api/LICENSE` 与版本信息未被覆盖。

> 暂不把 `api/` 改造成 `git subtree`：先用「pin SHA + 子目录补丁」跑顺 1~2 个季度，待本地补丁（尤其 C-0/C-1）稳定收敛后再评估。

## 7. 同步历史

| 日期 | OLD commit | NEW commit | 上游版本 | 变更摘要 / 冲突文件 | 处理人 |
|---|---|---|---|---|---|
| 2026-10-09 | —（建立基线） | `a636575` | 11.7.1 | 初始 pin，盘点本地补丁（见 §3），未做合并 | — |
