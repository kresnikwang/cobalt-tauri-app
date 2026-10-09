import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { constants as fsConstants } from "node:fs";
import { createWriteStream, mkdtempSync, rmSync, readFileSync } from "node:fs";
import { access, chmod, cp, mkdir, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
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

// --- Relocatable Python + gallery-dl (image/gallery engine) ---
// gallery-dl has no official macOS binary, so we bundle a relocatable,
// statically-linked CPython from astral-sh/python-build-standalone and install
// gallery-dl into its prefix site-packages. The interpreter is invoked as
// `<bundle>/bin/python3.12 -I -m gallery_dl` (isolated + relocatable: it finds
// its stdlib relative to the executable and ignores ambient PYTHON* vars).
const pythonBuildTag = process.env.PYTHON_BUILD_TAG?.trim() || "20261003";
const pythonVersion = process.env.PYTHON_VERSION?.trim() || "3.12.15";
const galleryDlVersion = process.env.GALLERY_DL_VERSION?.trim() || "1.32.16";
// Pin official PyPI so an ambient, possibly stale pip mirror cannot downgrade
// gallery-dl. Override with GALLERY_DL_INDEX_URL if needed.
const pythonPackageIndex = process.env.GALLERY_DL_INDEX_URL?.trim() || "https://pypi.org/simple";
const bundledPythonDir = join(binariesDir, "python");
const pythonMajMin = pythonVersion.split(".").slice(0, 2).join(".");
const bundledPythonBin = join(bundledPythonDir, "bin", `python${pythonMajMin}`);
const bundledPythonLib = join(bundledPythonDir, "lib", `python${pythonMajMin}`);

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

async function sha256OfFile(path) {
  const hash = createHash("sha256");
  hash.update(readFileSync(path));
  return hash.digest("hex");
}

async function rmIfExists(path) {
  await rm(path, { recursive: true, force: true });
}

// Remove everything in a directory whose basename matches one of `regexes`.
async function removeMatching(dir, regexes) {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }
  await Promise.all(
    entries.map(async (entry) => {
      if (regexes.some((re) => re.test(entry.name))) {
        await rm(join(dir, entry.name), { recursive: true, force: true });
      }
    })
  );
}

