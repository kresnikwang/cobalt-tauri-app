#!/usr/bin/env node
// Sync the vendored api/ snapshot from imputnet/cobalt.
//
// api/ is a copy of the `api/` subdirectory of the imputnet/cobalt monorepo,
// pinned to a commit recorded in api/UPSTREAM.md. This script generates a
// subdirectory-restricted diff between the old and new upstream commits and
// 3-way-applies it at the repo root, so local additions (version-info/, the
// vitest suite, custom tunnel code) are preserved.
//
// Safe by default: runs as a DRY RUN and never commits or edits UPSTREAM.md.
// See api/UPSTREAM.md §6 for the full quarterly runbook.
//
// Usage:
//   node scripts/sync-cobalt-api.mjs                 dry run: fetch, show OLD->NEW, predict conflicts
//   node scripts/sync-cobalt-api.mjs --apply         generate and 3-way-apply the patch, then run unit tests
//   node scripts/sync-cobalt-api.mjs --to <ref>      target a specific upstream ref/tag (default: upstream/main)
//   node scripts/sync-cobalt-api.mjs --no-fetch      skip `git fetch upstream`
//   node scripts/sync-cobalt-api.mjs --install       also run `pnpm --dir api install` before tests
//   node scripts/sync-cobalt-api.mjs --force         apply even with a dirty working tree (not recommended)

import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const UPSTREAM_URL = "git@github.com:imputnet/cobalt.git";
const SUBDIR = "api/";
const UPSTREAM_DOC = join(root, "api", "UPSTREAM.md");
const PATCH_PATH = "/tmp/cobalt-api.patch";

// Files we locally patched on top of upstream (api/UPSTREAM.md §3-C).
// Upstream changes to these are the likely 3-way conflict points.
const LOCAL_PATCH_HINTS = [
    "api/package.json",
    "api/src/core/api.js",
    "api/src/core/itunnel.js",
    "api/src/stream/internal.js",
    "api/src/stream/manage.js",
    "api/src/stream/proxy.js",
    "api/src/stream/stream.js",
    "api/src/stream/shared.js",
    "api/src/stream/ffmpeg.js",
    "api/src/processing/match.js",
    "api/src/processing/match-action.js",
    "api/src/processing/request.js",
    "api/src/processing/schema.js",
    "api/src/processing/cookie/manager.js",
    "api/src/processing/helpers/youtube-session.js",
    "api/src/processing/services/bilibili.js",
    "api/src/processing/services/pinterest.js",
    "api/src/processing/services/twitter.js",
    "api/src/processing/services/youtube.js",
    "api/src/processing/services/soundcloud.js",
    "api/src/misc/crypto.js",
];

const argv = process.argv.slice(2);
const has = (flag) => argv.includes(flag);
const opt = (flag) => {
    const i = argv.indexOf(flag);
    return i >= 0 ? argv[i + 1] : undefined;
};
const APPLY = has("--apply");
const DO_FETCH = !has("--no-fetch");
const DO_INSTALL = has("--install");
const RUN_TESTS = !has("--no-test");
const FORCE = has("--force");
const TARGET = opt("--to") || "upstream/main";

function git(args, opts = {}) {
    return execFileSync("git", args, { cwd: root, encoding: "utf8", ...opts }).trim();
}
function gitOk(args) {
    return spawnSync("git", args, { cwd: root, encoding: "utf8" });
}
function log(msg = "") {
    console.log(msg);
}
function section(title) {
    log(`\n=== ${title} ===`);
}
function fail(msg) {
    console.error(`\n✗ ${msg}`);
    process.exit(1);
}

// 1. Make sure the read-only upstream remote exists (local .git/config only).
function ensureRemote() {
    const remotes = git(["remote"]).split("\n").filter(Boolean);
    if (!remotes.includes("upstream")) {
        log(`Adding read-only remote 'upstream' -> ${UPSTREAM_URL}`);
        git(["remote", "add", "upstream", UPSTREAM_URL]);
    }
}

