# Packaging the native apps

How ThreadLiner, PageLiner and NoteLiner (the pure-Rust egui apps in
`apps/*/native`) are packaged for Linux, Windows and macOS, and how releases are
built in CI.

## Package identity

Per app, the packaged artifacts are named as follows:

| | Value |
|---|---|
| Cargo package | `<app>-native` (so `cargo run -p <app>-native` works) |
| Installed binary | `<app>` (`[[bin]]` in `apps/<app>/native/Cargo.toml`), installed as `/usr/bin/<app>` |
| App id (Wayland app id / X11 WM class, bundle id) | `com.threadline.app`, `com.pageliner.app`, `com.nathanlaan.noteliner` |
| Linux package | `threadliner`, `pageliner`, `noteliner` |
| Desktop entry | `<app id>.desktop`, named "ThreadLiner" etc., `StartupWMClass=<app id>` |

For users upgrading from older Marina builds, each native package **conflicts
with, replaces and provides** the former Tauri packages `thread-liner`,
`page-liner` and `note-liner` (Debian: `Conflicts`/`Replaces`/`Provides`; RPM:
`Conflicts`/`Obsoletes`/`Provides`), so installing one removes the older build.
The former Electron packages were already named `threadliner` etc., so the
native package upgrades them in place.

- **Settings and data** live in the platform config/data folders
  (`marina_core::paths`) shared by every build Marina has shipped, so projects
  open unchanged.
- **eframe's own state** (theme, window, panes: `app.ron`) lives under
  `~/.local/share/<app>/`; each `main.rs` points `persistence_path` there via
  `STORAGE_ID`.
- **NoteLiner's MCP bridge** is `noteliner --mcp-bridge`; the Settings snippet
  uses the running executable's path, so it is always correct.

## Formats

| Format | Platform | Tool | Output (`target/packages/<app>/`) |
|---|---|---|---|
| `.deb` | Linux | `cargo-deb` (`[package.metadata.deb]`) | `<app>_<ver>-1_amd64.deb` |
| `.rpm` | Linux | `cargo-generate-rpm` (`[package.metadata.generate-rpm]`) | `<app>-<ver>-1.x86_64.rpm` |
| AppImage | Linux | `cargo-packager` (`[package.metadata.packager]`) | `<app>_<ver>_x86_64.AppImage` |
| Flatpak | Linux | `flatpak-builder` (`packaging/flatpak/<app id>.yml`) | a repo / bundle |
| NSIS installer | Windows | `cargo-packager` | `<app>_<ver>_x64-setup.exe` |
| `.app` + `.dmg` | macOS | `cargo-packager` | `<App>.app`, `<app>_<ver>_<arch>.dmg` |