async function ensurePythonGallerydl() {
  if (process.platform !== "darwin" || process.arch !== "arm64") {
    throw new Error(
      `Bundled Python/gallery-dl is only prepared for darwin/arm64, got ${process.platform}/${process.arch}.`
    );
  }

  const sitePackages = join(bundledPythonLib, "site-packages");
  const galleryPackage = join(sitePackages, "gallery_dl");
  if ((await fileExists(bundledPythonBin)) && (await fileExists(galleryPackage))) {
    return { downloaded: false };
  }

  // 1) Download the relocatable, stripped CPython and verify against SHA256SUMS.
  const base = `https://github.com/astral-sh/python-build-standalone/releases/download/${pythonBuildTag}`;
  const tarName = `cpython-${pythonVersion}+${pythonBuildTag}-aarch64-apple-darwin-install_only_stripped.tar.gz`;
  const work = mkdtempSync(join(tmpdir(), "cobalt-python-"));
  const tarPath = join(work, tarName);
  console.log(`Downloading relocatable Python ${pythonVersion} (${pythonBuildTag}) ...`);
  let response = await fetch(`${base}/${encodeURIComponent(tarName)}`, { redirect: "follow" });
  if (!response.ok || !response.body) {
    throw new Error(`Failed to download Python runtime (${response.status} ${response.statusText}).`);
  }
  await pipeline(response.body, createWriteStream(tarPath));

  const sumsResponse = await fetch(`${base}/SHA256SUMS`, { redirect: "follow" });
  if (!sumsResponse.ok) {
    throw new Error(`Failed to download SHA256SUMS (${sumsResponse.status} ${sumsResponse.statusText}).`);
  }
  const expectedLine = (await sumsResponse.text())
    .split(/\r?\n/)
    .find((line) => line.endsWith(`  ${tarName}`));
  const expectedHash = expectedLine?.split(/\s+/)[0];
  if (!expectedHash) {
    throw new Error(`SHA256SUMS does not contain an entry for ${tarName}.`);
  }
  const actualHash = await sha256OfFile(tarPath);
  if (actualHash !== expectedHash) {
    throw new Error(`Python runtime checksum mismatch.\n  expected ${expectedHash}\n  got      ${actualHash}`);
  }
  console.log("Python runtime SHA256 verified.");

  // 2) Extract (archive root is ./python) and install gallery-dl into the
  //    runtime's own prefix site-packages (no venv -> venvs bake absolute,
  //    non-relocatable paths).
  await rmIfExists(bundledPythonDir);
  await mkdir(binariesDir, { recursive: true });
  execFileSync("tar", ["-xzf", tarPath, "-C", binariesDir], { stdio: "inherit" });
  if (!(await fileExists(bundledPythonBin))) {
    throw new Error(`Python extraction did not produce ${bundledPythonBin}.`);
  }
  console.log(`Installing gallery-dl ${galleryDlVersion} from ${pythonPackageIndex} ...`);
  execFileSync(
    bundledPythonBin,
    [
      "-I", "-m", "pip", "install",
      "--no-cache-dir", "--no-compile", "--upgrade",
      "-i", pythonPackageIndex,
      "--target", sitePackages,
      `gallery-dl==${galleryDlVersion}`,
    ],
    { stdio: "inherit" }
  );

  // 3) Slim the runtime for distribution. The install_only build is a STATIC
  //    interpreter: _ssl/_hashlib/_socket/zlib/_lzma/_sqlite3 are compiled into
  //    the single python3.x binary and nothing references libpython3.x.dylib,
  //    so we can drop that dylib plus all Tcl/Tk + GUI/dev/test cruft. This
  //    takes ~71MB down to ~33MB and leaves only one Mach-O to sign.
  await rmIfExists(join(bundledPythonDir, "lib", `libpython${pythonMajMin}.dylib`));
  await rmIfExists(join(bundledPythonLib, "tkinter"));
  await rmIfExists(join(bundledPythonLib, "turtledemo"));
  await rmIfExists(join(bundledPythonLib, "ensurepip"));
  await rmIfExists(join(bundledPythonLib, "idlelib"));
  await rmIfExists(join(bundledPythonLib, "lib2to3"));
  await rmIfExists(join(bundledPythonLib, "unittest"));
  await rmIfExists(join(bundledPythonLib, "pydoc_data"));
  await rmIfExists(join(bundledPythonLib, "test"));
  await rmIfExists(join(bundledPythonLib, `config-${pythonMajMin}-darwin`));
  await rmIfExists(join(bundledPythonDir, "include"));
  await rmIfExists(join(bundledPythonDir, "share", "man"));
  await rmIfExists(join(bundledPythonDir, "lib", "pkgconfig"));
  await removeMatching(join(bundledPythonLib, "lib-dynload"), [/^_tkinter/]);
  await removeMatching(sitePackages, [/^pip($|-)/]);
  // Tcl/Tk runtimes and headers.
  await removeMatching(join(bundledPythonDir, "lib"), [/^(tcl|tk|itcl|thread)/, /^lib(tcl|tk)/]);
  // bin/: keep only the real python3.x binary; drop symlinks and helper scripts.
  await removeMatching(join(bundledPythonDir, "bin"), [/^(?!python3\.\d+$).+/]);

  await chmod(
    bundledPythonBin,
    fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR |
    fsConstants.S_IRGRP | fsConstants.S_IXGRP |
    fsConstants.S_IROTH | fsConstants.S_IXOTH
  );

  // 4) Smoke test: hermetic run, TLS built-ins, and the gallery-dl package.
  const version = execFileSync(bundledPythonBin, ["-I", "-m", "gallery_dl", "--version"], {
    encoding: "utf8",
  }).trim();
  if (!version.startsWith(galleryDlVersion)) {
    throw new Error(`gallery-dl smoke test expected ${galleryDlVersion}, got '${version}'.`);
  }
  execFileSync(bundledPythonBin, ["-I", "-c", "import _ssl,_hashlib,certifi,gallery_dl"], { stdio: "inherit" });

  // The smoke run above regenerates bytecode caches; remove them last so the
  // shipped tree is clean (a read-only bundle cannot reuse them anyway, and a
  // cold gallery-dl start only costs ~0.15s without them).
  execFileSync("find", [bundledPythonDir, "-name", "__pycache__", "-type", "d", "-prune", "-exec", "rm", "-rf", "{}", "+"], { stdio: "inherit" });
  execFileSync("find", [bundledPythonDir, "-name", "*.pyc", "-delete"], { stdio: "inherit" });

  return { downloaded: true, version };
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

const pythonResult = await ensurePythonGallerydl();
await access(bundledPythonBin, fsConstants.X_OK);

// Stage third-party license texts INTO the bundled Python prefix. The
// install_only_stripped CPython tarball ships only CPython's own
// lib/pythonX.Y/LICENSE.txt and omits the licenses of its statically-linked
// components (OpenSSL, zlib, bzip2, liblzma, libffi, SQLite, expat, ...), so
// canonical copies are vendored in the repo under third_party/ and copied here
// (the whole binaries/python directory is mapped as a Tauri resource, so these
// ship inside the app). Idempotent: runs on every prepare, even on cache hit.
const bundledPythonLicenses = join(bundledPythonDir, "licenses");
const thirdPartyPython = join(root, "third_party", "python-build-standalone");
const thirdPartyGalleryDl = join(root, "third_party", "gallery-dl");
await rm(bundledPythonLicenses, { recursive: true, force: true });
await mkdir(bundledPythonLicenses, { recursive: true });
await cp(join(thirdPartyPython, "LICENSE"), join(bundledPythonLicenses, "python-build-standalone.LICENSE.txt"));
await cp(join(thirdPartyPython, "licenses"), join(bundledPythonLicenses, "components"), { recursive: true });
await cp(join(thirdPartyGalleryDl, "LICENSE"), join(bundledPythonLicenses, "gallery-dl.GPL-2.0-only.txt"));
console.log("Bundled Python/gallery-dl license texts staged under binaries/python/licenses.");

try {
  // The repository carries a tiny executable placeholder so Cargo can resolve
  // resources in dev mode. Go 1.26 refuses to overwrite that non-object file.
  await rm(bundledSniffer, { force: true });
  // NOTE: no `-ldflags "-s -w"` here. Stripping LC_UUID breaks
  // `go test` binaries on newer macOS (dyld: missing LC_UUID).
  execFileSync("go", ["build", "-trimpath", "-o", bundledSniffer, "./cmd/res-sniffer"], {
    cwd: snifferSource,
    stdio: "inherit"
  });
  await chmod(bundledSniffer, fsConstants.S_IRUSR | fsConstants.S_IWUSR | fsConstants.S_IXUSR
    | fsConstants.S_IRGRP | fsConstants.S_IXGRP
    | fsConstants.S_IROTH | fsConstants.S_IXOTH);
  await access(bundledSniffer, fsConstants.X_OK);
  // Smoke-test the sidecar: it speaks JSON over stdio and requires
  // --data-dir (it has no --help flag). Send a `status` command and
  // expect a `ready` + `response` pair back. Non-fatal: the dedicated
  // `go test` job already covers sidecar logic; some CI sandboxes
  // restrict spawning the freshly built binary.
  try {
    await new Promise((resolve, reject) => {
      const dataDir = mkdtempSync(join(tmpdir(), "cobalt-sniffer-smoke-"));
      const child = spawn(bundledSniffer, ["--data-dir", dataDir], { stdio: ["pipe", "pipe", "pipe"] });
      let output = "";
      let stderr = "";
      const done = (fn) => (...args) => {
        clearTimeout(timer);
        try { child.kill("SIGKILL"); } catch {}
        try { rmSync(dataDir, { recursive: true, force: true }); } catch {}
        fn(...args);
      };
      const timer = setTimeout(() => {
        done(reject)(new Error(`resource sniffer smoke test timed out. stdout: ${output} stderr: ${stderr}`));
      }, 15000);
      child.stdout.on("data", (chunk) => {
        output += chunk.toString();
        if (output.includes('"ready"') && output.includes('"response"')) {
          done(resolve)();
        }
      });
      child.stderr.on("data", (chunk) => { stderr += chunk.toString(); });
      child.on("exit", (code, signal) => {
        if (code !== null && code !== 0 && !(output.includes('"ready"') && output.includes('"response"'))) {
          done(reject)(new Error(`resource sniffer exited early (code=${code} signal=${signal}). stdout: ${output} stderr: ${stderr}`));
        }
      });
      child.on("error", (error) => {
        done(reject)(error);
      });
      child.stdin.write(JSON.stringify({ id: "smoke", command: "status" }) + "\n");
      // Keep stdin open: the sidecar exits when stdin closes, which would
      // race with reading the response.
    });
    console.log("Resource sniffer smoke test passed.");
  } catch (error) {
    console.warn(`WARNING: resource sniffer smoke test skipped: ${error.message}`);
  }
} catch (error) {
  throw new Error(`Unable to build the resource sniffer. Install Go 1.22+ and retry: ${error.message}`);
}

console.log(`Prepared Node runtime: ${bundledNode}`);
console.log(`Prepared FFmpeg runtime: ${bundledFfmpegRuntime}`);
console.log(`Verified yt-dlp runtime: ${bundledYtDlp}${ytDlpResult.downloaded ? ` (downloaded from ${ytDlpResult.url})` : ""}`);
console.log(`Verified Python ${pythonVersion} + gallery-dl ${pythonResult.version || galleryDlVersion} runtime: ${bundledPythonDir}${pythonResult.downloaded ? " (downloaded and prepared)" : " (cached)"}`);
console.log(`Built local resource sniffer: ${bundledSniffer}`);