// 2. Read the current pin from api/UPSTREAM.md ("Pinned commit | `<40sha>`").
function readOldPin() {
    if (!existsSync(UPSTREAM_DOC)) fail(`Missing ${UPSTREAM_DOC}; establish the baseline first (Task 2.1).`);
    const text = readFileSync(UPSTREAM_DOC, "utf8");
    const m = text.match(/Pinned commit\s*\|\s*`([0-9a-f]{7,40})`/i);
    if (!m) fail("Could not parse 'Pinned commit' from api/UPSTREAM.md.");
    return m[1];
}

function refInfo(ref) {
    const sha = git(["rev-parse", ref]);
    const date = git(["show", "-s", "--format=%cI", ref]);
    const subject = git(["show", "-s", "--format=%s", ref]);
    let version = "?";
    const pv = gitOk(["show", `${ref}:api/package.json`]);
    if (pv.status === 0) {
        const vm = pv.stdout.match(/"version"\s*:\s*"([^"]+)"/);
        if (vm) version = vm[1];
    }
    return { sha, date, subject, version };
}

function apiVersionAt(ref) {
    return refInfo(ref).version;
}

// ---------------------------------------------------------------------------
ensureRemote();
if (DO_FETCH) {
    section("Fetching upstream");
    const r = gitOk(["fetch", "upstream", "--tags"]);
    if (r.status !== 0) fail(`git fetch upstream failed:\n${r.stderr || r.stdout}`);
    log("Fetched imputnet/cobalt (branches + tags).");
}

const OLD = readOldPin();
if (!/^[0-9a-f]{7,40}$/.test(OLD) || gitOk(["cat-file", "-e", `${OLD}^{commit}`]).status !== 0) {
    fail(`Old pin ${OLD} not found locally; rerun without --no-fetch.`);
}
const NEW_RESOLVED = git(["rev-parse", TARGET]);

const oldInfo = refInfo(OLD);
const newInfo = refInfo(NEW_RESOLVED);

section("Sync plan");
log(`OLD (current pin): ${oldInfo.sha}`);
log(`                   ${oldInfo.date}  api ${oldInfo.version}  ${oldInfo.subject}`);
log(`NEW (target):      ${newInfo.sha}   [${TARGET}]`);
log(`                   ${newInfo.date}  api ${newInfo.version}  ${newInfo.subject}`);

if (oldInfo.sha === newInfo.sha) {
    log("\nAlready at the pinned commit — nothing to sync.");
    process.exit(0);
}

// Files changed upstream under api/, and overlap with our local patches.
const nameStatus = git(["diff", "--name-status", OLD, NEW_RESOLVED, "--", SUBDIR]);
const changed = nameStatus
    .split("\n")
    .filter(Boolean)
    .map((line) => {
        const [status, ...paths] = line.split("\t");
        return { status, path: paths[paths.length - 1] };
    });

section(`Upstream changes under ${SUBDIR}(${changed.length} files)`);
const counts = {};
for (const c of changed) counts[c.status[0]] = (counts[c.status[0]] || 0) + 1;
log(`By type: ${Object.entries(counts).map(([k, v]) => `${k}=${v}`).join(", ")}`);

const conflicts = changed
    .filter((c) => LOCAL_PATCH_HINTS.includes(c.path))
    .map((c) => c.path);
if (conflicts.length) {
    log("\n! These changed files also carry LOCAL patches — expect 3-way conflicts / manual review:");
    for (const p of conflicts) log(`    - ${p}`);
    log("  After applying, re-apply local changes; api/package.json MUST stay file:./version-info + vitest, no isolated-vm.");
}

// Always (re)generate the patch so it can be inspected/applied by hand too.
// NOTE: use the raw diff bytes — trimming the trailing newline makes
// `git apply` report "corrupt patch" on the final hunk.
const patch = execFileSync("git", ["diff", OLD, NEW_RESOLVED, "--", SUBDIR], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
});
writeFileSync(PATCH_PATH, patch, "utf8");
log(`\nSubdir patch written to ${PATCH_PATH} (${patch.split("\n").length} lines).`);

