import { execFileSync } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import { access, chmod, cp, mkdir, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

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

const sourceFfmpeg = execFileSync("node", [
  "-e",
  "console.log(require('ffmpeg-static'))"
], {
  cwd: join(root, "api"),
  encoding: "utf8"
}).trim();

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
console.log(`Verified yt-dlp runtime: ${bundledYtDlp}`);
console.log(`Built local resource sniffer: ${bundledSniffer}`);
