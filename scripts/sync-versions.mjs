import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const files = {
  packageJson: join(root, "package.json"),
  tauriConf: join(root, "src-tauri", "tauri.conf.json"),
  cargoToml: join(root, "src-tauri", "Cargo.toml"),
};

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function currentVersions() {
  const pkg = readJson(files.packageJson);
  const conf = readJson(files.tauriConf);
  const cargo = readFileSync(files.cargoToml, "utf8");
  const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  return { packageJson: pkg.version, tauriConf: conf.version, cargoToml: cargoVersion };
}

function usage() {
  console.log(`Usage:
  node scripts/sync-versions.mjs <version>   Align package.json, tauri.conf.json and Cargo.toml
  node scripts/sync-versions.mjs --check      Verify all three versions match (and optionally match $RELEASE_VERSION)`);
}

const arg = process.argv[2];
if (!arg || arg === "--help" || arg === "-h") {
  usage();
  process.exit(arg ? 0 : 1);
}

if (arg === "--check") {
  const versions = currentVersions();
  const expected = process.env.RELEASE_VERSION?.replace(/^v/, "").trim() || null;
  const values = Object.values(versions);
  const allEqual = values.every((v) => v === values[0]);
  console.log(`Versions: package.json=${versions.packageJson} tauri.conf.json=${versions.tauriConf} Cargo.toml=${versions.cargoToml}`);
  if (expected) console.log(`Expected (RELEASE_VERSION): ${expected}`);
  if (!allEqual) {
    console.error("Version mismatch: run `node scripts/sync-versions.mjs <version>` to align them.");
    process.exit(1);
  }
  if (expected && values[0] !== expected) {
    console.error(`Version ${values[0]} does not match tag version ${expected}.`);
    process.exit(1);
  }
  console.log("Versions are in sync.");
  process.exit(0);
}

const version = arg.replace(/^v/, "").trim();
if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version)) {
  console.error(`Invalid version: ${arg}. Expected semver like 1.0.10`);
  process.exit(1);
}

const pkg = readJson(files.packageJson);
pkg.version = version;
writeFileSync(files.packageJson, JSON.stringify(pkg, null, 2) + "\n");

const conf = readJson(files.tauriConf);
conf.version = version;
writeFileSync(files.tauriConf, JSON.stringify(conf, null, 2) + "\n");

const cargo = readFileSync(files.cargoToml, "utf8");
if (!/^version\s*=\s*"[^"]+"/m.test(cargo)) throw new Error("Could not find version field in src-tauri/Cargo.toml");
const updated = cargo.replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`);
writeFileSync(files.cargoToml, updated);

// Keep Cargo.lock in sync so `cargo check --locked` passes in CI.
// Best-effort: if cargo is unavailable, CI will surface the mismatch.
try {
  const { execFileSync } = await import("node:child_process");
  execFileSync("cargo", ["check", "--manifest-path", join(root, "src-tauri", "Cargo.toml")], {
    stdio: "ignore"
  });
  console.log("Cargo.lock updated.");
} catch {
  console.warn("WARNING: could not update Cargo.lock automatically (cargo unavailable). Run `cargo check --manifest-path src-tauri/Cargo.toml` locally and commit the lockfile.");
}

console.log(`Synced all versions to ${version}`);
