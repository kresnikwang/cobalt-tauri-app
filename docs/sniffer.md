# Local Resource Capture

Resource Capture is an opt-in, local feature for media that has no shareable URL,
including WeChat Channels web playback.

1. Choose **Resource capture** in Cobalt and start it.
2. Install Cobalt's local certificate. It is generated uniquely for this Mac,
   trusted only for SSL in the login keychain, and stored in the app-data
   directory with a private key readable only by the user.
3. Enable the system proxy. Cobalt snapshots the active macOS services first.
4. Fully quit and reopen WeChat after the proxy is active. WeChat may keep the
   proxy configuration from when it was launched.
5. Play a video in the target service. For Video Channels, Cobalt captures the
   signed media URL and its short-lived decode key from the page's local request.
6. Select the captured item and download it. Cobalt decrypts only the required
   prefix of the saved Video Channels file before marking the task complete.

The sidecar listens only on `127.0.0.1`. HTTPS inspection is limited to the
explicit platform allowlist. Cobalt does not send media URLs, cookies, request
headers, proxy credentials, or decode keys to the frontend or a remote service.

## Recovery

Stopping capture restores the saved system proxy configuration. If Cobalt exits
unexpectedly, it attempts restoration at its next startup. If macOS remains
configured with `127.0.0.1:8899`, open Cobalt and choose **Restore previous
proxy**, or disable HTTP and HTTPS proxies for the active network service in
macOS Network settings.

Capture may not work in applications that use certificate pinning. Use only for
content you are permitted to save.

## WeChat Channels diagnostics

- **Waiting for Video Channels page** means WeChat has not loaded the injected
  same-origin hook. Confirm the proxy is active, then fully restart WeChat.
- **Page matched, but no capture** means the hook loaded but no media metadata
  was returned. Open the video detail view and let it play for several seconds.
- Captured Finder video streams are intentionally hidden until their decode key
  is available; downloading the encrypted CDN response directly produces an
  unusable file.
