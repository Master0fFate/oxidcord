# First-party plugins

Every plugin here is a workspace member, builds with `cargo build -p <name> --target wasm32-wasip2 --release` and passes the conformance suite. The API is described in `docs/plugin-api/`.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `hello-timestamps` | Relative send time under each message, Copy ID button | `message-render`, `ui-slots`, `actions-clipboard` |
| `link-cleaner` | Strips tracking parameters from links before sending | `message-send` |
| `spoiler-all` | Wraps every message in a spoiler, toggled from the composer toolbar | `message-render`, `ui-slots`, `storage` |
| `keyword-highlighter` | Bolds keywords, adds a keyword line, notifies on a match | `message-render`, `storage`, `context-basic`, `actions-notify` |
| `quick-replies` | Snippet popover in the composer and `/snip <name>` | `ui-slots`, `storage`, `actions-send`, `context-basic` |
| `text-shortcuts` | Expands `:shrug:` style emoticons and your own `;shortcuts` before sending | `message-send`, `storage`, `ui-slots` |
| `delete-keeper` | Keeps and shows the text of deleted messages | `context-timeline`, `storage`, `ui-slots` |
| `edit-history` | Keeps earlier versions of edited messages and shows them | `context-timeline`, `storage`, `message-render` |
| `user-notes` | Private notes on users, shown as a badge | `ui-slots`, `storage`, `user-display` |
| `local-status-badge` | Status bar item with the channel id and a color cycling badge | `context-basic`, `ui-slots` |
| `word-mute` | Hides messages containing words or phrases you muted | `message-filter`, `storage`, `ui-slots`, `context-basic` |
| `theme-midnight` | Theme-only plugin, no wasm | none |
| `theme-amoled-violet` | Theme-only plugin: true black, violet accent, rounder corners | none |
| `conformance-probe` | Test fixture the conformance runner pushes to its limits, not for users | none |

## Commands

Run them from the repository root.

```
cargo xtask plugin new <id> [--dir plugins]
cargo xtask plugin build <path> [--release]
cargo xtask plugin pack <path> [--out dist/plugins] [--sign <secret key file>]
cargo xtask plugin conformance [--dir plugins]
cargo xtask plugin catalog [--dir plugins] [--out plugins/catalog.json] [--packed dist/plugins] [--base-url <url>]
cargo xtask plugin keygen <out>
cargo xtask plugin sign-catalog <catalog.json> <secret key file>
```

- `new` scaffolds `Cargo.toml`, `plugin.toml`, `src/lib.rs` and `README.md` in `<dir>/<last part of the id>`.
- `build` builds the crate for `wasm32-wasip2` and prints the path of the `.wasm`. A directory without a `Cargo.toml` (a theme) has nothing to build.
- `pack` builds in release mode and writes `<id>-<version>.oxidcord-plugin`. With `--sign` the archive carries a publisher signature.
- `conformance` packs every plugin in the directory into a temporary directory, installs the archives there and runs `plugin_host::conformance::run` over the result. It prints one row per plugin and exits non-zero when a check fails.
- `catalog` packs every plugin (except `conformance-probe`) into `--packed` and writes the catalog with the sha256 and length of each archive. `--base-url` is put in front of the archive name to form each `url`, without it the url is the file name. Entries start as `reviewed: false`.
- `keygen` writes a base64 ed25519 secret key (mode 0600 on unix, refuses to overwrite) and prints the base64 public key.
- `sign-catalog` writes `<catalog.json>.sig` next to the catalog: a base64 ed25519 signature over the exact bytes of the file. Hosts check it with `plugin_host::catalog::verify_catalog`, so change the catalog and the signature is void.
