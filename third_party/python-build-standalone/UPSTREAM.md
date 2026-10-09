# python-build-standalone attribution

Because gallery-dl has no official macOS binary, the app bundles a relocatable,
statically-linked CPython from
[astral-sh/python-build-standalone](https://github.com/astral-sh/python-build-standalone),
pinned to release tag **20261003**, CPython **3.12.15**
(`PYTHON_BUILD_TAG` / `PYTHON_VERSION` in `.github/workflows/*.yml` and
`scripts/prepare-release-bundle.mjs`). The exact asset is
`cpython-3.12.15+20261003-aarch64-apple-darwin-install_only_stripped.tar.gz`,
verified against the release `SHA256SUMS` at build time.

Licenses:

- CPython / standard library — **Python Software Foundation (PSF) License**;
  the interpreter build ships its own text at `lib/python3.12/LICENSE.txt`.
- python-build-standalone build tooling/repository — **MPL-2.0**; included here
  as `LICENSE` (the repository root `LICENSE` at tag 20261003).
- Statically-linked third-party components — their own licenses. The
  `install_only_stripped` tarball does **not** ship these texts, so upstream's
  curated collection is vendored under `licenses/` (filenames
  `LICENSE.<component>.txt`, indexed by upstream's `python-licenses.rst`), all
  fetched from the repository at tag 20261003. Components include OpenSSL 3
  (Apache-2.0), zlib, bzip2, liblzma/xz, libffi, SQLite, expat, mpdecimal,
  libedit/ncurses and others; Linux-only X11 and Tcl/Tk texts are kept for
  parity with upstream's curated set even though the macOS build prunes Tcl/Tk.

During `prepare-release-bundle.mjs` these texts are copied into the bundle under
`binaries/python/licenses/` (`python-build-standalone.LICENSE.txt` and
`components/`).
