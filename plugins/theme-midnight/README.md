# Midnight

A theme-only plugin: the manifest has no `entry`, so there is no wasm and nothing runs. The package ships a `theme.json`.

`theme.json` is a flat object of token overrides. `base` picks the built-in theme to start from (`dark`, `light` or `amoled`), every other key is a token from `crates/ui/src/theme.rs`: colors as `#rrggbb`, `#rrggbbaa` or `rgb()`/`rgba()`, sizes as numbers, `ui_fonts` and `code_fonts` as lists. Unknown or malformed tokens are ignored.

`Package::install_into` writes `theme.json` next to `plugin.toml` in `<plugins_dir>/<id>/<version>/`. The file uses the same format as `<themes_dir>/<name>.json`, which the UI loads with `Theme::from_file`. `theme.json` is part of the content hash that signatures cover.

Permissions: none.
