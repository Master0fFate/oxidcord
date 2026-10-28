# Building and packaging plugins

## Manifest

Every plugin has a `plugin.toml` next to its wasm file.

```toml
id = "dev.example.my-plugin"
name = "My Plugin"
version = "0.1.0"
api = "^1.1"
authors = ["Example"]
license = "MIT"
description = "What it does."
permissions = ["message-render", "storage"]
network_hosts = []
image_hosts = []
entry = "my_plugin.wasm"
min_host = "0.1.0"
priority = 0
```

| Key | Meaning |
| --- | --- |
| `id` | Reverse DNS name, 3 to 128 characters of `[a-z0-9.-]`. Also the install directory name |
| `name`, `description`, `authors`, `license` | Shown in the plugin manager |
| `version` | Semver |
| `api` | Semver requirement on the plugin API. The host implements 1.2.0, so `^1.0`, `^1.1` and `^1.2` all load. Use `^1.1` when the plugin uses the `ui`, `client` or `hooks` interfaces and `^1.2` for `hooks2` |
| `permissions` | Either bare names or tables with a reason: `[[permissions]]` with `permission = "storage"` and `reason = "..."`. The consent dialog shows the reason |
| `network_hosts` | Lowercase hostnames `fetch` may reach. Needs the `net-fetch` permission, and the user must approve each host. Discord hosts (`discord.com`, `discordapp.com`, `discord.gg`, `discord.media`, `discordapp.net` and their subdomains) are rejected |
| `hooks` | Names of the `hooks2` hooks the plugin implements: `draft-change`, `autocomplete-emoji`, `decorate-channel`, `notification`, `message-click`, `channel-click`, `link-click`, `display-user`, `before-send`, `intercept`. The host never calls an undeclared hook. Optional |
| `url_schemes` | App URI schemes `open-url` may use, picked from `spotify`, `steam`, `tidal`, `itms-apps` and `music`. Needs the `actions-open-url` permission, and the user must approve each one. Any other scheme is rejected when the manifest loads. Optional |
| `image_hosts` | Lowercase hostnames 1.1 views and patches may load images from, besides the Discord CDN. The user must approve each host |
| `entry` | Relative path to the component inside the package, ending in `.wasm`, no `..`. Optional: a plugin without one ships only a `theme.json`. First party themes do not use this, they ship through the themes catalog (`docs/themes.md`) |
| `min_host` | Oldest host version the plugin runs on. Optional |
| `priority` | Order in the render pipeline, lower runs first, ties by id. Optional, default 0 |
| `settings_schema` | A JSON schema as a TOML table. Optional |

A plugin whose `api` requirement does not match, or whose `min_host` is newer than the host, is listed as incompatible and never runs.

## Build

```
cargo build -p my-plugin --target wasm32-wasip2 --release
```

The `wasm32-wasip2` target links the crate straight into a WebAssembly component (`wasm-component-ld` ships with the Rust toolchain), so no extra componentization step is needed. The SDK also builds for the host target so a workspace-wide `cargo build` still works, but only the `wasm32-wasip2` output is a loadable plugin.

The output is `target/wasm32-wasip2/release/<crate_name>.wasm`. Copy it next to `plugin.toml` under the name given by `entry`, or point `Package::from_files` at it.

Inside this workspace the SDK is a workspace dependency named `plugin-sdk`, so first-party plugins write `use plugin_sdk::...`. Outside it, depend on the `oxidcord-plugin-sdk` package and write `use oxidcord_plugin_sdk::...`.

The SDK has one feature, `ext`. Without it the bindings cover the 1.0 `plugin` world and `export_plugin!(T)` produces a component that also loads on 1.0 hosts. With it the bindings cover the `plugin-ext` world: the `view`, `mount`, `menu`, `patch`, `timer` and `client` modules and the `PluginExt` trait become available and `export_plugin!(T, ext)` exports the 1.1 hooks. A plugin built with the feature but exported with plain `export_plugin!(T)` exports no-op hooks.

```toml
[dependencies]
plugin-sdk = { workspace = true, features = ["ext"] }
```

## Install layout

Installed plugins live in `<plugins_dir>/<id>/<version>/` and contain `plugin.toml`, the wasm file, an optional `theme.json`, an optional `assets/` directory and an optional `signature` file. `PluginManager::load_all` loads the highest version directory of each id. A directory that holds a `plugin.toml` directly (`<plugins_dir>/<name>/plugin.toml`) is loaded as is, which is handy during development.

## Pack

A `.oxidcord-plugin` file is a zip archive with `plugin.toml`, the entry wasm, optional `theme.json`, optional `assets/` and optional `signature` at its root.

```rust
let package = plugin_host::Package::from_files(manifest_path, Some(wasm_path))?;
package.pack(Path::new("my-plugin.oxidcord-plugin"))?;
```

`Package::open` reads an archive and rejects entries with `..`, absolute paths or symlinks, and archives whose declared or actual uncompressed size exceeds 64 MB. `Package::install_into(plugins_dir)` extracts to `<plugins_dir>/<id>/<version>/`, staging first and renaming into place.

## Signing and trust

