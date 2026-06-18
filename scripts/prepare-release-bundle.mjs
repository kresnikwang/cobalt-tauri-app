import { execFileSync } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import { access, chmod, cp, mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const tauriDir = join(root, "src-tauri");
const bundledApiDir = join(tauriDir, "bundled-api");
const binariesDir = join(tauriDir, "binaries");

const nodePath = execFileSync("node", ["-p", "process.execPath"], {
  encoding: "utf8"
}).trim();

const bundledNode = join(binariesDir, "node");
const bundledFfmpegRuntime = join(binariesDir, "ffmpeg");

await rm(bundledApiDir, { recursive: true, force: true });
await mkdir(bundledApiDir, { recursive: true });
await cp(join(root, "api"), bundledApiDir, {
  recursive: true,
  dereference: true,
  filter: (source) => !source.includes("/node_modules/")
    && !source.endsWith("/.DS_Store")
    && !source.endsWith("/scratch_test_yt.js")
    && !source.endsWith("/test_request.js")
    && !source.endsWith("/vitest.config.ts")
});
await writeFile(join(bundledApiDir, "pnpm-workspace.yaml"), `packages:
  - "."
allowBuilds:
  esbuild: true
  ffmpeg-static: true
  syscall-napi: true
`);
execFileSync("pnpm", [
  "install",
  "--prod",
  "--node-linker=hoisted",
  "--no-optional",
  "--ignore-scripts=false"
], {
  cwd: bundledApiDir,
  stdio: "inherit"
});

const sourceFfmpeg = execFileSync("node", [
  "-e",
  "console.log(require('ffmpeg-static'))"
], {
  cwd: join(root, "api"),
  encoding: "utf8"
}).trim();
const bundledFfmpeg = execFileSync("node", [
  "-e",
  "console.log(require('ffmpeg-static'))"
], {
  cwd: bundledApiDir,
  encoding: "utf8"
}).trim();
await cp(sourceFfmpeg, bundledFfmpeg, { force: true });
await chmod(bundledFfmpeg, fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR
  | fsConstants.S_IRGRP | fsConstants.S_IXGRP
  | fsConstants.S_IROTH | fsConstants.S_IXOTH);

await mkdir(binariesDir, { recursive: true });
await cp(nodePath, bundledNode, { force: true });
await chmod(bundledNode, fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR
  | fsConstants.S_IRGRP | fsConstants.S_IXGRP
  | fsConstants.S_IROTH | fsConstants.S_IXOTH);
await cp(sourceFfmpeg, bundledFfmpegRuntime, { force: true });
await chmod(bundledFfmpegRuntime, fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR
  | fsConstants.S_IRGRP | fsConstants.S_IXGRP
  | fsConstants.S_IROTH | fsConstants.S_IXOTH);

await access(join(bundledApiDir, "src", "cobalt.js"), fsConstants.R_OK);
await access(bundledFfmpeg, fsConstants.X_OK);
await access(bundledFfmpegRuntime, fsConstants.X_OK);

console.log(`Prepared release API bundle: ${bundledApiDir}`);
console.log(`Prepared Node runtime: ${bundledNode}`);
console.log(`Prepared FFmpeg runtime: ${bundledFfmpegRuntime}`);
