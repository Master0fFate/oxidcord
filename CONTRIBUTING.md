# Contributing

Read `AGENTS.md` first. It holds the rules every change follows.

## Setup

Rust 1.99 is pinned by `rust-toolchain.toml`. Windows needs the MSVC toolchain and cmake. Linux needs ALSA, xkbcommon, Wayland, X11, fontconfig, dbus, WebKitGTK 4.1 and GTK 3 development headers. macOS needs Xcode command line tools.

```bash
cargo run -p oxidcord -- --mock
```

## Before opening a pull request

```bash
cargo xtask check-all
cargo xtask headless-run --mock
```

## Pull requests

Keep them small and on one topic. Write the description in plain words: what changed and why. Screenshots for anything visual.

## Plugins

Plugin API changes are additive only. See `docs/plugin-api/`.
