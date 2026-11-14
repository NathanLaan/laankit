# Security Policy

LaanKit is a set of six local-first desktop apps: NoteLiner, ThreadLiner, PageLiner, BlockLiner, PrintLiner, and MediaLiner. This document describes what is supported, how to report a vulnerability, and the security model you can reason about.

## Supported versions

Each app has its own version. Security fixes are made against each app's latest released version; there are no long-term support branches. Please reproduce any issue on the current release (or `main`) before reporting.

## Reporting a vulnerability

Please do not open a public issue for security problems.

Report privately through GitHub's private vulnerability reporting:

1. Go to the repository's Security tab → [Report a vulnerability](https://github.com/NathanLaan/laankit/security/advisories/new).
2. Describe the issue, the affected app(s) and version, and the steps to reproduce. A minimal proof of concept helps.

This keeps the report confidential while it is being triaged. You will get an acknowledgement, and once a fix is available we will coordinate disclosure and credit you if you would like. As a single-maintainer project, responses are best-effort rather than on a guaranteed SLA.

When reporting, please describe the class of problem and how to reproduce it, but not a weaponized exploit or a step-by-step data-exfiltration path.

## Security model

Understanding how LaanKit handles data and the network makes it clearer what is and isn't in scope.

### Local-first by design

- Your data lives on your own machine, in files you control: a git repository for NoteLiner, ThreadLiner, and PageLiner, any folder of Mermaid files for BlockLiner, and your own photo folders for MediaLiner. If you configure a git remote, syncing pushes to your remote with your credentials. LaanKit operates no servers and has no accounts.
- LaanKit shells out to the system `git` CLI, so it uses your existing SSH keys and git configuration. It does not store or manage credentials itself.

### Network use

Network access is limited and user-initiated:

- Git sync: push/pull to the remote you configure (NoteLiner, ThreadLiner, and PageLiner).
- Feed fetching: ThreadLiner retrieves the RSS/Atom/JSON feeds you subscribe to.
- Update check: a check for new releases at launch.
- Bambu Lab cloud: PrintLiner signs in to your Bambu Lab account and reads your printers' status from Bambu Lab's servers (REST over HTTPS, and MQTT over TLS with Mozilla's root certificates). It only reads status; the one message it publishes asks a printer for its full report.

No app opens a listening network port for inbound connections.

### Rendering untrusted content

ThreadLiner renders feed article HTML and PageLiner renders EPUB content. This content is untrusted. It is rendered natively (`laankit-richtext`) rather than in a webview, and is treated as data, not as a place to execute scripts. Rendering bugs that could be triggered by a malicious feed or EPUB are in scope.

### NoteLiner's MCP server

NoteLiner can expose the open project to local AI assistants over the Model Context Protocol. It is off by default and designed to be local-only:

- It binds no TCP port. It listens on a user-scoped Unix domain socket (`chmod 0600`); there is no remote transport.
- It only ever exposes the currently open project; switching projects tears the server down before new data is reachable.
- Write tools can require per-call confirmation (*Allow once* / *for the session* / *Deny*) and can be disabled per tool. Writes go through the same save path as the UI and are size-capped.
- Every MCP call is logged in the app's Log panel.

The MCP server is not available on Windows yet (it requires a Unix socket).

### PrintLiner's Bambu Lab sign-in

- Your password is sent only to Bambu Lab, for the sign-in request, and is never written anywhere.
- The access token Bambu Lab returns is kept in the OS keychain (Secret Service on Linux, Credential Manager on Windows, Keychain on macOS). On a machine with no keychain, PrintLiner asks before keeping it in `token.json` in its config folder, readable only by your user (`0600` on Unix), and the status bar says so. Otherwise it keeps the token in memory until it closes.
- Signing out deletes the stored token. A token that Bambu Lab refuses is never retried; PrintLiner asks you to sign in again.
- Anyone with your user account can read a token stored in a file, so on a shared machine use the keychain or a dedicated user account.

### MediaLiner and your photo folders

- MediaLiner reads the folders you add and never changes a photo's contents. It writes nothing into your folders except what a merge moves there and a merge's `.medialiner-duplicates` folder.
- Merging a folder is the only operation that moves your files. It never overwrites a file, writes each move to a journal (in MediaLiner's data folder) before making it, and across drives deletes an original only after its copy is complete and the same size. Undo moves the files back from the journal.
- Photos are untrusted input: they're decoded by the pure-Rust `image` and `kamadak-exif` crates. A photo that crashes or hangs the decoder is in scope.
- A photo's EXIF location is hidden in the Info pane until you choose to show it. Thumbnails are cached in your user's cache folder; Settings → Storage clears them.
- Open in Folder runs `explorer.exe` on Windows, and `gdbus` or `dbus-send` on Linux, with the file's path as an argument, never through a shell.

### Distribution and integrity

- Released binaries are currently unsigned on Windows and macOS. Windows SmartScreen warns on first run, and macOS Gatekeeper blocks the first launch (right-click → Open, or clear the quarantine attribute). Code signing is a tracked TODO; see [`docs/packaging.md`](docs/packaging.md).
- Verify downloads come from the official [GitHub releases](https://github.com/NathanLaan/laankit/releases).
- Bundled third-party material (fonts, icons) is listed in [`NOTICE`](NOTICE).

## Out of scope

- Vulnerabilities in third-party dependencies that are already public and tracked upstream (report upstream; we will pick up the fix).
- Issues that require an already-compromised machine or physical access.
- The absence of code signing on prebuilt binaries (known; see above).
