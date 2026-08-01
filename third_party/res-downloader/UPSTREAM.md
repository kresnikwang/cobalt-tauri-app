# res-downloader attribution

This feature's architecture and platform research were informed by
[putyy/res-downloader](https://github.com/putyy/res-downloader), pinned during
development to commit `45d5e9eddc6c2b0baebc41cd5edced32ec3c67b8`.

Upstream is licensed under Apache-2.0. Cobalt intentionally does not reuse the
upstream's fixed certificate/private key or its wildcard HTTPS interception
configuration. The Cobalt sidecar has its own minimal implementation and only
captures an explicit allowlist of media domains.
