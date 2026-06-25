import { execFileSync } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import { access, chmod, cp, mkdir } from "node:fs/promises";
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

console.log(`Prepared Node runtime: ${bundledNode}`);
console.log(`Prepared FFmpeg runtime: ${bundledFfmpegRuntime}`);
console.log(`Verified yt-dlp runtime: ${bundledYtDlp}`);