Why these tools: `cargo-deb` and `cargo-generate-rpm` give full control of the
package metadata (dependencies, `Conflicts`/`Replaces`); `cargo-packager`
(from the Tauri bundler's authors) covers AppImage, NSIS and macOS bundles
with one config and downloads linuxdeploy/NSIS itself, so there's one
non-Linux tool to learn. Its own `.deb` isn't used.

### Sizes (Linux x86_64, release with LTO, measured 2026-10-18, v2.2.5)

| App | Binary | `.deb` | `.rpm` | AppImage |
|---|---|---|---|---|
| ThreadLiner | 27 MB | 7.9 MB | 8.5 MB | 12 MB |
| PageLiner | 32 MB | 9.5 MB | 11 MB | 14 MB |
| NoteLiner | 60 MB | 18 MB | 19 MB | 24 MB |

NoteLiner is the largest because of in-process Typst for PDF export
(`noteliner-export`, about 34 MB of the binary).

## Versions

Every app shares one version, set with:

```bash
scripts/set-version.sh 2.3.0        # or 2.3.0-rc.1
```

It updates each app's `native/Cargo.toml` `[package] version` and adds a dated
`<release>` to each `packaging/linux/*.metainfo.xml`, then refreshes `Cargo.lock`
(workspace entries only; `NO_LOCKS=1` skips this). Shared crates under `crates/`
keep their own versions. Package versions come from Cargo, so build the packages
after setting the version.

## Building locally

```bash
cargo install --locked cargo-deb cargo-generate-rpm cargo-packager
scripts/package-native.sh                         # all apps, the host's formats
scripts/package-native.sh noteliner               # one app
FORMATS="deb appimage" scripts/package-native.sh  # chosen formats
```

The script runs `cargo run -p xtask -- icons` (the hicolor PNGs and the
Windows `.ico`, from `apps/<app>/assets/icon.png`, into the git-ignored
`packaging/generated/`), builds the release binaries, then each format. Host
defaults: `deb rpm appimage` on Linux, `nsis` on Windows (run it from Git
Bash), `app dmg` on macOS.

On a machine without FUSE (containers, CI), set `APPIMAGE_EXTRACT_AND_RUN=1`
for the AppImage step: linuxdeploy is itself an AppImage.

### Shared files

| Path | What |
|---|---|
| `packaging/linux/<app id>.desktop` | Desktop entries for .deb/.rpm/Flatpak (validated with `desktop-file-validate`) |
| `packaging/linux/<app id>.appimage.desktop.hbs` | The AppImage's desktop entry (a cargo-packager template, for `StartupWMClass`) |
| `packaging/linux/<app id>.metainfo.xml` | AppStream metadata (validated with `appstreamcli validate`) |
| `packaging/flatpak/<app id>.yml` | Flatpak manifests |
| `xtask/` | `cargo run -p xtask -- icons` |
| `apps/<app>/native/build.rs` | Embeds the icon in the Windows `.exe` (Windows hosts only) |

## Runtime dependencies (Linux)

The binaries link only against glibc; winit, glutin and fontconfig `dlopen`
the rest, so the packages declare them explicitly:

- `git` (sync, history; all three apps shell out to it)
- EGL/GL (`libegl1`, `libgl1`), `libxkbcommon0`, `libfontconfig1`
- Wayland: `libwayland-client0`, `libwayland-egl1`
- X11: `libx11-6`, `libx11-xcb1`, `libxcursor1`, `libxi6`, `libxrender1`,
  `libxkbcommon-x11-0`

The RPMs require the same libraries by soname (`libEGL.so.1()(64bit)` etc.),
so they don't depend on one distribution's package names. Both need the
glibc of the build machine or newer: CI builds on Ubuntu 24.04 (glibc 2.39).
The AppImage bundles no libraries; it expects the same ones on the host.

## Flatpak

**Untested**: there's no `flatpak-builder` on the development machine.

```bash
scripts/flatpak-sources.sh          # → packaging/flatpak/cargo-sources.json (vendored crates)
flatpak-builder --user --install-deps-from=flathub --force-clean \
  build-dir packaging/flatpak/com.nathanlaan.noteliner.yml
```

- Runtime `org.freedesktop.Platform` 25.08 with the `rust-stable` SDK
  extension; the build is offline from `cargo-sources.json`
  (flatpak-cargo-generator), which must be regenerated whenever `Cargo.lock`
  changes.
- **git** is built as a module (the runtime doesn't ship it), pinned to
  git 2.51.0 by sha256.
- **Permissions:** Wayland/X11, DRI, network (feeds, git sync, the update
  check), `--socket=ssh-auth` for git over SSH, notifications (ThreadLiner),
  and `--filesystem=home`. Home access is deliberate: projects and data
  folders are git repositories that are reopened by path from the recent
  lists and handed to `git`, which portal document paths
  (`/run/user/…/doc/…`) would break.
- **Config is separate** inside the sandbox (`~/.var/app/<id>/config`), so a
  Flatpak install doesn't share settings with a .deb/.rpm install. NoteLiner's
  MCP clients must run `flatpak run com.nathanlaan.noteliner --mcp-bridge`.

## Windows and macOS

**Untested locally** (built only by CI on its runners). Both are unsigned.

- **Windows:** a per-user NSIS installer (`installer-mode = "currentUser"`,
  no admin rights) with a Start-menu shortcut and the app icon (the installer
  icon from `packaging/generated/<app>/<app id>.ico`; the `.exe` icon is
  embedded by `build.rs`). The MCP server isn't available on Windows yet
  (it needs a Unix socket).
- **macOS:** `<App>.app` (bundle id = the app id, `Info.plist` and the `.icns`
  generated by cargo-packager from the PNGs) and a `.dmg`; minimum macOS 11.

### Signing (TODO)

| Platform | What's needed | Secrets (GitHub Actions) |
|---|---|---|
| Windows | Authenticode-sign the `.exe` files and the installer (`signtool` or cargo-packager's `windows.signCommand`) | `WINDOWS_CERTIFICATE` (base64 .pfx), `WINDOWS_CERTIFICATE_PASSWORD`; or Azure Trusted Signing credentials |
| macOS | `codesign` with a Developer ID Application certificate, hardened runtime, then notarize and staple (cargo-packager: `macos.signingIdentity`, `macos.notarizationCredentials`) | `APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID`, and either `APPLE_ID` + `APPLE_PASSWORD` (app-specific password) or an App Store Connect API key (`APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_PATH`) |
| Linux | Optional: sign the .deb/.rpm or the repository metadata | a GPG key |

Until then, Windows SmartScreen warns on first run, and macOS needs
right-click → Open (or `xattr -d com.apple.quarantine`).

## CI

- **`.github/workflows/release-native.yml`** runs on `native-v*` tags and by
  hand. It runs a matrix (Ubuntu 24.04 → deb, rpm, AppImage; Windows → NSIS;
  macOS → app, dmg) that runs `scripts/package-native.sh` and uploads the
  packages as artifacts. For a tag it then attaches them to a **draft**
  GitHub release; publishing stays manual.
- **`.github/workflows/build.yml`** (`native` job) lints and tests the native
  crates on all three platforms.
- **No UI snapshot job.** egui_kittest's `snapshot` feature renders with
  wgpu, which needs a software Vulkan driver (lavapipe) on CI and committed
  baseline images per platform; text rasterization differs between Mesa
  versions, so the comparisons would be flaky. The kittest suites already run
  headless in the `native` job (they check widgets through AccessKit, not
  pixels).

## Verified locally (Linux, 2026-10-18)

- Built all nine Linux packages (deb, rpm, AppImage for each app).
- Inspected `dpkg-deb -c`/`-f` and the RPM headers: files, dependencies,
  `Conflicts`/`Replaces`/`Provides` and `Obsoletes` as above.
- `desktop-file-validate` and `appstreamcli validate --no-net` pass for every
  desktop entry and metainfo file (including the AppImage's).
- Each AppImage launches (with `MARINA_SCREENSHOT`, a temporary
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME`).
- Not installed: nothing was installed system-wide.
