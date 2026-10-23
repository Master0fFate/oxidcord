# Publishing plugins and themes

## For users: the catalog

Settings, Plugins has a catalog section with a Catalog URL field and a Refresh button. It lists the plugins and themes of a catalog. The default catalog is the one in this repository (`plugins/catalog.json`, signed by `plugins/catalog.json.sig`). You can point the field at any other catalog URL.

- Refreshing asks for consent first, then downloads the catalog and its `.sig` file over HTTPS with a client that sends no cookies and no account data.
- The client verifies the signature against the catalog key compiled into it. A catalog with a missing or wrong signature is rejected as a whole.
- Installing downloads the archive, checks its length and sha256 against the signed entry, shows the permissions the plugin asks for with the author's reason for each, and only installs after you confirm. Nothing runs until you grant permissions.
- `reviewed` on an entry means the maintainers read the source for that version. It is a note, not a guarantee. The permission list is the real information.

## For authors: ship your own plugin

1. Scaffold: `cargo xtask plugin new dev.example.my-plugin`. Themes are a folder with `plugin.toml` and `theme.json`, see `docs/themes.md`.
2. Build and test: `cargo xtask plugin build plugins/my-plugin --release`, then `cargo xtask plugin conformance`.
3. Pack: `cargo xtask plugin pack plugins/my-plugin --sign <your key file>` writes `<id>-<version>.oxidcord-plugin`. Make a key once with `cargo xtask plugin keygen <file>` and keep the file private.
4. Share the archive directly (users install it from Settings, Plugins, Install from file), or host your own catalog.

## Host a catalog

```bash
cargo xtask plugin catalog --dir plugins --packed packs --base-url https://example.com/packs
cargo xtask plugin sign-catalog plugins/catalog.json <catalog key file>
```

Upload `catalog.json`, `catalog.json.sig` and the archives next to each other. Users enter the URL of `catalog.json`.

Clients only accept catalogs signed by the key built into them. A custom catalog therefore needs a build of Oxidcord with your public key in `crates/session/src/catalog.rs`, which is how forks run their own store. The official catalog key stays with the maintainers.

## For maintainers: update the official catalog

1. Bump `version` in the plugin's `plugin.toml` for every change. Clients compare versions and reinstall only newer ones.
2. Rebuild everything and regenerate: `cargo xtask plugin catalog --dir plugins --packed packs --base-url https://raw.githubusercontent.com/Master0fFate/oxidcord/main/packs`.
3. Review the diff of `plugins/catalog.json`: only the plugins that changed should have a new `sha256`. Set `reviewed` to `true` for the ones whose source you read.
4. Sign: `cargo xtask plugin sign-catalog plugins/catalog.json <catalog key file>`.
5. Commit `catalog.json`, `catalog.json.sig` and `packs`, and push to `main`. The catalog section picks it up on the next refresh.

The catalog private key never goes into the repository. Losing it means shipping a new client with a new key, leaking it means anybody can push plugins to every user, so treat it like a release signing key.
