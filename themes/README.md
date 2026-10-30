# Themes

First party themes. A theme is one `theme.json` of token overrides on top of a built-in theme, with no code. The format and the token list are in `docs/themes.md`. Plugins live in `plugins/` and have their own catalog.

| Theme | What it looks like |
| --- | --- |
| `midnight` | Deep blue dark theme with the violet accent |
| `amoled-violet` | True black, violet accent, rounder corners, Montserrat |
| `angelcore` | Near-black duotone, square corners, thin rules, mono type |

## Layout

```
themes/<id>/theme.json
themes/<id>/theme.toml
themes/<id>/README.md
```

`theme.toml` holds `id`, `name`, `version`, `author` and `description`. The `id` matches the folder name and `[a-z0-9-]{1,64}`.

## Add a theme

1. Make a folder `themes/<id>/` with `theme.json` and `theme.toml`.
2. Open a pull request. Bump `version` in `theme.toml` for every later change to `theme.json`.
3. A maintainer regenerates and signs the catalog after merging.

## Catalog

`catalog.json` lists every theme with the sha256 and length of its `theme.json`. `catalog.json.sig` is a base64 ed25519 signature over the exact bytes of `catalog.json`, made with the same key as `plugins/catalog.json.sig`.

```
cargo xtask theme catalog --dir themes --out themes/catalog.json --base-url https://raw.githubusercontent.com/Master0fFate/oxidcord/main/themes
cargo xtask theme sign themes/catalog.json <catalog key file>
```

Commit `catalog.json`, `catalog.json.sig` and the theme folders together. The private key never goes into the repository.
