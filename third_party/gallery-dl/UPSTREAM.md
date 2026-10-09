# gallery-dl attribution

The app bundles [mikf/gallery-dl](https://github.com/mikf/gallery-dl) as the
image/gallery download engine, pinned to **1.32.16**
(`GALLERY_DL_VERSION` in `.github/workflows/*.yml` and
`scripts/prepare-release-bundle.mjs`).

Upstream is Copyright Mike Fährmann and contributors, licensed under
**GNU GPL v2.0 only** ("GPL-2.0-only"); the full text is included beside this
file as `LICENSE` (taken from the pinned wheel's
`gallery_dl-1.32.16.dist-info/licenses/LICENSE`).

gallery-dl is run unmodified as a **separate, independent sidecar process**
(`binaries/python/bin/python3.12 -I -m gallery_dl ...`), communicating only via
CLI arguments, stdout/stderr and the filesystem — it is neither linked into nor
compiled with the application. No gallery-dl source is modified; if it ever is,
the corresponding modified source must be made available per GPL-2.0.

During `prepare-release-bundle.mjs` this `LICENSE` is copied into the bundle at
`binaries/python/licenses/gallery-dl.GPL-2.0-only.txt`.
