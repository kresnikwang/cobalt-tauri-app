# Third-party notices

## res-downloader

Resource sniffing design research references
[putyy/res-downloader](https://github.com/putyy/res-downloader), Apache License
2.0. See `third_party/res-downloader/UPSTREAM.md` for the pinned source revision
and the security differences in Cobalt's implementation.

## wx_channels_download

WeChat Channels bridge-method research and ISAAC64 compatibility behavior were
adapted with reference to
[ltaoo/wx_channels_download](https://github.com/ltaoo/wx_channels_download),
pinned to commit `3551436de39ae32dec86e4d064977a6684e429e3`.

The upstream license is MIT with the Commons Clause License Condition v1.0.
See `third_party/wx_channels_download/UPSTREAM.md` and `LICENSE`. The condition
does not grant the right to sell software whose value derives substantially
from this functionality.

## gallery-dl (bundled image/gallery engine)

The app bundles [gallery-dl](https://github.com/mikf/gallery-dl) (pinned
version, see `GALLERY_DL_VERSION` in `.github/workflows/*.yml` and
`scripts/prepare-release-bundle.mjs`). gallery-dl is Copyright Mike Fährmann
and contributors, licensed under **GNU GPL v2.0 only** ("GPL-2.0-only").

It is executed as a **separate, independent sidecar process**
(`binaries/python/bin/python3.12 -I -m gallery_dl ...`) communicating only via
command-line arguments, stdout/stderr and the filesystem — it is not linked
into or compiled with the Cobalt application. The complete corresponding
source and the GPL-2.0 license text are available from the upstream project.
The license text is shipped inside the bundle at a stable path
`binaries/python/licenses/gallery-dl.GPL-2.0-only.txt`, and also at the
wheel-provided `binaries/python/lib/python3.12/site-packages/gallery_dl-<ver>.dist-info/licenses/LICENSE`;
a canonical copy is kept in the repo at `third_party/gallery-dl/LICENSE`. If you
modify gallery-dl itself, the GPL obligations apply to that modified program.

## Bundled CPython runtime

gallery-dl has no official macOS binary, so the app bundles a relocatable
CPython from [astral-sh/python-build-standalone](https://github.com/astral-sh/python-build-standalone)
(pinned via `PYTHON_BUILD_TAG` / `PYTHON_VERSION`). The interpreter and
standard library are under the **Python Software Foundation (PSF) License**
(shipped at `binaries/python/lib/python3.12/LICENSE.txt`). The
python-build-standalone **build tooling/repository** is licensed under MPL-2.0
(shipped at `binaries/python/licenses/python-build-standalone.LICENSE.txt`).

The statically-linked CPython runtime incorporates third-party components under
their own licenses — notably **OpenSSL 3** (Apache-2.0), plus zlib, bzip2,
liblzma (xz), libffi, SQLite, expat, mpdecimal, libedit/ncurses and others.
The `install_only_stripped` tarball upstream publishes omits these component
license texts, so the exact, curated license collection from the pinned
python-build-standalone tag is vendored in the repo
(`third_party/python-build-standalone/licenses/`, with upstream's own index
`python-licenses.rst`) and copied into the bundle at
**`binaries/python/licenses/components/`** during `prepare-release-bundle.mjs`.

Python packages installed alongside gallery-dl (all under
`binaries/python/lib/python3.12/site-packages/`). Modern wheels ship their
license under the package's `*.dist-info/licenses/` directory:

| Package | License |
|---|---|
| gallery-dl | GPL-2.0-only |
| requests | Apache-2.0 |
| urllib3 | MIT |
| certifi (Mozilla CA bundle) | MPL-2.0 |
| idna | BSD-3-Clause |
| charset-normalizer | MIT |
