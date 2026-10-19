# Remaining Work — Native Apps

Status: **Open (2026-10-18).** What's left after the Option C port
([plan-native-rust-egui.md](plan-native-rust-egui.md)) and the editor,
slides and rich-text plan
([plan-editor-slides-richtext.md](plan-editor-slides-richtext.md)). The
planned engineering work is done: the native ThreadLiner, PageLiner and
NoteLiner are the default builds (version 2.2.5).

## Suggested order

1. Commit and push (section 1).
2. Fix whatever the first Windows and macOS CI builds turn up.
3. Single-instance, then MCP on Windows (section 2).
4. The hand checks (section 4), alongside daily use.
5. Code signing and Flatpak (section 3) before a public release.
6. The decisions in section 5, once the native builds have proven
   themselves.

## 1. Commit and release

- [ ] Commit the work since the last commit: the spike removal, the
      editor/slides/rich-text plan and its five phases (about 120 changed
      files).
- [ ] Push. CI's `native` job makes the first Windows and macOS builds;
      nothing has been compiled on either platform yet, so expect fixes.
- [ ] Tag `native-v2.2.5`, or run the `release-native` workflow by hand, to
      get draft packages for Linux, Windows and macOS.

## 2. Features not built yet

| Item | Apps | Notes | Priority |
|---|---|---|---|
| Single-instance: a second launch focuses the running app | All three | The most noticeable gap. A local socket or named pipe per app id, passing on file or project arguments. | High |
| MCP server on Windows | NoteLiner | The server uses a Unix socket and reports "not available" on Windows. Needs a named-pipe transport behind the same runtime file and `--mcp-bridge` protocol. | High |
| List editing with several cursors | NoteLiner (`marina-editor`) | List continuation, Tab and the list buttons act on the primary cursor only. | Medium |
| Enter inside a list at 50k lines (8.8 ms p95) | NoteLiner (`marina-editor`) | List continuation still scans the whole note; scope it to the list. | Low |
| Remove the TextEdit fallback engine | NoteLiner (`marina-editor`) | One release after the IME and screen-reader checks (section 4) pass. Removes `MARINA_EDITOR_ENGINE=textedit`. | Medium |
| Shift+arrow selection in rich text | All three (`marina-richtext`) | Needs a keyboard caret in `RichView`. | Medium |
| More code-fence languages (TypeScript, TOML, Svelte, PowerShell) | NoteLiner | Not in syntect's bundled set; a larger set (e.g. `two-face`) was deferred by decision. | Low |
| Help in its own window | ThreadLiner | Currently a dialog. | Low |
| Sync status after a failed Sync Now | PageLiner | Only the error is shown today. | Low |
| EPUB paging keys while a dialog is open | PageLiner | Arrow keys may still turn pages when the dialog has no text field (untested). | Low |

## 3. Packaging

- [ ] **Code signing:** Windows Authenticode, and Apple codesign plus
      notarization. Needs certificates and GitHub secrets; see
      `docs/packaging.md`.
- [ ] **Flatpak:** build the manifests in `packaging/flatpak/`; they have
      never been run through `flatpak-builder`.
- [ ] **Older Linux distros:** the `.deb` and `.rpm` need glibc 2.39 or
      newer (built on Ubuntu 24.04). Build on an older image if older
      distros matter.

## 4. Hand checks

These need real hardware, desktops or accounts, so they can't be automated
here.

- [ ] **Input methods** in the new editor core: IBus/Fcitx (Wayland and
      X11), Windows TSF and macOS. Check inline composition, candidate
      window position, commit and cancel, and Enter/Backspace while
      composing.
- [ ] **Screen readers** in the editor: Orca, NVDA and VoiceOver. Check that
      the visible lines are read, caret and selection are announced, and
      reading works after scrolling.
- [ ] **A week of daily use** of the native apps on real data. The original
      plan made this the gate before cutover; cutover happened first.
- [ ] **Sync against the real git remote** in all three apps.
- [ ] **MCP:** a client through `noteliner --mcp-bridge`, including a write
      confirmation.
- [ ] **Presenting:** a real two-monitor run with the speaker view on X11,
      Windows and macOS (Swap displays does nothing on Wayland).
- [ ] **Windows, macOS and KDE integration:**
  - [ ] clipboard image paste
  - [ ] file dialogs, and drag-and-drop from the file manager
  - [ ] notifications
  - [ ] the custom title bar
  - [ ] non-US keyboard shortcuts
  - [ ] trackpad scrolling and zoom
  - [ ] Alt+drag rectangular selection where Alt+drag moves windows
- [ ] **Content:**
  - [ ] PDFs with unusual outlines or encryption
  - [ ] heavily styled or fixed-layout EPUBs
  - [ ] math sharpness on HiDPI, and how long the first formula takes
        after launch
- [ ] **Spell check** with real Hunspell dictionaries on Windows and macOS.

## 5. Decisions

- **When to delete the old builds.** After the cutover, the Tauri builds
  (`apps/*/src-tauri`, `crates/marina-tauri`) are only a rollback path (see
  `docs/packaging.md`). The Svelte renderers and Electron builds go with
  them: the npm workspaces, `packages/desktop-ui`, and the Playwright
  tests. Suggested order: Tauri first, then Svelte/Electron.
- **Tidy-up on the development machine.** `~/.local/share/` has leftover
  `marina-editor`, `marina-gallery` and `marina-shell` folders from test
  runs, and `/tmp` has spike caches (`marina-spike-article`,
  `marina-spike-epub`).
