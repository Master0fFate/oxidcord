# Architecture

Oxidcord is a Cargo workspace. Data flows one way: gateway event, state reducer, revision bump, UI re-render of the views whose revision changed. The UI never performs network calls; it sends `Command` values to the session, which owns the REST client and the gateway task.

```
apps/desktop        binary: wiring, window, lifecycle, CLI flags (--mock, --safe-mode)
crates/protocol     typed Discord REST and gateway models, lenient serde decoding
crates/gateway      websocket session: identify, heartbeat, resume, zlib-stream, backoff
crates/rest         HTTP client: rate-limit buckets, 429 retry, Authorization redaction
crates/state        normalized store, permission math, read state, message cache
crates/store        sqlite cache and settings, token in the OS keyring
crates/markdown     Discord markdown parser to AST to layout model
crates/media        image and animation decoding, texture cache
crates/ui           native views and components, theme tokens
crates/platform     tray, notifications, keyring, single instance, hotkeys
crates/plugin-host  WebAssembly component runtime, capability broker, UI slot renderer
crates/plugin-sdk   guest-side SDK for plugin authors
tools/mockcord      mock Discord REST and gateway server driven by fixtures
tools/xtask         headless-run, package, plugin pack, conformance runner
plugins/            first-party example plugins
```

## Threads

- UI thread owns the window and all view entities.
- A tokio runtime on background threads owns the gateway socket, REST client, sqlite writer and plugin workers.
- Crossing: `tokio::sync::mpsc` for commands into the session, a `watch`-style revision counter plus `Arc` snapshots out to the UI. No lock is held across a frame.

## Decoding policy

Every enum that Discord may extend has an `Unknown(i64)` or `Unknown(String)` variant. Collections decode element by element: one bad element is dropped and logged once per kind, never the whole payload. Snowflakes are `u64` behind a newtype that serializes as a string.

## Security boundaries

- Token: `Token` newtype, not `Clone`, not `Debug` printable, zeroized on drop, stored via `keyring`. Plaintext fallback only behind the explicit `storage.plaintext_token` setting.
- Network: HTTPS only. Redirects drop the Authorization header. Response bodies are capped.
- Plugins: see `docs/plugin-api/overview.md`. No WASI, no host handles, every host function gated by a manifest permission the user approved.

## Platform

`crates/platform` has no UI framework dependency. Each module wraps one OS integration and hides the per-OS bodies behind `cfg`; the rest of the workspace only sees the plain API.

- `tray-icon` on Windows needs the UI thread's event loop to pump window messages. The desktop app must create the `Tray` on the UI thread after the event loop exists, and keep it alive for the process lifetime. The `Receiver<TrayEvent>` can be drained from any thread.
- `Hotkeys` has the same rule: create it on the UI thread.
- `Tray` and `Hotkeys` install process-global event handlers, so each can be created once per process.
- `single_instance::acquire` returns `AlreadyRunning` when another instance holds the lock. The second launch should call `notify_existing` with its CLI args (deep link) and exit.
- On Windows, toasts need an AppUserModelID: call `notifications::set_app_user_model_id` at startup and pass the same id as `Notification::app_id`.
