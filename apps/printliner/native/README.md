# PrintLiner Native

PrintLiner's app: an [egui](https://github.com/emilk/egui) UI over `printliner-core`, with no webview or JavaScript. It runs on Linux, Windows, and macOS.

## Running

```bash
cargo run -p printliner-native --release
cargo run -p printliner-native --release -- --demo    # three made-up printers, no account
cargo test -p printliner-core -p printliner-native
```

The package is `printliner-native`; its binary is `printliner`, the one the release packages install as `/usr/bin/printliner`.

- Signing in. Sign In on the welcome screen (or the palette) asks for the region, email, and password, then the emailed code if the account wants one. An account made with Google or Apple sign-in has no Bambu Lab password: enter the email, leave the password empty, and choose Email Me a Code Instead. Accounts that confirm sign-ins with an authenticator app aren't supported yet; PrintLiner says so rather than failing.
- The token. It goes to the system keychain. If there's no keychain (a minimal Linux desktop with no Secret Service), PrintLiner asks whether to keep it in a private file instead; otherwise it lasts until PrintLiner closes. Settings → Account shows where it is and when it expires, and Sign Out deletes it.
- Signing in again. Bambu Lab's tokens can't be refreshed. From 7 days before the token expires, a banner offers Sign In Again; when Bambu Lab refuses the token, the dashboard says so and offers Sign In.
- Window chrome. Settings → UI → Custom Window Titlebar switches between the system frame and LaanKit's title bar (takes a restart).

## Keys

| Keys | Does |
|---|---|
| Click a card | The printer's detail |
| Esc, Alt+Left | Back to all printers |
| F5, Ctrl+R | Refresh the printer list and task history |
| Ctrl+= / Ctrl+- / Ctrl+0 | UI scale |
| Ctrl+J | Toggle the status bar |
| Ctrl+K, Ctrl+,, Ctrl+Shift+I | Command palette, Settings, About |

## How status arrives

1. Sign-in gives an access token; the account's preferences give its user id, and the MQTT username is `u_<uid>`.
2. The printer list (`/v1/iot-service/api/user/bind`) loads at start, every 5 minutes, and on Refresh.
3. One MQTT connection to the region's broker (port 8883, TLS with Mozilla's roots) subscribes to `device/<serial>/report` for every printer, and sends each a `pushall` on every connect, since P1 and A1 printers otherwise send only changes.
4. Each report is merged into the printer's full state (objects key by key; arrays and values replace) and read into a `PrinterStatus`. The UI gets a printer's status at most once a second, and at once when its state changes.
5. Task history loads on connect (the last 50) and when a job ends (the last 5, merged in). Plate images are fetched when shown and cached on disk.
6. A printer with no report for 60 seconds shows as No signal. A refused token stops the connection and asks you to sign in again; any other drop retries after 5 seconds, doubling to 60.

## Architecture

`printliner-core` (`crates/printliner-core`) knows nothing about egui:

| Module | What it does |
|---|---|
| `region` | The regions and their REST and MQTT hosts |
| `cloud` | The REST client: sign in, email code, MQTT username, devices, tasks, images; lenient parsing |
| `mqtt` | The MQTT session: subscribe, `pushall`, reconnect with backoff, stop on refused credentials |
| `status` | `merge_json`, `GcodeState`, `PrinterStatus` |
| `hms`, `ams` | HMS codes and severity; AMS units, trays, and the tray in use |
| `history` | The trend samples, append, read, and prune |
| `secrets` | The token in the keychain or a private file |
| `demo` | The demo printers |

The app (`apps/printliner/native/src`):

| Module | What it does |
|---|---|
| `main` | The window, `--demo` |
| `app` | All UI state; backend events, commands, view actions, dialogs |
| `backend` | The session on a tokio runtime: sign-in, token storage, devices, MQTT, throttling, sampling, tasks, covers; or the demo |
| `views` | Toolbar, status bar, banners, welcome, printer cards |
| `detail` | A printer's detail, and the trends chart |
| `model` | A printer as the views show it, and the state's words and tone |
| `overlays` | Sign In, Settings, About, confirmations |
| `config` | `settings.json` and `ui-preferences.json` |
| `commands` | The commands and shortcuts |

## Tests

```bash
cargo test -p printliner-core -p printliner-native
```

- `printliner-core`: parsing from fixtures (`tests/fixtures/`: device list, tasks, a full report, and a change), merging, HMS and AMS decoding, history files, the token file's permissions, the REST client against a mock server (`tests/cloud_signin.rs`), and the MQTT session against an in-process broker (`tests/mqtt_session.rs`).
- The app: headless `egui_kittest` tests (`ui_tests.rs`) over the demo printers (cards, detail, back, reordering) and a full sign-in with an emailed code against a mock server, with the token in a private file.

The fixtures are made up to the shape PrintLiner reads; recorded responses from a real account replace them once the cloud has been checked against real printers (P0 in the plan).

## Diagnostics

The shared `LAANKIT_SCREENSHOT`, `LAANKIT_THEME`, and `LAANKIT_SCALE` work as in the other apps. `LAANKIT_OPEN` takes `settings`, `about`, `palette`, `signin`, or `printer:<serial>`:

```bash
WAYLAND_DISPLAY= LAANKIT_OPEN=printer:DEMO00X1C0001 LAANKIT_SCREENSHOT=/tmp/detail.png LAANKIT_SCREENSHOT_EXIT=1 \
  cargo run -p printliner-native -- --demo
```

Debug builds also read `PRINTLINER_API_BASE` (a REST base URL) and `PRINTLINER_MQTT_HOST` (`host:port`, plain TCP), to point PrintLiner at a local mock server and broker.
