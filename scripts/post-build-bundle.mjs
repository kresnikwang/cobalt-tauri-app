import { execFileSync, execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { cp, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const macosBundleDir = join(root, "src-tauri", "target", "release", "bundle", "macos");
const defaultApp = join(macosBundleDir, "Cobalt.app");
const ytDlpEntitlements = join(root, "src-tauri", "entitlements", "yt-dlp.plist");
const pythonEntitlements = join(root, "src-tauri", "entitlements", "python.plist");

// On CI there is no Developer ID certificate. Sign ad-hoc so the .app
// still has a consistent seal and Gatekeeper behaviour stays testable.
// Locally (or when APPLE_SIGNING_IDENTITY is provided) keep the strict
// Developer ID + hardened-runtime path.
const isCI = process.env.CI === "true" || process.env.GITHUB_ACTIONS === "true";
const skipSign = process.env.SKIP_SIGNING === "true" || process.env.SKIP_SIGNING === "1";

function resolveSigningIdentity() {
  if (process.env.APPLE_SIGNING_IDENTITY?.trim()) {
    return { identity: process.env.APPLE_SIGNING_IDENTITY.trim(), adHoc: false };
  }
  if (isCI || skipSign) {
    return { identity: "-", adHoc: true };
  }
  try {
    const identities = execFileSync("security", ["find-identity", "-v", "-p", "codesigning"], {
      encoding: "utf8"
    });
    const match = identities.match(/([A-F0-9]{40})\s+"Developer ID Application:[^"]+"/);
    if (match) return { identity: match[1], adHoc: false };
  } catch {
    // fall through to the error below
  }
  throw new Error("No valid Developer ID Application identity was found in the macOS keychain. Set APPLE_SIGNING_IDENTITY, or set SKIP_SIGNING=1 for an unsigned local build.");
}

function sign(path, identity, { entitlements, adHoc }) {
  const args = ["--force", "--sign", identity];
  if (adHoc) {
    // ad-hoc: no hardened runtime, no timestamp server needed.
  } else {
    args.push("--options", "runtime", "--timestamp");
  }
  if (entitlements && !adHoc) {
    args.push("--entitlements", entitlements);
  }
  args.push(path);
  execFileSync("codesign", args, { stdio: "inherit" });
}

const { identity, adHoc } = resolveSigningIdentity();
console.log(`Signing identity: ${identity}${adHoc ? " (ad-hoc, CI/unsigned mode)" : ""}`);
for (const executable of ["ffmpeg", "res-sniffer"]) {
  sign(join(defaultApp, "Contents", "Resources", "binaries", executable), identity, { adHoc });
}
// yt-dlp's PyInstaller launcher loads an embedded Python framework carrying a
// different signature. Hardened Runtime otherwise rejects that library after
// we apply our Developer ID signature to the launcher. (Only applies to real
// Developer ID signing; ad-hoc builds skip entitlements.)
sign(join(defaultApp, "Contents", "Resources", "binaries", "yt-dlp"), identity, {
  entitlements: ytDlpEntitlements,
  adHoc
});
// Bundled CPython (gallery-dl image engine): sign every extension module first,
// then the static interpreter. The interpreter disables library validation so
// its separately-signed .so modules still load under Hardened Runtime. This all
// has to happen before the outer-bundle seal below.
const pythonResourceDir = join(defaultApp, "Contents", "Resources", "binaries", "python");
if (existsSync(pythonResourceDir)) {
  const listFiles = (args) =>
    execFileSync("find", [pythonResourceDir, "-type", "f", ...args], { encoding: "utf8" })
      .split("\n").map((line) => line.trim()).filter(Boolean);
  for (const extension of listFiles(["(", "-name", "*.so", "-o", "-name", "*.dylib", ")"])) {
    sign(extension, identity, { adHoc });
  }
  const interpreters = listFiles(["-path", "*/bin/python3.*"]);
  if (interpreters.length === 0) {
    throw new Error(`Bundled Python interpreter missing under ${pythonResourceDir}/bin`);
  }
  for (const interpreter of interpreters) {
    sign(interpreter, identity, { entitlements: pythonEntitlements, adHoc });
  }
}
// Keep the official Node.js Foundation signature. Signing the outer bundle
// after all other resources ensures its CodeResources seal is final.
// (Ad-hoc mode still re-seals so `codesign --verify` passes.)
sign(defaultApp, identity, { adHoc });
execFileSync("codesign", ["--verify", "--deep", "--strict", "--verbose=2", defaultApp], {
  stdio: "inherit"
});
if (!adHoc) {
  execFileSync(join(defaultApp, "Contents", "Resources", "binaries", "yt-dlp"), ["--version"], {
    stdio: "inherit"
  });
} else {
  // yt-dlp re-signed ad-hoc cannot load its embedded framework; verify
  // presence instead of executing it.
  execSync(`test -x "${join(defaultApp, "Contents", "Resources", "binaries", "yt-dlp")}"`, { stdio: "inherit" });
  console.log("Ad-hoc mode: skipped executing re-signed yt-dlp (expected).");
}

const now = new Date();
const pad = (n) => String(n).padStart(2, '0');
const timestamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}_${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;

const timestampedApp = join(macosBundleDir, `Cobalt_${timestamp}.app`);

await rm(timestampedApp, { recursive: true, force: true });
await cp(defaultApp, timestampedApp, { recursive: true });

console.log(`\n==================================================`);
console.log(`Timestamped Release Bundle created successfully:`);
console.log(`${timestampedApp}`);
console.log(`Signed with ${adHoc ? "ad-hoc" : "Developer ID"} identity: ${identity}`);
console.log(`==================================================\n`);
