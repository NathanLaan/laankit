//! Fills a throwaway ThreadLiner setup for screenshots and manual testing:
//! a git-backed data folder with the given feeds, plus a `config.json` under
//! `$XDG_CONFIG_HOME/ThreadLiner` pointing at it. Never point it at your real
//! config.
//!
//! ```bash
//! export XDG_CONFIG_HOME=$(mktemp -d)
//! cargo run -p threadliner-native --example threadliner_demo_data -- /tmp/tl-data \
//!   https://blog.rust-lang.org/feed.xml https://jvns.ca/atom.xml
//! cargo run -p threadliner-native
//! ```

use std::path::PathBuf;

use serde_json::json;
use threadliner_core::feed;
use threadliner_core::store::Store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .expect("usage: threadliner_demo_data <data dir> <feed url>..."),
    );
    if std::env::var_os("XDG_CONFIG_HOME").is_none() {
        anyhow::bail!("set XDG_CONFIG_HOME to a temp folder first");
    }
    std::fs::create_dir_all(&dir)?;
    let git =
        laankit_core::git::Git::new().with_default_identity("ThreadLiner", "threadliner@localhost");
    if !git.is_repo(&dir).await {
        git.init_repo(&dir, None).await?;
    }
    git.configure_user(&dir).await?;
    let mut store = Store::open(&dir)?;
    let mut first_tag = None;
    for url in args {
        match feed::fetch_and_parse(&url).await {
            Ok(parsed) => {
                let title = parsed
                    .title
                    .clone()
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| url.clone());
                let entries = feed::normalize_entries(&parsed.items);
                let f = store.add_feed(
                    &title,
                    &url,
                    parsed.link.clone(),
                    parsed.description.clone(),
                )?;
                let id = f["id"].as_str().unwrap_or("").to_string();
                let n = store.insert_entries(&id, &entries)?;
                println!("{title}: {n} entries");
                let tag = match &first_tag {
                    Some(t) => t,
                    None => {
                        let t = store.add_tag("Tech")?;
                        store.add_tag("Reading")?;
                        first_tag.insert(t["id"].as_str().unwrap_or("").to_string())
                    }
                };
                store.assign_tag(&id, tag)?;
            }
            Err(e) => eprintln!("{url}: {e}"),
        }
    }
    git.commit_all(&dir, "Demo data").await?;
    let config = laankit_core::paths::user_data_dir("ThreadLiner").join("config.json");
    laankit_core::json::write(&config, &json!({ "dataDir": dir, "remoteUrl": null }))?;
    println!("wrote {}", config.display());
    Ok(())
}
