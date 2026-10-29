# Architecture

Oxidcord is a Cargo workspace. Data flows one way: gateway event, state reducer, revision bump, UI re-render of the views whose revision changed. The UI never performs network calls; it sends `Command` values to the session, which owns the REST client and the gateway task.

```
apps/desktop        binary: wiring, window, lifecycle, CLI flags (--mock, --safe-mode)
crates/protocol     typed Discord REST and gateway models, lenient serde decoding
crates/gateway      websocket session: identify, heartbeat, resume, zlib-stream, backoff
crates/rest         HTTP client: rate-limit buckets, 429 retry, Authorization redaction
crates/session      session task: gateway and REST clients, command handling, plugin manager and broker
crates/state        normalized store, permission math, read state, message cache
crates/store        sqlite cache and settings, token in the OS keyring
crates/markdown     Discord markdown parser to AST to layout model
crates/media        image, animation and Lottie sticker decoding, texture cache, ranged fetch, embed provider allowlist, bundled Twemoji pack
crates/player       audio and video playback for chat media: symphonia demux, FFmpeg codec pack, platform decoders
crates/activity     local Rich Presence server (discord-ipc pipes and ws://127.0.0.1:6463-6472), game detection, Spotify listening status
crates/ui           native views and components, theme tokens
crates/platform     tray, notifications, keyring, single instance, hotkeys, idle time, speech, file dialogs, notification sounds
crates/plugin-host  WebAssembly component runtime: manifests, permission grants, hook dispatch, mount, menu and patch stores
crates/plugin-sdk   guest-side SDK for plugin authors (API 1.2, feature `ext` for views and hooks2)
tools/mockcord      mock Discord REST and gateway server driven by fixtures
tools/xtask         headless-run, package, plugin pack, conformance runner, twemoji pack
plugins/            first-party plugins, including the ports of Vencord plugins
```

## Threads

- UI thread owns the window and all view entities.
- A tokio runtime on background threads owns the gateway socket, REST client, sqlite writer and plugin workers.
- Crossing: `tokio::sync::mpsc` for commands into the session, a `watch`-style revision counter plus `Arc` snapshots out to the UI. No lock is held across a frame.

## Decoding policy

Every enum that Discord may extend has an `Unknown(i64)` or `Unknown(String)` variant. Collections decode element by element: one bad element is dropped and logged once per kind, never the whole payload. Snowflakes are `u64` behind a newtype that serializes as a string.

## Security boundaries

- Token: `Token` newtype, not `Clone`, not `Debug` printable, zeroized on drop, stored via `keyring`. Plaintext fallback only behind the explicit `storage.plaintext_token` setting, written with a user-only ACL on Windows and 0600 on unix.
- Network: HTTPS only. Redirects drop the Authorization header. Response bodies are capped.
- Plugins: see `docs/plugin-api/overview.md`. WASI only with an empty, network-free context, no host handles, every host function gated by a manifest permission the user approved.

## Platform

`crates/platform` has no UI framework dependency. Each module wraps one OS integration and hides the per-OS bodies behind `cfg`; the rest of the workspace only sees the plain API.

- `tray-icon` on Windows needs the UI thread's event loop to pump window messages. The desktop app must create the `Tray` on the UI thread after the event loop exists, and keep it alive for the process lifetime. The `Receiver<TrayEvent>` can be drained from any thread.
- `Hotkeys` has the same rule: create it on the UI thread.
- `Tray` and `Hotkeys` install process-global event handlers, so each can be created once per process.
- `single_instance::acquire` returns `AlreadyRunning` when another instance holds the lock. The second launch should call `notify_existing` with its CLI args (deep link) and exit.
- On Windows, toasts need an AppUserModelID: call `notifications::set_app_user_model_id` at startup and pass the same id as `Notification::app_id`.
- `idle::seconds_since_input` reads the time since the last keyboard or mouse input on Windows and macOS and returns `None` elsewhere. It backs the plugin functions `idle-seconds` and the `idle-changed` event.
- `speech::speak_with` reads text aloud through SAPI on Windows and returns `false` elsewhere; `speech::available` tells which. It backs the plugin function `speak`.
- `dialog` shows the native open and save dialogs used by the plugin file functions, and `sound` plays the notification chime at a given volume.

## Activity

`crates/activity` runs the same local RPC server the Discord desktop client exposes, so apps that integrate with Discord find Oxidcord instead. The IPC transport binds the first free `discord-ipc-{0..9}` pipe (`\\.\pipe\` on Windows, `$XDG_RUNTIME_DIR`, `$TMPDIR`, `$TMP`, `$TEMP` or `/tmp` elsewhere); the WebSocket transport binds the first free port in 6463-6472 on 127.0.0.1 and enforces the application's `rpc_origins` when a browser sends an `Origin`. A handshake is validated against `GET /applications/{id}/rpc`, answered with the `READY` dispatch (`cdn_host`, `api_endpoint`, `environment`, the current user), and `SET_ACTIVITY` is validated with Discord's field limits before asset keys are resolved (`/oauth2/applications/{id}/assets` names to ids, `http(s)` URLs through `/applications/{id}/external-assets` to `mp:` paths). Each connection owns its activity; it disappears when the pipe closes.

Game detection fetches `/applications/detectable` into `cache/detectable.json` once a day, keeps only the executables for the running OS and matches process paths every five seconds. Spotify follows the web client: the connection's `access_token` opens `wss://dealer.spotify.com`, the `Spotify-Connection-Id` is registered with `PUT api.spotify.com/v1/me/notifications/player`, and `PLAYER_STATE_CHANGED` events become the `Listening` activity (`sync_id`, `party.id = spotify:<user id>`, `flags = 48`, `assets.large_image = spotify:<hash>`). Expired tokens are refreshed through `/users/@me/connections/spotify/{id}/access-token`.

The engine composes one activity list: Spotify is gated only by the connection's `show_activity`; RPC and detected games are gated by the account's `show_current_game` user setting and the local `settings.activity` toggles. The session appends the list to every presence update it sends.

## Chat media

Videos, audio files and gifv loops stream through `crates/player` with ranged requests over the same host allowlist as images. Video decoding prefers the FFmpeg codec pack (`cargo xtask codecs` installs the pinned, checksummed LGPL build into `target/<profile>/codecs`), then Media Foundation on Windows, then OpenH264 for H.264. On macOS the pack is also looked up in the Homebrew and MacPorts library folders.

Provider embeds (YouTube, Vimeo, Spotify, SoundCloud) are never loaded from the embed payload. `media::providers` rebuilds the player URL from a validated id against a fixed origin list, and the frame is a sandboxed iframe inside a local document with a CSP that only allows those origins. On Windows the frame is a WebView2 popup owned by the main window; on other platforms the play button opens the link in the browser.
