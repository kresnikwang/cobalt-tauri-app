import { execFileSync } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import { access, chmod, cp, mkdir, rm } from "node:fs/promises";
import { createWriteStream } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const tauriDir = join(root, "src-tauri");
const binariesDir = join(tauriDir, "binaries");

const nodePath = execFileSync("node", ["-p", "process.execPath"], {
  encoding: "utf8"
}).trim();

const bundledNode = join(binariesDir, "node");
const bundledFfmpegRuntime = join(binariesDir, "ffmpeg");
const bundledYtDlp = join(binariesDir, "yt-dlp");
const bundledSniffer = join(binariesDir, "res-sniffer");
const snifferSource = join(root, "sidecars", "res-sniffer");

function resolveFfmpegBinary() {
  // NOTE: this file is ESM, so `require` is not defined here.
  // Delegate to a CJS one-liner in a child node process instead.
  // `pnpm add -D` style installs may place ffmpeg-static under the
  // workspace root or under api/node_modules — try both.
  const candidates = [
    `process.env.FFMPEG_BIN && console.log(process.env.FFMPEG_BIN)`,
    `console.log(require('ffmpeg-static'))`,
  ];
  const cwds = [join(root, "api"), root];
  let lastError;
  for (const cwd of cwds) {
    for (const snippet of candidates) {
      try {
        const out = execFileSync("node", ["-e", snippet], {
          cwd,
          encoding: "utf8"
        }).trim().split("\n").pop().trim();
        if (out && !out.startsWith("undefined")) return out;
      } catch (error) {
        lastError = error;
      }
    }
  }
  throw new Error(
    `Unable to locate the ffmpeg-static binary. Install dependencies first (pnpm install): ${lastError?.message ?? "unknown error"}`
  );
}

async function fileExists(path) {
  try {
    await access(path, fsConstants.X_OK);
    return true;
  } catch {
    return false;
  }
}

async function ensureYtDlp() {
  if (await fileExists(bundledYtDlp)) {
    return { downloaded: false };
  }
  // CI runners start from a clean checkout where src-tauri/binaries/ is
  // git-ignored, so there is no local yt-dlp to verify. Download the
  // official static macOS arm64 build instead of failing the job.
  const overrideUrl = process.env.YT_DLP_URL?.trim();
  const version = process.env.YT_DLP_VERSION?.trim() || "2025.11.12";
  const platform = process.platform;
  const arch = process.arch;
  let url = overrideUrl;
  if (!url) {
    if (platform === "darwin" && arch === "arm64") {
      url = `https://github.com/yt-dlp/yt-dlp/releases/download/${version}/yt-dlp_macos`;
    } else if (platform === "darwin") {
      url = `https://github.com/yt-dlp/yt-dlp/releases/download/${version}/yt-dlp_macos_legacy`;
    } else if (platform === "linux" && arch === "arm64") {
      url = `https://github.com/yt-dlp/yt-dlp/releases/download/${version}/yt-dlp_linux_aarch64`;
    } else if (platform === "linux") {
      url = `https://github.com/yt-dlp/yt-dlp/releases/download/${version}/yt-dlp_linux`;
    } else {
      throw new Error(
        `No yt-dlp binary at ${bundledYtDlp} and no automatic download for ${platform}/${arch}. ` +
        `Provide one manually or set YT_DLP_URL to a direct download.`
      );
    }
  }
  console.log(`yt-dlp not found locally, downloading from ${url} ...`);
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok || !response.body) {
    throw new Error(`Failed to download yt-dlp (${response.status} ${response.statusText}) from ${url}`);
  }
  await mkdir(binariesDir, { recursive: true });
  const out = createWriteStream(bundledYtDlp, { mode: 0o755 });
  await pipeline(response.body, out);
  await chmod(
    bundledYtDlp,
    fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR |
    fsConstants.S_IRGRP | fsConstants.S_IXGRP |
    fsConstants.S_IROTH | fsConstants.S_IXOTH
  );
  return { downloaded: true, url };
}

const sourceFfmpeg = resolveFfmpegBinary();

await mkdir(binariesDir, { recursive: true });
await cp(nodePath, bundledNode, { force: true });
await chmod(bundledNode, fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR
  | fsConstants.S_IRGRP | fsConstants.S_IXGRP
  | fsConstants.S_IROTH | fsConstants.S_IXOTH);
await cp(sourceFfmpeg, bundledFfmpegRuntime, { force: true });
await chmod(bundledFfmpegRuntime, fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR
  | fsConstants.S_IRGRP | fsConstants.S_IXGRP
  | fsConstants.S_IROTH | fsConstants.S_IXOTH);

await access(bundledFfmpegRuntime, fsConstants.X_OK);
await access(bundledNode, fsConstants.X_OK);

const ytDlpResult = await ensureYtDlp();
await access(bundledYtDlp, fsConstants.X_OK);

try {
  // The repository carries a tiny executable placeholder so Cargo can resolve
  // resources in dev mode. Go 1.26 refuses to overwrite that non-object file.
  await rm(bundledSniffer, { force: true });
  execFileSync("go", ["build", "-trimpath", "-ldflags", "-s -w", "-o", bundledSniffer, "./cmd/res-sniffer"], {
    cwd: snifferSource,
    stdio: "inherit"
  });
  await chmod(bundledSniffer, fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR
    | fsConstants.S_IRGRP | fsConstants.S_IXGRP
    | fsConstants.S_IROTH | fsConstants.S_IXOTH);
  await access(bundledSniffer, fsConstants.X_OK);
  execFileSync(bundledSniffer, ["--help"], { stdio: "ignore" });
} catch (error) {
  throw new Error(`Unable to build the resource sniffer. Install Go 1.22+ and retry: ${error.message}`);
}

console.log(`Prepared Node runtime: ${bundledNode}`);
console.log(`Prepared FFmpeg runtime: ${bundledFfmpegRuntime}`);
console.log(`Verified yt-dlp runtime: ${bundledYtDlp}${ytDlpResult.downloaded ? ` (downloaded from ${ytDlpResult.url})` : ""}`);
console.log(`Built local resource sniffer: ${bundledSniffer}`);
