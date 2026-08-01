import { execFileSync } from "node:child_process";
import { cp, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const macosBundleDir = join(root, "src-tauri", "target", "release", "bundle", "macos");
const defaultApp = join(macosBundleDir, "Cobalt.app");
const ytDlpEntitlements = join(root, "src-tauri", "entitlements", "yt-dlp.plist");

function resolveSigningIdentity() {
  if (process.env.APPLE_SIGNING_IDENTITY?.trim()) {
    return process.env.APPLE_SIGNING_IDENTITY.trim();
  }
  const identities = execFileSync("security", ["find-identity", "-v", "-p", "codesigning"], {
    encoding: "utf8"
  });
  const match = identities.match(/([A-F0-9]{40})\s+"Developer ID Application:[^"]+"/);
  if (!match) {
    throw new Error("No valid Developer ID Application identity was found in the macOS keychain.");
  }
  return match[1];
}

function sign(path, identity, entitlements) {
  const args = [
    "--force",
    "--options", "runtime",
    "--timestamp",
    "--sign", identity
  ];
  if (entitlements) {
    args.push("--entitlements", entitlements);
  }
  args.push(path);
  execFileSync("codesign", args, { stdio: "inherit" });
}

const identity = resolveSigningIdentity();
for (const executable of ["ffmpeg", "res-sniffer"]) {
  sign(join(defaultApp, "Contents", "Resources", "binaries", executable), identity);
}
// yt-dlp's PyInstaller launcher loads an embedded Python framework carrying a
// different signature. Hardened Runtime otherwise rejects that library after
// we apply our Developer ID signature to the launcher.
sign(join(defaultApp, "Contents", "Resources", "binaries", "yt-dlp"), identity, ytDlpEntitlements);
// Keep the official Node.js Foundation signature. Signing the outer bundle
// after all other resources ensures its CodeResources seal is final.
sign(defaultApp, identity);
execFileSync("codesign", ["--verify", "--deep", "--strict", "--verbose=2", defaultApp], {
  stdio: "inherit"
});
execFileSync(join(defaultApp, "Contents", "Resources", "binaries", "yt-dlp"), ["--version"], {
  stdio: "inherit"
});

const now = new Date();
const pad = (n) => String(n).padStart(2, '0');
const timestamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}_${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;

const timestampedApp = join(macosBundleDir, `Cobalt_${timestamp}.app`);

await rm(timestampedApp, { recursive: true, force: true });
await cp(defaultApp, timestampedApp, { recursive: true });

console.log(`\n==================================================`);
console.log(`Timestamped Release Bundle created successfully:`);
console.log(`${timestampedApp}`);
console.log(`Signed with Developer ID identity: ${identity}`);
console.log(`==================================================\n`);