The signature is an ed25519 signature over the sha256 of the manifest bytes followed by the wasm bytes and the `theme.json` bytes when the package has them. The `signature` file is either the 64 signature bytes (catalog signature) or the 32 byte public key followed by the 64 signature bytes (publisher signature, made by `Package::sign`).

`Package::verify(catalog_key)` returns `Trust::Catalog` when the catalog key verifies the signature, `Trust::Signed` when the embedded key does, and `Trust::Unsigned` otherwise. A publisher signature proves the archive was not modified after signing, not that the publisher is who they claim.

## Grants

The user's decision is stored per plugin id under `plugins.grants.<id>` in the store key-value table: the granted permissions, the approved network hosts, the approved image hosts and the enabled flag. `Command::PluginGrant` takes one host list and splits it between `network_hosts` and `image_hosts` by the manifest. `Grants::requires_reprompt(&manifest)` is true when a manifest asks for a permission or host that was not granted yet, for example after an update. Revoking takes effect on the next host call, no restart needed.

## Hook timeouts

Each hook runs against an epoch deadline. The engine increments its epoch every 10 ms, and a call is given `ceil(timeout / 10 ms) + 1` ticks, so a hook is killed between its timeout and roughly one tick after it. The defaults are `init` 5 s, `on-event` 200 ms, `render-message` 50 ms, `before-send` 500 ms, `filter-message` 50 ms, `display-user` 50 ms, `on-ui-action` 2 s, `on-command` 2 s, `settings-schema` 500 ms, `render-view` 200 ms, `on-view-event` and `on-menu-item` 2 s, `intercept-message` 50 ms, `on-timer` 200 ms and `on-emoji-pick` 200 ms. They are fields of `HookTimeouts` in `RuntimeConfig`.

The epoch only interrupts guest code. While the guest waits inside a host call (a `fetch` can take up to 10 s) the interrupt fires when the call returns. The caller of a hook therefore also waits at most `timeout + 100 ms`; past that it gives up, skips the plugin, and if the plugin had already started the call the instance is abandoned, counted as a crash and replaced after the backoff.

A pipeline hook that times out or traps is skipped and the chain continues with the previous output. Callers of `render_message`, `before_send`, `filter_message` and `display_user` block for up to the sum of the deadlines of the plugins involved, so call them from a worker thread, never from the UI thread.

## Running the conformance suite

```rust
let report = plugin_host::conformance::run(Path::new("plugins"));
println!("{report}");
assert!(report.passed());
```

It creates a temporary store, grants each plugin its manifest permissions, and prints per check timings. See the overview for what it checks.

## Tooling

`cargo xtask plugin new|build|pack|conformance|catalog|keygen|sign-catalog` wraps the steps above, see `plugins/README.md`.

## Catalog

A catalog is a JSON file `{ "version": 1, "generated_at": ..., "plugins": [...] }`, where each entry has `id`, `name`, `version`, `description`, `permissions`, `url`, `sha256` and `length` of the archive, and `reviewed`. `plugin_host::catalog::verify_catalog(bytes, signature, &key)` checks the catalog bytes against the contents of the `.sig` file next to it (a base64 ed25519 signature over the exact file bytes) and returns the parsed catalog. Entries that fail to decode are dropped, an unsupported `version` is an error.

The client checks catalogs against `CATALOG_PUBLIC_KEY` in `crates/session/src/catalog.rs`. The committed value is an all-zero placeholder that never verifies, so `CatalogRefresh` fails until the release key from `cargo xtask plugin keygen` is pasted there. `CatalogRefresh { url }` downloads `url` and `url.sig` over HTTPS with a credential-free client, `CatalogInstall { id }` downloads the entry's archive, checks its length and sha256 against the signed entry and then installs it like a local file. The UI asks for consent before sending either command.

## Running inside the session

The session owns the `PluginManager`. It loads `store.paths.plugins_dir()` on a blocking task right after start, honours safe mode, and forwards message, reaction, typing, channel switch and connection events. Hooks never run on the session task:

- `before-send` runs on a blocking task before the REST call. A cancel marks the pending message as failed with the plugin's reason, a rewrite sends the new content and marks the message as altered by that plugin. Messages sent by a plugin through `send-message` skip `before-send`.
- Every message that enters the cache is parsed immediately and stored as a `RenderedMessage`. When a plugin holds `message-render` or `message-filter`, a single `plugin-render` thread then runs the render and filter hooks, the selected channel first and the newest message first, and publishes finished batches about every 30 ms. Until a result arrives the parsed message is shown, or hidden when a filter plugin is active. A message a filter hook rejected is left out of the chat.
- When a plugin holds `message-intercept`, the same thread runs `intercept-message` after the render and filter hooks and stores the patches in the plugin host's `Patches` store, which the message rows read. See `interception.md`.
- View events, menu items, contextual `render-view` calls and emoji picks run on the plugin job queue, never on the session task or the UI thread. View events and menu items bump the render epoch like `on-ui-action` does.
- `display-user` overrides are computed in the background for the authors of the visible channel and the member list, with a 2 s debounce after changes, and read by the message and member rows.
- Enabling, disabling, reloading, installing, uninstalling or granting a plugin bumps a render epoch. Cached messages with an older epoch are rendered again, the visible channel at once and other channels when they are opened.
- A plugin package with a `theme.json` is copied to `<themes_dir>/<id>.json` while the plugin is enabled.
