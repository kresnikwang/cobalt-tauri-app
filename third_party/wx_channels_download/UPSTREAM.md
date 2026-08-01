# wx_channels_download attribution

Cobalt's WeChat Channels compatibility work references
[ltaoo/wx_channels_download](https://github.com/ltaoo/wx_channels_download),
pinned during development to commit
`3551436de39ae32dec86e4d064977a6684e429e3`.

Referenced behavior includes the WeChat bridge method names used to obtain feed
metadata, the 64-bit numeric `decodeKey`, and ISAAC64 decryption of the first
131072 bytes of a Finder video response. Cobalt does not copy the upstream
SunnyRoot certificate or private key, does not inject an in-WeChat download UI,
and does not use wildcard HTTPS interception.

Upstream is licensed under the MIT License with the Commons Clause License
Condition v1.0. The complete upstream license is included beside this file.
