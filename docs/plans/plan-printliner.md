# PrintLiner: Implementation Plan

Status: Part 1 (the dashboard) implemented (2026-11-07), PrintLiner 0.1.0, except P0, which needs a real Bambu account and printers. Parts 2 and 3 not started. A new LaanKit app, PrintLiner, that shows a live dashboard of your Bambu Lab 3D printers and runs as a basic touch kiosk next to them, where anyone can see what each printer is doing and send one of a curated set of prints. It talks to the printers through the Bambu Lab cloud (your Bambu account), not over the local network.

## Summary

PrintLiner is the fifth LaanKit app, built the same way as the other four: pure `Rust`, egui/eframe over the shared `laankit-egui` shell, with a UI-independent backend crate (`printliner-core`) and its data stored as plain files you control.

It has two modes in one binary:

| Mode | Who | What it does |
|---|---|---|
| Dashboard | You, at your desk | Signs in to your Bambu account, shows every printer bound to it with live status, details, HMS alerts, AMS trays, and task history, and manages the library of printable items. |
| Kiosk | Anyone walking up | Full screen, touch first. Shows each printer's state, progress, and time left, lists the published items with their preview images, and offers Print on an item whose printer is online and idle. Leaving kiosk mode or changing anything needs the admin PIN. |

The printers stay in their normal cloud mode. No LAN-only mode, no Developer Mode, no fixed IP addresses, and no access codes. Bambu Handy and Bambu Studio keep working alongside PrintLiner.

The work is in three parts. Dashboard mode is built and used first. A decision phase then puts the kiosk questions to you again, with real use of the dashboard behind the answers and a spike on printing and control. Kiosk mode, the library, and printing are built last, on what that phase decides. See Phases.

Naming follows the other apps: the product is PrintLiner, the binary and package are `printliner`, the Cargo packages are `printliner-native` and `printliner-core`, and the app id is `com.printliner.app`.

## Part 1: what was built and what changed

- P1 to P4 are done: `crates/printliner-core` and `apps/printliner/native`, the packaging files, the CI and packaging script lists, and the README, ARCHITECTURE, SECURITY, and packaging docs. `cargo clippy` is clean and 44 tests pass (core unit tests, the REST client against a mock server, the MQTT session against an in-process broker, and headless UI tests over the demo printers and a full emailed-code sign-in).
- P0 is not done: nothing has run against a real Bambu account yet. The REST and report fixtures are made up to the shape PrintLiner reads, from the endpoints and fields in this plan; P0 replaces them with recorded, anonymised ones. Until then, the sign-in, the EU broker, one MQTT connection for several printers, and the task `status` words are unconfirmed.
- Sign-in with an emailed code alone, no password (Email Me a Code Instead), for accounts made with Google or Apple sign-in. The first real sign-in, with a Google account, failed with Bambu Lab's 400 "Incorrect account or password"; a refused password now explains the likely causes (wrong password, wrong region, or a Google or Apple account) instead of showing the server's words.
- Demo mode synthesises its reports instead of replaying recorded ones, since there are none yet.
- The refused-credentials path is tested against a stand-in broker that answers CONNECT with "not authorised" (CONNACK code 5), as the plan expects of Bambu's broker; rumqttd drops a bad login without answering, so it can't test that path.
- Without a keychain, the token can stay in memory for the run (the plan offered only the file); the question offers Save in File or Not Now.
- HMS codes show their code, severity, and a link to the wiki home page; no message table yet.
- The trend chart and the progress ring are drawn with egui's painter; no charting dependency.
- Settings → Printers renames and reorders printers for display, as the plan's Dashboard mode table says.
- `LAANKIT_OPEN=printer:<serial>` opens a printer's detail, for screenshots.
- `scripts/set-version.sh` now includes PrintLiner, so a shared version bump moves it to the same version as the other apps.
- Checked by eye (`LAANKIT_SCREENSHOT`): the demo dashboard and a printer's detail in the dark theme, the sign-in dialog in the light theme, and the dashboard and detail in the light theme at 50% and 200% scale (the grid wraps to one column and the detail to one column at 200%).

