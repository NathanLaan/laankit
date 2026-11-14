# PrintLiner

PrintLiner is a desktop dashboard for the Bambu Lab 3D printers on your Bambu account. It signs in through the Bambu Lab cloud, so the printers stay in their normal mode: no LAN-only mode, no Developer Mode, no access codes, and Bambu Studio and Bambu Handy keep working alongside it.

It is a pure-Rust desktop app built on [egui](https://github.com/emilk/egui) over the shared `laankit-*` crates, with no webview and no JavaScript. The shipped binary is `printliner`.

- Run, build, install: see the [root README](../../README.md).
- The native app in depth: see [`native/README.md`](native/README.md).
- How it fits together: [`ARCHITECTURE.md`](../../ARCHITECTURE.md).
- What's planned next, including the kiosk: [`docs/plans/plan-printliner.md`](../../docs/plans/plan-printliner.md).

## Features

- Sign in with your Bambu Lab email and password, and the emailed code when the account asks for one, or with an emailed code alone (for accounts made with Google or Apple sign-in, which have no password). Choose Global, Europe, or China for the account's region.
- A card per printer: its state (Idle, Preparing, Printing, Paused, Finished, Failed, Offline, or No signal), a progress ring, time left, layer, the job's name and plate image, the filament in use, and an alert count.
- Printer detail: the job with its progress, speed, and Wi-Fi signal; nozzle, bed, and chamber temperatures; the AMS trays with colour, material, amount left, and the tray in use; HMS alerts with their codes, severity, and a link to Bambu Lab's HMS wiki; and the cloud's last 50 tasks with their plate images, results, times, and grams.
- Trends: temperatures and progress over the last 24 hours or 7 days, from a sample PrintLiner records every minute while it's open.
- Display names and order: rename and reorder the printers for PrintLiner only (Settings → Printers). The names on your Bambu account stay as they are.
- Demo mode: `printliner --demo` shows three made-up printers, with no account.

PrintLiner only reads status. It doesn't start, pause, or stop prints.

## Your data

| Where | What |
|---|---|
| The system keychain | The Bambu Lab access token. Your password is never saved. |
| `~/.config/PrintLiner/settings.json` | Region, account email, where the token is kept, and the printers' display names and order. |
| `~/.config/PrintLiner/ui-preferences.json` | Window and view preferences. |
| `~/.config/PrintLiner/token.json` | The token, only on a machine with no keychain and only if you agree. Readable only by you. |
| `~/.local/share/printliner/history/` | The trend samples: `<serial>/<YYYY-MM>.jsonl`, kept for 90 days. |
| `~/.local/share/printliner/covers/` | Cached plate images from task history. |

The folders are the platform equivalents on Windows and macOS (see the root README's Uninstall section).
