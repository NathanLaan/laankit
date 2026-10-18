//! The article corpus: entries with content from well-known feeds, fetched once with
//! `threadliner_core::feed` and cached outside the repo (`SPIKE_ARTICLE_DIR`).

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use threadliner_core::feed;

/// Blogs, news, comics, a podcast and a YouTube channel: the range of HTML ThreadLiner shows.
const FEEDS: &[(&str, &str)] = &[
    ("Rust Blog", "https://blog.rust-lang.org/feed.xml"),
    (
        "Simon Willison",
        "https://simonwillison.net/atom/everything/",
    ),
    ("Julia Evans", "https://jvns.ca/atom.xml"),
    ("LWN", "https://lwn.net/headlines/rss"),
    (
        "Ars Technica",
        "https://feeds.arstechnica.com/arstechnica/index",
    ),
    ("The Verge", "https://www.theverge.com/rss/index.xml"),
    (
        "Smashing Magazine",
        "https://www.smashingmagazine.com/feed/",
    ),
    ("GitHub Blog", "https://github.blog/feed/"),
    ("Martin Fowler", "https://martinfowler.com/feed.atom"),
    ("xkcd", "https://xkcd.com/atom.xml"),
    ("Daring Fireball", "https://daringfireball.net/feeds/main"),
    ("Dan Luu", "https://danluu.com/atom.xml"),
    ("Kottke", "https://feeds.kottke.org/main"),
    ("Changelog (podcast)", "https://changelog.com/podcast/feed"),
    (
        "Fireship (YouTube)",
        "https://www.youtube.com/feeds/videos.xml?channel_id=UCsBjURrPoezykLs9EqgamOA",
    ),
    ("Cloudflare Blog", "https://blog.cloudflare.com/rss/"),
    (
        "Raspberry Pi News",
        "https://www.raspberrypi.com/news/feed/",
    ),
];
/// Entries kept per feed, so no single site dominates.
const PER_FEED: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Article {
    pub feed: String,
    pub title: String,
    pub link: Option<String>,
    pub content: String,
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("SPIKE_ARTICLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("marina-spike-article"))
}

fn cache_file() -> PathBuf {
    cache_dir().join("articles.json")
}

pub fn load() -> Result<Vec<Article>> {
    let path = cache_file();
    let json = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "No corpus at {}; run `spike-article fetch` first",
            path.display()
        )
    })?;
    Ok(serde_json::from_str(&json)?)
}

/// Fetches every feed and caches up to [`PER_FEED`] entries with content from each.
pub fn fetch() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let mut articles = Vec::new();
    for (name, url) in FEEDS {
        match rt.block_on(feed::fetch_and_parse(url)) {
            Ok(parsed) => {
                let entries = feed::normalize_entries(&parsed.items);
                let total = entries.len();
                let kept: Vec<Article> = entries
                    .into_iter()
                    .filter_map(|e| {
                        let content = e.content.filter(|c| !c.trim().is_empty())?;
                        Some(Article {
                            feed: name.to_string(),
                            title: e.title.unwrap_or_default(),
                            link: e.link,
                            content,
                        })
                    })
                    .take(PER_FEED)
                    .collect();
                log::info!("{name}: {total} entries, kept {}", kept.len());
                articles.extend(kept);
            }
            Err(e) => log::warn!("{name}: {e:#}"),
        }
    }
    std::fs::create_dir_all(cache_dir())?;
    std::fs::write(cache_file(), serde_json::to_string_pretty(&articles)?)?;
    log::info!(
        "Cached {} articles in {}",
        articles.len(),
        cache_file().display()
    );
    Ok(())
}