## Decisions

| Decision | Status |
|---|---|
| D1: Connect through the Bambu cloud (REST plus cloud MQTT) | Proposed, confirmed in P0 |
| D2: Start a print by handing the item to Bambu Connect, not by sending print commands ourselves | Proposed, decided in P5 |
| D3: Live control (pause, resume, stop, light) only where the cloud accepts it | Proposed, decided in P5 |
| D4: Two modes, Dashboard and Kiosk, with a local admin PIN instead of user accounts | Proposed, decided in P5 |
| D5: One sliced 3MF per item, tied to one printer, sent as is | Proposed, decided in P5 |
| D6: The library is a folder you choose; history and settings live in the app's own folders | Proposed (settings and history in P1; the library folder in P6) |
| D7: The Bambu access token is kept in the OS keychain, never the password | Proposed, built in P2 |

D1, D6, and D7 are what the dashboard needs. D2 to D5 are about the kiosk and printing, so they stay proposals until the decision phase (P5).

### D1: Bambu cloud, not the LAN

| | Bambu cloud (this plan) | Local network |
|---|---|---|
| Printer setup | Bound to your Bambu account, as it already is | LAN-only mode, Developer Mode, fixed IP, access code per printer |
| Bambu Handy and cloud features | Kept | Lost |
| Where PrintLiner can run | Anywhere with internet | Only on the printers' network |
| Live status | Cloud MQTT, the same report format | LAN MQTT |
| Third-party print control | Restricted by Bambu's Authorization Control firmware (see D2, D3) | Works with Developer Mode |
| Needs internet | Yes | No |

PrintLiner signs in once with your Bambu account, lists the printers bound to it, and streams their status over Bambu's cloud MQTT broker. Nothing on the printers changes.

### D2: Printing through Bambu Connect

Bambu's Authorization Control firmware (2025) requires print commands from third-party software to come through Bambu's own authorized clients. An unsigned `project_file` command sent over cloud MQTT by PrintLiner would be rejected by current X1, P1, A1, and H2 firmware, and working around the signing is not something PrintLiner will do.

The sanctioned route is Bambu Connect, Bambu's helper app for third-party integrations. PrintLiner hands it the item's 3MF through its URL scheme:

```
bambu-connect://import-file?path=<url-encoded absolute path>&name=<url-encoded item title>&version=1.0.0
```

Bambu Connect opens with the file, the person confirms the printer and the plate settings, and Bambu Connect sends the job through the cloud. PrintLiner then tracks the job from the printer's live status (see Starting a print).

| Option | Works on current firmware | One tap | Notes |
|---|---|---|---|
| A. Bambu Connect hand-off (recommended) | Yes | No, one confirm in Bambu Connect | The supported path. Needs Bambu Connect installed and signed in on the same machine. |
| B. Bambu Studio hand-off | Yes | No, the person sends from Studio | Fallback where Bambu Connect isn't available. Opens the 3MF in Bambu Studio (`bambu-studio <file>`). Heavier on a kiosk. |
| C. Direct cloud `project_file` | Only on firmware without Authorization Control | Yes | Not built. Revisit only if P5 shows a supported way to do it. |

The hand-off target is a setting (Bambu Connect or Bambu Studio), behind one small `Handoff` type in the app. The decision phase (P5) confirms the URL scheme, the parameters, and which platforms Bambu Connect runs on. If Bambu Connect has no build for the kiosk's operating system, the kiosk uses Bambu Studio, and that is recorded in P5's findings.

### D3: Live control

Pause, resume, stop, and the chamber light are print-control commands too, so the same firmware restriction is expected to apply over the cloud. PrintLiner sends only `pushall` (a full status request) until the decision phase shows otherwise, so the dashboard ships without controls. P5 tries each command per model and firmware and records the result in a capability table in `printliner-core`. A command the table marks as accepted gets a button on the printer detail view in Dashboard mode; nothing else does. Kiosk mode never shows controls.

### D4: Two modes and an admin PIN

A desktop app has no server-side identity, so the "anyone can look and print, only an admin can change anything" rule becomes two modes:

