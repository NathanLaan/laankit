//! PrintLiner's backend, with no UI framework: the Bambu Lab cloud.
//!
//! | Module    | What it does |
//! |-----------|--------------|
//! | `region`  | The cloud regions and their REST and MQTT hosts. |
//! | `cloud`   | The REST client: sign in, email code, devices, task history, cover images. |
//! | `mqtt`    | The cloud MQTT session: subscribe to each printer's reports, ask for full status. |
//! | `status`  | Merges partial reports into a full state and maps it to [`status::PrinterStatus`]. |
//! | `hms`     | HMS codes: display code, severity, wiki link. |
//! | `ams`     | AMS units and trays. |
//! | `history` | 60-second status samples, one JSON Lines file per printer per month. |
//! | `secrets` | The access token, in the OS keychain or (when asked) a private file. |
//! | `demo`    | Three made-up printers, for `--demo` and the UI tests. |
//!
//! PrintLiner only reads status: the one message it publishes is `pushall`.

pub mod ams;
pub mod cloud;
pub mod demo;
pub mod history;
pub mod hms;
pub mod mqtt;
pub mod region;
pub mod secrets;
pub mod status;
mod values;

pub use cloud::{Client, CloudError, Device, Login, Task, Token};
pub use region::Region;
pub use status::{GcodeState, PrinterStatus};
