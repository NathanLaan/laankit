# Security Policy

Marina is a set of local-first desktop apps (NoteLiner, ThreadLiner, PageLiner).
This document describes what is supported, how to report a vulnerability, and the
security model you can reason about.

## Supported versions

Marina ships from a single shared version across all three apps. Security fixes
are made against the **latest released version**; there are no long-term support
branches. Please reproduce any issue on the current release (or `main`) before
reporting.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report privately through GitHub's private vulnerability reporting:

1. Go to the repository's **Security** tab →
   **[Report a vulnerability](https://github.com/NathanLaan/marina/security/advisories/new)**.
2. Describe the issue, the affected app(s) and version, and the steps to
   reproduce. A minimal proof of concept helps.

This keeps the report confidential while it is being triaged. You will get an
acknowledgement, and once a fix is available we will coordinate disclosure and
credit you if you would like. As a single-maintainer project, responses are
best-effort rather than on a guaranteed SLA.

When reporting, please describe the **class** of problem and how to reproduce it
— not a weaponized exploit or a step-by-step data-exfiltration path.

## Security model

Understanding how Marina handles data and the network makes it clearer what is
and isn't in scope.

### Local-first by design

- Your data lives on your own machine, inside a git repository you control. If
  you configure a remote, syncing pushes to **your** remote with **your**
  credentials — Marina operates no servers and has no accounts.
- Marina shells out to the system `git` CLI, so it uses your existing SSH keys
  and git configuration. It does not store or manage credentials itself.

### Network use

Network access is limited and user-initiated:

- **Git sync** — push/pull to the remote you configure (all three apps).
- **Feed fetching** — ThreadLiner retrieves the RSS/Atom/JSON feeds you
  subscribe to.
- **Update check** — a check for new releases at launch.

No app opens a listening network port for inbound connections.

### Rendering untrusted content

ThreadLiner renders feed article HTML and PageLiner renders EPUB content. This
content is **untrusted**. It is rendered natively (`marina-richtext`) rather than
in a webview, and is treated as data, not as a place to execute scripts.
Rendering bugs that could be triggered by a malicious feed or EPUB are in scope.

### NoteLiner's MCP server

NoteLiner can expose the open project to local AI assistants over the Model
Context Protocol. It is **off by default** and designed to be local-only:

- It binds **no TCP port**. It listens on a user-scoped Unix domain socket
  (`chmod 0600`); there is no remote transport.
- It only ever exposes the **currently open project**; switching projects tears
  the server down before new data is reachable.
- Write tools can require **per-call confirmation** (*Allow once* / *for the
  session* / *Deny*) and can be disabled **per tool**. Writes go through the same
  save path as the UI and are size-capped.
- Every MCP call is logged in the app's Log panel.

The MCP server is not available on Windows yet (it requires a Unix socket).

### Distribution and integrity

- Released binaries are currently **unsigned** on Windows and macOS. Windows
  SmartScreen warns on first run, and macOS Gatekeeper blocks the first launch
  (right-click → Open, or clear the quarantine attribute). Code signing is a
  tracked TODO; see [`docs/packaging.md`](docs/packaging.md).
- Verify downloads come from the official
  [GitHub releases](https://github.com/NathanLaan/marina/releases).
- Bundled third-party material (fonts, icons) is listed in [`NOTICE`](NOTICE).

## Out of scope

- Vulnerabilities in third-party dependencies that are already public and
  tracked upstream (report upstream; we will pick up the fix).
- Issues that require an already-compromised machine or physical access.
- The absence of code signing on prebuilt binaries (known; see above).