| | Kiosk mode | Dashboard mode |
|---|---|---|
| See each printer's state, progress, time left, and current job | Yes | Yes |
| Browse published items with preview images | Yes | Yes |
| Print an item whose printer is online and idle | Yes | Yes |
| Printer detail: temperatures, AMS, HMS, task history, trends | No | Yes |
| Live controls (where D3 allows) | No | Yes |
| Sign in or out of the Bambu account | No | Yes |
| Import, edit, publish, and delete items | No | Yes |
| Settings, leave kiosk mode, quit | No | Yes |

- Kiosk mode starts with `printliner --kiosk`, or from the View menu. It goes full screen, hides the menus, title bar, command palette, and keyboard shortcuts other than the unlock gesture.
- Unlocking (a long press on the PrintLiner logo, or Ctrl+Alt+K) asks for the admin PIN. The PIN is set the first time kiosk mode is turned on, and stored as an Argon2 hash in the settings file, never in plain text.
- An unlocked kiosk returns to kiosk mode after the idle timeout (120 seconds by default) with no input. A countdown toast shows for the last 15 seconds.
- No PIN set means kiosk mode can't be turned on. That stops a kiosk being left open by accident.
- The operating system's own kiosk setup (auto login, auto start, no other apps) is out of scope; the packaging docs say how to auto start `printliner --kiosk`.

### D5: What a printable item is

An item is one `.gcode.3mf` exported from Bambu Studio or OrcaSlicer ("Export plate sliced file"), tied to one printer bound to your account.

- On import, `printliner-core` opens the 3MF and requires `Metadata/plate_N.gcode`, so an unsliced project is refused with a clear message.
- It reads `Metadata/slice_info.config` for the printer model, nozzle size, estimated time, weight, and filament list, and takes `Metadata/plate_N.png` as the preview image.
- A 3MF sliced for a different printer model than the chosen printer is refused. This is a hard block, not a warning.
- A 3MF with several plates asks which plate the item is.
- Print hands over that exact file. Nothing is re-sliced or re-mapped.

### D6: Where the data lives

PrintLiner follows the LaanKit rule that your data is plain files you control.

| Where | What |
|---|---|
| The library folder (you choose it, like a BlockLiner folder) | `printliner.json`, the item index (title, description, printer serial, plate, slice info, published, position), and `items/<id>.gcode.3mf` and `items/<id>.png` for each item. Copy the folder to another machine and the library comes with it. |
| `~/.config/PrintLiner` | `settings.json` (region, account email, hand-off target, PIN hash, idle timeout, last library folder, printer order and display names) and `ui-preferences.json` (theme, scale, panes, as the other apps). |
| `~/.local/share/printliner` | `jobs.jsonl` (one line per hand-off and its outcome), `history/<serial>/<YYYY-MM>.jsonl` (60-second status samples for the trends view, pruned after 90 days), a cover image cache, and eframe's `app.ron`. |

Writes go through `laankit-core::json` (atomic, order preserving) and `laankit-core::debounce`, as in the other apps. PrintLiner doesn't use git.

### D7: The Bambu access token