// Predict whether it applies cleanly. Note: `git apply --3way --check` is
// optimistic — it cannot always reproduce the full index 3-way merge, so files
// listed above as carrying local patches can STILL conflict on real --apply.
const check = gitOk(["apply", "--3way", "--check", PATCH_PATH]);
const appliesCleanly = check.status === 0;
log(`Apply check (git apply --3way --check): ${appliesCleanly ? "no obvious blocker" : "reports conflicts"}`);
if (!appliesCleanly && check.stderr.trim()) log(check.stderr.trim().split("\n").slice(0, 12).join("\n"));
if (appliesCleanly && conflicts.length) {
    log("(check is optimistic — the overlap files listed above can still conflict on real --apply.)");
}

if (!APPLY) {
    section("Dry run (no changes made)");
    log("To apply:");
    log("  1. node scripts/sync-cobalt-api.mjs --apply");
    log("  2. Resolve any conflicts above; restore api/package.json local deps and api/version-info/.");
    log("  3. pnpm --dir api install && pnpm --dir api test:unit --run");
    log("  4. Update the pin + sync-history table in api/UPSTREAM.md, then deploy /opt/cobalt-api (see UPSTREAM.md §6).");
    process.exit(appliesCleanly ? 0 : 2);
}

// --apply: require a clean TRACKED tree (at least under api/); untracked files
// (node_modules, a not-yet-committed UPSTREAM.md, local junk) don't block a patch.
const dirty = git(["status", "--porcelain", "--untracked-files=no", "--", SUBDIR, UPSTREAM_DOC]).trim();
if (dirty && !FORCE) {
    fail(`Working tree has uncommitted changes under ${SUBDIR}or api/UPSTREAM.md.\nCommit/stash them first, or rerun with --force.\n${dirty}`);
}

section("Applying patch (3-way)");
const apply = gitOk(["apply", "--3way", PATCH_PATH]);
if (apply.status !== 0) {
    log(apply.stdout);
    log(apply.stderr);
    fail("3-way apply produced conflicts. Resolve the unmerged paths in api/, then run the install/tests and update UPSTREAM.md.");
}
log("Patch applied.");

// AGPL guard: the license must never disappear during a sync.
if (!existsSync(join(root, "api", "LICENSE"))) {
    fail("api/LICENSE is missing after apply — restore it (AGPL-3.0 compliance).");
}

// Local-dependency guard: version-info must still resolve offline.
const pkg = JSON.parse(readFileSync(join(root, "api", "package.json"), "utf8"));
const dep = pkg.dependencies?.["@imput/version-info"];
if (dep !== "file:./version-info" || !existsSync(join(root, "api", "version-info", "index.js"))) {
    log("! WARNING: local vendoring of @imput/version-info is not intact "
        + `(dep=${dep}). Restore "file:./version-info" and api/version-info/ before installing.`);
}
if (pkg.devDependencies?.vitest === undefined) {
    log("! WARNING: vitest devDependency missing; restore it so `test:unit` works.");
}
if (pkg.dependencies?.["isolated-vm"] !== undefined) {
    log("! NOTE: upstream reintroduced isolated-vm; confirm the server/bundle can build it or re-remove it.");
}

if (DO_INSTALL) {
    section("pnpm --dir api install");
    const inst = gitOk(["pnpm", "--dir", "api", "install"]);
    log(inst.stdout);
    if (inst.status !== 0) fail(`pnpm install failed:\n${inst.stderr}`);
}

if (RUN_TESTS) {
    section("Unit tests (pnpm --dir api test:unit --run)");
    const test = gitOk(["pnpm", "--dir", "api", "test:unit", "--run"]);
    log(test.stdout);
    if (test.status !== 0) {
        log(test.stderr);
        fail("api unit tests failed; fix before committing the sync.");
    }
} else {
    section("Unit tests skipped (--no-test)");
    log("Remember to run: pnpm --dir api install && pnpm --dir api test:unit --run");
}

section("Next steps (not done automatically)");
log("1. Update api/UPSTREAM.md pin + sync history:");
log(`     Pinned commit: ${newInfo.sha}`);
log(`     date/version : ${newInfo.date}  api ${newInfo.version}`);
log(`     subject      : ${newInfo.subject}`);
log("2. Deploy /opt/cobalt-api per docs/plans/2026-09-08-cobalt-api-server-migration.md and smoke-test.");
log("3. Commit ONLY api/ + api/UPSTREAM.md changes; do not touch src-tauri/bundled-api in this commit.");
log("\nSync applied and tests passed.");
