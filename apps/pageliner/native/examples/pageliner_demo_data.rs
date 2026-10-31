//! Fills a throwaway PageLiner library for screenshots and manual testing:
//! imports the given EPUB and PDF files into `$XDG_CONFIG_HOME/PageLiner/library`.
//! Never point it at your real config (on Linux it refuses to run without
//! `XDG_CONFIG_HOME`).
//!
//! ```bash
//! export XDG_CONFIG_HOME=$(mktemp -d)
//! cargo run -p pageliner-native --example pageliner_demo_data -- ~/Documents/books/*.pdf ~/Documents/books/*.epub
//! cargo run -p pageliner-native
//! ```

use std::path::PathBuf;

use pageliner_core::library::Library;

fn main() -> anyhow::Result<()> {
    if cfg!(all(unix, not(target_os = "macos"))) && std::env::var_os("XDG_CONFIG_HOME").is_none() {
        anyhow::bail!("set XDG_CONFIG_HOME to a temp folder first");
    }
    let dir = marina_core::paths::user_data_dir("PageLiner").join("library");
    let mut library = Library::open(dir.clone())?;
    for arg in std::env::args().skip(1) {
        let path = PathBuf::from(&arg);
        match library.add(&path) {
            Ok(book) => println!("{} ({}): {}", book.title, book.format, book.id),
            Err(e) => eprintln!("{arg}: {e}"),
        }
    }
    println!("library: {}", dir.display());
    Ok(())
}
