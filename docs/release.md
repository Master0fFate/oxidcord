# Releasing

## Update signing key

The client only installs an update if `manifest.json` carries a valid ed25519 signature (`manifest.json.sig`, hex encoded, over the raw manifest bytes) made with the key whose public half is compiled into the client as `UPDATE_PUBLIC_KEY` in `crates/updater/src/keys.rs`.

The value committed in the repository is a placeholder: its private half was thrown away, so no release can be signed for it. Before the first public release replace it with your own key.

1. Run `cargo xtask release keygen update-signing.key`. It writes the base64 secret key to the file (created with owner-only permissions on Unix) and prints a ready-made `pub const UPDATE_PUBLIC_KEY: [u8; 32] = [...]` line.
2. Paste that line over the constant in `crates/updater/src/keys.rs` and commit it.
3. Store the file contents as the repository secret `UPDATE_SIGNING_KEY`, then delete the file. Keep an offline backup. Losing the key means every installed client needs a manual reinstall, leaking it means an attacker can ship updates.

Rotating the key requires shipping a release signed with the old key that contains the new public key.

## Cutting a release

1. Bump `version` in `[workspace.package]` and commit.
2. Tag the commit `vX.Y.Z` (`vX.Y.Z-beta.1` for a prerelease) and push the tag.
3. `.github/workflows/release.yml` checks the tag against the crate version, builds every package, writes and signs `manifest.json`, and creates the GitHub release with the commit subjects since the previous tag as notes.

Prereleases are skipped by `releases/latest`, so only clients on the beta channel or with prereleases allowed see them.

## Repository configuration

- Secret `UPDATE_SIGNING_KEY`: base64 ed25519 seed from `keygen`.
- Variables `LINUXDEPLOY_SHA256` and `APPIMAGETOOL_SHA256`: sha256 of the `linuxdeploy` and `appimagetool` AppImages the Linux job downloads. The job refuses to run without them.
- Optional secrets `MACOS_CERTIFICATE` (base64 Developer ID `.p12`), `MACOS_CERTIFICATE_PASSWORD` and `MACOS_SIGN_IDENTITY`. Without them the dmg is unsigned.

## Asset names

`oxidcord-<version>-<target>-<kind>.<ext>` where target is one of `windows-x86_64`, `macos-aarch64`, `macos-x86_64`, `linux-x86_64`, `linux-aarch64` and kind is `installer` (`exe`), `portable` (`zip`), `appimage` (`AppImage`), `dmg`, `deb`, `rpm`. `cargo xtask release manifest` ignores files that do not match.

## Manifest

```json
{
  "version": "0.2.0",
  "published_at": "2026-10-22T12:00:00Z",
  "notes_url": "https://github.com/owner/name/releases/tag/v0.2.0",
  "artifacts": [
    {
      "target": "windows-x86_64",
      "kind": "installer",
      "url": "https://github.com/owner/name/releases/download/v0.2.0/oxidcord-0.2.0-windows-x86_64-installer.exe",
      "sha256": "64 hex digits",
      "length": 12345678
    }
  ]
}
```

## Client behaviour

- The update check uses a credential-free client, follows redirects only to `github.com`, `objects.githubusercontent.com` and `release-assets.githubusercontent.com`, caps the API body at 1 MB and the manifest at 1 MB.
- The manifest signature is verified before anything in it is trusted. An update is offered only when its version is higher than the running one.
- The package kind matches the running install: NSIS installer under `%LOCALAPPDATA%\Programs\Oxidcord`, portable zip elsewhere on Windows, dmg inside an `.app` bundle on macOS, AppImage when `APPIMAGE` is set on Linux. Anything else (deb, rpm, Flatpak) is managed by the package manager and only gets a notification.
- The download is streamed to `updates/<version>/<name>.part` under the data directory, length-capped, then its length and sha256 are checked before the rename and again right before install.
- Portable and AppImage and dmg installs keep the previous files as `<name>.old` and record a trial in `updates/trial.json`. The first start of the new version marks it started; `mark_start_ok` after the window opens removes the backup. If a started version never confirms, the next start restores the `.old` files.