- The password is used only for the sign-in request and never written anywhere.
- The access token is stored in the OS keychain (Secret Service on Linux, Credential Manager on Windows, Keychain on macOS) through the `keyring` crate, under the service `com.printliner.app` and the account email.
- Where no keychain is available (a minimal Linux kiosk with no Secret Service), PrintLiner asks before falling back to a file in the config folder with `0600` permissions, and the status bar says the token is stored in a file.
- `SECURITY.md` gets a PrintLiner section: what it stores, that it connects to Bambu's servers, and that it opens no listening port.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  apps/printliner/native   eframe window, dashboard, kiosk,       │
│                           library views, overlays                │
│                           + a tokio backend bridging to the core │
├──────────────────┬───────────────────────────────────────────────┤
│  laankit-egui    │  printliner-core                              │
│  (egui shell)    │  cloud · mqtt · status · hms · ams · threemf  │
│                  │  · library · jobs · history                   │
├──────────────────┴───────────────────────────────────────────────┤
│  laankit-core   JSON stores · paths · debounce · update check    │
└──────────────────────────────────────────────────────────────────┘
```

Dependencies point down, as in the rest of LaanKit: `printliner-core` knows nothing about egui and is tested on its own.

### `printliner-core`

| Module | What it does |
|---|---|
| `cloud` | The REST client: sign in, email code, device list, task history, cover images. `reqwest` with rustls (already in the workspace). |
| `mqtt` | The cloud MQTT session: connect, subscribe to each printer's report topic, send `pushall`, reconnect with backoff. `rumqttc` with rustls and `webpki-roots`. |
| `status` | Merges partial reports into a full state (`merge_json`) and maps it to a typed `PrinterStatus`. |
| `hms` | Decodes HMS entries into display codes, severity, and a wiki link. |
| `ams` | Parses AMS units and trays, the active tray, and humidity. |
| `capabilities` | The per-model table from P5: which commands the cloud accepts, whether the model has bed and first-layer detection. |
| `threemf` | Opens a 3MF, checks it is sliced, reads slice info, extracts the plate preview. `zip` and `quick-xml`. |
| `library` | The library folder: index, import, edit, publish, reorder, delete. |
| `jobs` | Hand-off records and matching them to what the printers report. |
| `history` | 60-second status samples and pruning. |

### The app

| Module | What it does |
|---|---|
| `main` | Window setup, `--kiosk`, the persistence path, and the shared diagnostics (`LAANKIT_SCREENSHOT`, forced theme and scale). |
| `app` | All UI state; handles backend events, commands, and view actions. |
| `backend` | A tokio runtime around `printliner-core`. The UI sends requests over channels; the backend sends back `Event`s (status changed, devices refreshed, sign-in needed, import finished). No network or file work on the UI thread. |
| `views` | Dashboard grid, printer detail, library, kiosk home, kiosk items. |
| `overlays` | Sign in, email code, PIN entry, import item, print confirm, settings, about. |
| `handoff` | Opens the 3MF in Bambu Connect or Bambu Studio. |
| `config` | `settings.json` and `ui-preferences.json`. |

The UI repaints when the backend sends an event (`ctx.request_repaint`), not on a timer, so an idle dashboard uses almost no CPU.

## Bambu cloud integration

### Regions and hosts

| Region | REST base | MQTT broker |
|---|---|---|
| Global (default) | `https://api.bambulab.com` | `us.mqtt.bambulab.com:8883` |
| Europe | `https://api.bambulab.com` | `eu.mqtt.bambulab.com:8883` |
| China | `https://api.bambulab.cn` | `cn.mqtt.bambulab.com:8883` |

The region is chosen on the sign-in dialog. P0 checks that a global account works on the EU broker before it is offered.

Requests send `User-Agent: PrintLiner/<version>` and use a 20-second timeout.

### Signing in

1. `POST /v1/user-service/user/login` with `{"account": email, "password": password}`.
2. The response holds `accessToken`, `expiresIn` (seconds), `loginType`, and `tfaKey`.
   - A non-empty `accessToken` means signed in; the expiry is now plus `expiresIn`.
   - `loginType == "verifyCode"` means an email code is needed: `POST /v1/user-service/user/sendemail/code` with `{"email": email, "type": "codeLogin"}`, show the code field, then `POST /v1/user-service/user/login` with `{"account": email, "code": code}`.
   - A non-empty `tfaKey` means the account has an authenticator app. P0 finds the TFA endpoint and request shape; until then PrintLiner says the account's two-factor method isn't supported yet, rather than treating it as an email code.
   - A 401 is a wrong email or password.
3. `GET /v1/design-user-service/my/preference` gives `{uid}`; the MQTT username is `u_<uid>`. If this fails, sign-in fails with that message, since status can't work without it.

There is no working token refresh. PrintLiner checks the expiry at start and warns in Dashboard mode from 7 days before it. A 401 or 403 from REST, or a bad-credentials CONNACK from MQTT, marks the account "sign-in needed": Dashboard mode shows a banner with Sign In, and Kiosk mode shows "Printers unavailable, ask a staff member" on every card and disables Print.

