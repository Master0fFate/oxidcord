# Packaging

Everything here is driven by `.github/workflows/release.yml` on a `v*` tag. Each script reads `VERSION` (no leading `v`) and writes to `dist/` using the asset naming from `docs/release.md`.

| Path | What it is |
| --- | --- |
| `windows/installer.nsi` | NSIS per-user installer into `%LOCALAPPDATA%\Programs\Oxidcord`, start menu shortcut, uninstaller, `Uninstall` registry keys under HKCU, silent `/S`. `/S /R` relaunches the app after install, which the updater uses. |
| `windows/oxidcord.rc` | Resource template. `apps/desktop/build.rs` fills in the icon path and version and compiles it with `embed-resource`, so the exe carries `assets/icon.ico` and version info. |
| `windows/plugins/x86-unicode/WinShell.dll` | Optional, not committed. When present the installer sets the AppUserModelID `dev.oxidcord.Oxidcord` on the shortcut through `WinShell::SetLnkAUMI`, which Windows needs to attribute toast notifications. Drop the Unicode build of the NSIS WinShell plugin there before building the installer. |
| `linux/oxidcord.desktop`, `linux/dev.oxidcord.Oxidcord.metainfo.xml` | Desktop entry and AppStream metadata shared by deb, rpm, AppImage and Flatpak. |
| `appimage/build.sh` | Fills an AppDir with `linuxdeploy` and packs it with `appimagetool`. |
| `deb/build.sh` | `cargo deb` using `[package.metadata.deb]` in `apps/desktop/Cargo.toml`. |
| `rpm/build.sh` | `cargo generate-rpm` using `[package.metadata.generate-rpm]` in `apps/desktop/Cargo.toml`. |
| `flatpak/dev.oxidcord.Oxidcord.json` | Manifest skeleton, not part of CI. Generate `cargo-sources.json` next to it with `flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json` from the flatpak-builder-tools repository, then `flatpak-builder --user --install build packaging/flatpak/dev.oxidcord.Oxidcord.json`. The login webview needs WebKitGTK 4.1, which the freedesktop runtime does not ship; add it as a module before submitting. |
| `macos/Info.plist`, `macos/entitlements.plist` | Bundle metadata and hardened runtime entitlements: microphone, camera, network client, and JIT memory for the wasm plugin runtime. |
| `macos/bundle.sh` | Builds `Oxidcord.app`, generates `icon.icns` from `assets/icon.png` with `sips` and `iconutil`, signs with `codesign` when `MACOS_SIGN_IDENTITY` is set, and builds the dmg with `hdiutil`. |

Assets live in `assets/`: `icon.png` (256 px) and `icon.ico` (16 to 256 px).

## What CI does

1. `verify` checks the tag matches the `oxidcord` crate version.
2. `windows` builds the MSVC release binary, runs `makensis` and zips the portable build.
3. `macos` builds aarch64 and x86_64, optionally imports the signing certificate, runs `bundle.sh`.
4. `linux` builds, then runs the AppImage, deb and rpm scripts. It needs the `LINUXDEPLOY_SHA256` and `APPIMAGETOOL_SHA256` repository variables.
5. `publish` collects every artifact, runs `cargo xtask release manifest`, `sign-manifest` and `notes`, and creates the GitHub release.
