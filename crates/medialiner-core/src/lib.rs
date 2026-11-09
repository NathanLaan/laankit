//! MediaLiner's backend, with no UI framework.
//!
//! | Module    | What it does |
//! |-----------|--------------|
//! | `library` | The list of folders you added, and the rules for adding one inside or around another. |
//! | `scan`    | A folder's subfolders and files, sorted naturally, without hidden ones. |
//! | `media`   | Which files are photos (and, later, videos). |
//! | `natural` | Natural sorting: "Day 2" before "Day 10". |
//! | `thumbs`  | Decoding photos the right way up, thumbnails, and their cache on disk. |
//! | `info`    | A photo's details for the Info pane, from the file and its EXIF data. |
//! | `merge`   | Merging one folder into another: the plan, the moves, the journal, and undo. |
//! | `reveal`  | Showing a file in the system file manager. |

pub mod info;
pub mod library;
pub mod media;
pub mod merge;
pub mod natural;
pub mod reveal;
pub mod scan;
pub mod thumbs;

pub use library::{Added, Library};
pub use media::MediaKind;
pub use scan::FileEntry;