### Device list and task history

| Request | Used for | Fields read |
|---|---|---|
| `GET /v1/iot-service/api/user/bind` | The printers on the account | `dev_id` (serial), `name`, `dev_model_name`, `dev_product_name`, `online` |
| `GET /v1/user-service/my/tasks?deviceId=<serial>&limit=<n>` | Task history and the current job's cover | `id` (number or string), `title`, `cover`, `status` (a string), `weight`, `costTime`, `startTime`, `endTime` (RFC 3339 or epoch seconds or milliseconds), `deviceId` |
| `GET <cover url>` | The plate image for a task | Retry once with the bearer token on 403 |

The device list refreshes at start, every 5 minutes, and from Refresh in Dashboard mode. Task history loads on connect (`limit=50`) and again when a printer's job finishes (`limit=5`). The response's `dev_access_code` is not stored.

Parsing is lenient on purpose: numbers may arrive as strings, `id` may be either, and unknown fields are ignored. Each of these gets a unit test from a recorded response.

### Live status over MQTT

- Credentials: username `u_<uid>`, password the access token. TLS through rustls with `webpki-roots`; certificate verification is never turned off.
- Topics: subscribe to `device/<serial>/report`; publish to `device/<serial>/request`.
- On every connect and reconnect, send `{"pushing":{"sequence_id":"<n>","command":"pushall","version":1,"push_target":1}}` to each printer, since P1 and A1 printers otherwise send only changes. `sequence_id` is a per-session counter.
- Each report is deep-merged into the printer's last full state (objects merge key by key; arrays and scalars replace).
- One connection per account subscribed to every printer is the target. P0 checks the broker allows it; if not, one connection per printer, as the fallback.
- Reconnect with backoff (5 s, doubling to 60 s). A printer with no report for 60 seconds shows as "No signal" while the device list still says online.
- The UI gets at most one event per printer per second, plus one at once on every `gcode_state` change.

`PrinterStatus` fields, all from `print.*`:

| Field | Source |
|---|---|
| State | `gcode_state`: IDLE, PREPARE, RUNNING, PAUSE, FINISH, FAILED |
| Progress, time left | `mc_percent`, `mc_remaining_time` (minutes) |
| Layer | `layer_num` of `total_layer_num` |
| Job name | `subtask_name`, else `gcode_file` |
| Job id | `task_id`, else `subtask_id`; `"0"` or empty means none |
| Stage | `mc_print_stage` (number or string) |
| Temperatures | `nozzle_temper`, `nozzle_target_temper`, `bed_temper`, `bed_target_temper`, `chamber_temper` |
| Speed, Wi-Fi | `spd_lvl`, `wifi_signal` |
| Errors | `print_error`, `hms` |
| AMS | `ams` (trays, active tray, humidity), `vt_tray` |

"Idle" for printing purposes means online, a fresh report, and `gcode_state` of IDLE, FINISH, or FAILED.

### HMS codes

Each `{attr, code}` entry shows as `AAAA_BBBB_CCCC_DDDD` (the four 16-bit halves in hex), with severity from `code >> 16`: 1 fatal, 2 serious, 3 common, 4 info. The detail view links each code to Bambu's HMS wiki page. A bundled table of readable messages for the common codes (filament runout, bed obstruction, first-layer defect, nozzle clog) is a later addition; the code and severity are always shown.

### Rate and politeness

PrintLiner polls only the device list (5 minutes) and task history (on connect and after a job ends). Everything live comes from MQTT. There is no scraping, no browser user agent, and no request the Bambu apps wouldn't also make.

## Printable items

### Importing (Dashboard mode)

Library, Import Item, choose a `.gcode.3mf`, choose the printer, then title and description. `printliner-core::threemf` checks it (D5), copies it into `items/` under a new id, writes the preview PNG beside it, and adds it to `printliner.json` unpublished. Items can be edited, reordered, published, unpublished, replaced with a new 3MF, and deleted. A printer that leaves the account keeps its items, shown as "Printer not on this account" and unprintable until reassigned.

### Browsing and printing (both modes)

A grid of cards: preview image, title, estimated time, grams, and the printer with its live state ("Ready", "Printing 43%", "Offline"). Print is enabled only when the item's printer is idle (see above); otherwise it is disabled with the reason on the button. The kiosk can filter by printer.

## Starting a print

1. Press Print on an item card.
2. The confirm dialog shows the item, the printer, the estimate, and "Make sure the bed is clear and the right filament is loaded". On models the capability table marks as having no bed detection, that line is shown as a warning.
3. Print hands the file to Bambu Connect (or Bambu Studio). PrintLiner writes a job record in `jobs.jsonl` with state `handed_off`, the item, the printer, and the time, and shows "Finish in Bambu Connect" on the item card.

PrintLiner then follows the job from the printer's own reports:

```
handed_off ──► printing ⇄ paused ──► finished
     │                               failed
     └──► expired (no matching job within 10 minutes)
```

- A job matches when the item's printer starts a job (`gcode_state` leaves IDLE/FINISH/FAILED) whose name matches the item's file or title, within 10 minutes of the hand-off.
- `FINISH` is finished; `FAILED` is failed, with the HMS codes or `print_error` as the reason; a job that matches nothing in 10 minutes becomes expired.
- A job started from anywhere else (Bambu Studio, Handy, the printer's screen) shows on the dashboard as an external job.
- While a hand-off is pending, Print on that printer's other items is disabled, so two people can't hand two jobs to one bed from the kiosk.

The confirm step in Bambu Connect is accepted. It is also a second chance for the person to check the printer and plate.

## Dashboard mode

| View | Content |
|---|---|
| Printers (home) | One card per printer, in the saved order: name, model, state, progress ring, time left, layer, job name and cover, HMS badge, active filament colour. |
| Printer detail | Temperatures (current and target), stage, speed, Wi-Fi, AMS trays with colour, type, remaining, and humidity, HMS list with severity and wiki links, task history (the cloud's last 50 tasks with cover, result, time, and grams), a 24-hour or 7-day trend chart of progress and temperatures from the history samples, and the controls D3 allows. |
| Library | Items with import, edit, publish, reorder, and delete. |
| Jobs | The hand-off log: when, item, printer, outcome. |
| Settings | Account (region, email, sign in and out, token expiry), hand-off target, kiosk (PIN, idle timeout, which printers the kiosk shows), library folder, and the shared theme and scale settings. |

Printer cards can be renamed and reordered for display. That changes only PrintLiner's settings, not the name on the Bambu account.

The trend chart is drawn with egui's painter, as BlockLiner's canvas is, so no charting dependency is added.

## Kiosk mode

| Screen | Content |
|---|---|
| Printers | The printer cards, larger, with state, progress, time left, and job name. No detail on tap. |
| Items | The published items grid with Print. |

- Two large tabs at the bottom switch between them. Every touch target is at least 48 logical pixels, and nothing depends on hover.
- It fits 4 to 8 printers on one screen at 1920×1080 and 1280×800; more scroll.
- It is usable at 50% and 200% UI scale, in the light and dark themes.
- Text entry is only needed for the PIN, so the PIN dialog has its own on-screen number pad and needs no system keyboard.
- When the account needs signing in again, or the network is down, every card says so in plain words, and Print is disabled.

## Testing

| What | How |
|---|---|
| Report merging, status mapping, HMS, AMS, lenient parsing | Unit tests in `printliner-core` from recorded, anonymised reports and REST responses (serials replaced) in `crates/printliner-core/tests/fixtures/`. |
| Sign-in flows and errors | Unit tests against a local mock HTTP server (`wiremock`), with the REST base and MQTT host overridable through `PRINTLINER_API_BASE` and `PRINTLINER_MQTT_HOST` (debug builds only). |
| MQTT session, reconnect, `pushall` | An integration test against an in-process broker (`rumqttd`) with a test certificate. |
| 3MF import | Unit tests with small sliced and unsliced 3MF fixtures, a multi-plate file, and a model mismatch. |
| Job matching | Unit tests that feed status sequences to `jobs` and check handed off → printing → finished, failed, and expired. |
| The UI | `egui_kittest` tests, headless, over demo data: dashboard cards, detail, library import, print confirm and hand-off (with a fake `Handoff` that records the call), kiosk lock, PIN unlock, idle return, and the sign-in-needed state. |
| Demo mode | `printliner --demo` replays recorded reports for three printers (X1C, P1S, A1), so the UI can be developed and screenshotted with no account. The UI tests use the same data. |

None of this replaces running it against the real account and printers, and checking both themes at 50% and 200% in both modes.

## Workspace changes

| File | Change |
|---|---|
| `Cargo.toml` | Add `apps/printliner/native` to `members`, `printliner-core` to `workspace.dependencies`, and the new shared dependencies. |
| `crates/printliner-core/` | New. |
| `apps/printliner/` | New: `native/` (the app), `assets/icon.svg` and `icon.png`, `README.md`, `native/README.md`. |
| `packaging/linux/` | `com.printliner.app.desktop`, `.metainfo.xml`, `.appimage.desktop.hbs`. |
| `scripts/package-native.sh` | Add `printliner` to `ALL_APPS`. |
| `.github/workflows/build.yml` and the release workflow | Add `printliner-core` and `printliner-native` to the clippy and test lists and the release matrix. |
| `README.md`, `ARCHITECTURE.md`, `SECURITY.md`, `NOTICE` | The new app, its crate, its network use and stored token, and any new bundled assets. |

New dependencies:

| Crate | For |
|---|---|
| `rumqttc` (rustls) and `webpki-roots` | Cloud MQTT |
| `keyring` | The access token |
| `zip`, `quick-xml` | Reading 3MF files |
| `argon2` | The PIN hash |
| `wiremock`, `rumqttd` (dev only) | Tests |

## Phases

The phases are in three parts: the dashboard, a decision phase, and the kiosk. Each part ends with something you can use, so the dashboard is in daily use before anything about the kiosk is settled.

### Part 1: Dashboard

| Phase | Scope | Done when |
|---|---|---|
| P0: Cloud status spike | Against a real account and the lab printers: email code and authenticator sign-in, token lifetime, the account's region and the EU broker, one MQTT connection for several printers, and `pushall` and the report fields per model. Record anonymised fixtures. | A short findings section added to this plan, and D1 confirmed. Q1 answered. |
| P1: Scaffold | `printliner-core` and `printliner-native` skeletons, the window on the `laankit-egui` shell, settings, an icon, packaging files, CI lists. | `cargo run -p printliner-native` opens an empty dashboard; clippy and tests pass; `scripts/package-native.sh printliner` builds. |
| P2: Account | `cloud` module, sign-in and email code dialogs, keychain storage, device list, sign-in-needed handling, demo mode. | You can sign in and see your printers listed (offline status); a revoked token shows the banner. |
| P3: Live status | `mqtt`, `status`, `hms`, `ams`, history samples. | Every printer's live state reaches the UI within seconds; reconnect after a network drop works; the broker integration test passes. |
| P4: Dashboard | Printer cards, printer detail, task history with covers, the trend chart, and Settings (account, display names and order, theme and scale). No library, Jobs view, or controls yet. Version 0.1.0. | The dashboard is checked by eye in both themes at 50% and 200%; UI tests cover cards and detail; a 0.1.0 package is in use on your desktop. |

### Part 2: Decision phase

| Phase | Scope | Done when |
|---|---|---|
| P5: Kiosk decisions | Put the questions below (Q2 to Q6) to you again, with at least a week of dashboard use behind them. Alongside, a spike against the lab printers: which control commands the cloud accepts on current firmware, the Bambu Connect URL scheme and its parameters on Linux, Windows, and macOS, the Bambu Studio fallback, and the 3MF metadata from Bambu Studio and OrcaSlicer. | Every question answered; D2 to D5 decided or revised in this plan; the capability table checked into `printliner-core`; Part 3 updated to match. |

### Part 3: Kiosk

| Phase | Scope | Done when |
|---|---|---|
| P6: Library | `threemf`, `library`, the library view and import dialog, and the controls P5 allows on the printer detail view. | An unsliced or mismatched 3MF is refused; a published item shows its preview and its printer's live state. |
| P7: Printing | `handoff`, the confirm dialog, `jobs` and matching, the Jobs view. | One real print through the chosen hand-off on each lab model, tracked to finished; one failed print shows its reason; matching tests pass. |
| P8: Kiosk | `--kiosk`, full screen, PIN, idle return, the kiosk screens, touch sizing. | A day of use on the kiosk machine next to the printers by the team; the UI tests cover lock, unlock, and idle return. |
| P9: Release | Docs, `SECURITY.md`, packages for Linux, Windows, and macOS, version 1.0.0. | A draft GitHub release with PrintLiner packages. |

P0 runs before or beside P1. P2 needs P1, P3 needs P2, and P4 needs P3. P5 needs P4 in use. P6 needs P5, P7 needs P6, and P8 needs P7.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Bambu changes or closes the cloud endpoints PrintLiner uses | Status or sign-in stops working | The endpoints are confined to `cloud` and `mqtt`; fixtures make a change quick to diagnose; the update check ships fixes. |
| Bambu Connect has no build for the kiosk's operating system | No hand-off on that machine | Bambu Studio hand-off as the fallback; P5 confirms before P7. |
| Bambu Connect's URL scheme changes | Print stops working | The hand-off is one small module; P5 records the version; the confirm dialog says when the hand-off fails to launch. |
| The access token expires with no refresh | The kiosk can't show status until someone signs in | Expiry warning from 7 days before, a clear kiosk message, and sign-in from Dashboard mode. |
| Accounts with an authenticator app | Sign-in fails | P0 finds the TFA flow; until then a clear "not supported yet" message. |
| A print starts onto a dirty bed, or with the wrong filament | A failed or ruined print | The confirm dialog reminder; the Bambu Connect confirm; the printer's own detection on models that have it; the failure is shown with its reason. |
| Anyone at the kiosk can print | Wasted filament | Only published items, only on an idle printer, one pending hand-off per printer, and every hand-off logged. |
| The token stored in a file on a kiosk with no keychain | Anyone with the machine's user account can read it | Asked for explicitly, `0600`, shown in the status bar; a dedicated kiosk user account is recommended in the docs. |
| Internet outage | No status and no printing | Stated on every card; the printers carry on with their current jobs. |

## Out of scope

- The local network: LAN MQTT, FTP upload, SSDP discovery, and Developer Mode.
- Sending print or control commands that Bambu's firmware reserves for its authorized clients.
- A print queue, slicing, STL files, and AMS slot remapping.
- Camera streams (the cloud gives task cover images only).
- More than one Bambu account at a time.
- Other printer brands.
- Notifications (email, Teams, desktop) when a print finishes. A possible later addition.

## Open questions

Q1 is answered in P0, because the dashboard can't sign in without it. The rest are asked again in the decision phase (P5), once the dashboard has been in use.

| # | Question | Asked in | Decides |
|---|---|---|---|
| Q1 | Which region is the lab account in, and does it use an email code or an authenticator app? | P0 | The sign-in flow and MQTT broker the dashboard needs. |
| Q2 | Is Bambu Connect acceptable on the kiosk machine, and which operating system will the kiosk run? | P5 | Hand-off option A or B (D2). |
| Q3 | Are pause and stop needed from PrintLiner at all? If the cloud refuses them, the only way to offer them is through Bambu's own apps. | P5 | D3, and whether the detail view gets controls. |
| Q4 | Is a separate kiosk mode with an admin PIN still wanted, or is the dashboard on a shared screen enough? | P5 | D4, and whether P8 is built. |
| Q5 | How many printers, and which models, should the kiosk be checked against? | P5 | The kiosk layout and the P7 test prints. |
| Q6 | Should the kiosk show every printer on the account, or only a chosen set? The plan assumes a chosen set, set in Settings. | P5 | The kiosk settings in P8. |
