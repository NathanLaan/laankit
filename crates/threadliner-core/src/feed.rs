//! Feed fetching and parsing. Port of `apps/threadliner/src/main/feed-parser.js`, which used
//! rss-parser (xml2js) for RSS/Atom and hand-parsed JSON Feed.
//!
//! The XML side deliberately mirrors rss-parser's field semantics rather
//! than a generic feed model: entries are de-duplicated by `guid`, and the
//! guid of an already-stored entry must come out identical here or every
//! entry would re-appear as unread after switching builds.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use roxmltree::{Document, Node};
use serde_json::Value;

const ACCEPT: &str =
    "application/feed+json, application/json, application/rss+xml, application/atom+xml, application/xml, text/xml, */*";
const USER_AGENT: &str = "ThreadLiner/0.1.0";

/// A parsed feed, reduced to what ThreadLiner stores.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedFeed {
    pub title: Option<String>,
    pub link: Option<String>,
    pub description: Option<String>,
    pub items: Vec<Item>,
}

/// rss-parser's item fields that `normalizeEntries` reads.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Item {
    pub guid: Option<String>,
    pub id: Option<String>,
    pub title: Option<String>,
    pub link: Option<String>,
    pub content_encoded: Option<String>,
    pub content: Option<String>,
    pub summary: Option<String>,
    pub creator: Option<String>,
    pub author: Option<String>,
    pub iso_date: Option<String>,
    pub pub_date: Option<String>,
}

/// One entry ready for the data store (`normalizeEntries`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    pub guid: String,
    pub title: Option<String>,
    pub link: Option<String>,
    pub content: Option<String>,
    pub author: Option<String>,
    pub published_at: Option<String>,
}

pub async fn fetch_and_parse(url: &str) -> Result<ParsedFeed> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
        .build()?;
    let res = client.get(url).header("Accept", ACCEPT).send().await?;
    if !res.status().is_success() {
        bail!("Failed to fetch feed: HTTP {}", res.status().as_u16());
    }
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = res.text().await?;
    parse(&content_type, &body)
}

pub fn parse(content_type: &str, body: &str) -> Result<ParsedFeed> {
    if let Some(feed) = try_json_feed(content_type, body) {
        return Ok(feed);
    }
    parse_xml(body)
}

pub fn normalize_entries(items: &[Item]) -> Vec<NewEntry> {
    let first = |opts: &[&Option<String>]| {
        opts.iter()
            .find_map(|o| o.as_ref().filter(|s| !s.is_empty()).cloned())
    };
    items
        .iter()
        .map(|it| NewEntry {
            guid: first(&[&it.guid, &it.id, &it.link, &it.title])
                .unwrap_or_else(|| Utc::now().timestamp_millis().to_string()),
            title: first(&[&it.title]),
            link: first(&[&it.link]),
            content: first(&[&it.content_encoded, &it.content, &it.summary]),
            author: first(&[&it.creator, &it.author]),
            published_at: first(&[&it.iso_date, &it.pub_date]),
        })
        .collect()
}

// --- JSON Feed --------------------------------------------------------------

fn try_json_feed(content_type: &str, body: &str) -> Option<ParsedFeed> {
    let looks_json = content_type.contains("application/feed+json")
        || content_type.contains("application/json")
        || body.trim_start().starts_with('{');
    if !looks_json {
        return None;
    }
    let feed: Value = serde_json::from_str(body).ok()?;
    let version = feed.get("version")?.as_str()?;
    if !version.starts_with("https://jsonfeed.org/version/") {
        return None;
    }
    let s = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let items = feed
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let date = s(item, "date_published").or_else(|| s(item, "date_modified"));
                    Item {
                        id: item.get("id").and_then(|v| match v {
                            Value::String(s) if !s.is_empty() => Some(s.clone()),
                            Value::Number(n) => Some(n.to_string()),
                            _ => None,
                        }),
                        title: s(item, "title"),
                        link: s(item, "url").or_else(|| s(item, "external_url")),
                        content: s(item, "content_html").or_else(|| s(item, "content_text")),
                        summary: s(item, "summary"),
                        author: json_feed_author(item, &feed),
                        iso_date: date,
                        ..Default::default()
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ParsedFeed {
        title: s(&feed, "title"),
        description: s(&feed, "description"),
        link: s(&feed, "home_page_url"),
        items,
    })
}

fn json_feed_author(item: &Value, feed: &Value) -> Option<String> {
    let names = |v: &Value| -> Option<String> {
        if let Some(list) = v
            .get("authors")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
        {
            let joined = list
                .iter()
                .filter_map(|a| {
                    a.get("name")
                        .and_then(Value::as_str)
                        .filter(|n| !n.is_empty())
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Some(joined).filter(|s| !s.is_empty());
        }
        v.get("author")
            .and_then(|a| a.get("name"))
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
    };
    // v1.1 "authors" array / v1 "author" object, item first, then feed.
    let has_item_authors = item
        .get("authors")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty());
    if has_item_authors || item.get("author").and_then(|a| a.get("name")).is_some() {
        return names(item);
    }
    names(feed)
}

// --- RSS / Atom (rss-parser semantics) ----------------------------------------

/// xml2js keys elements by their name as written (`prefix:local`).
fn qname(node: Node) -> String {
    let tag = node.tag_name();
    match tag.namespace().and_then(|ns| node.lookup_prefix(ns)) {
        Some(p) if !p.is_empty() => format!("{p}:{}", tag.name()),
        _ => tag.name().to_string(),
    }
}

fn children<'a, 'i, 'n>(
    node: Node<'a, 'i>,
    name: &'n str,
) -> impl Iterator<Item = Node<'a, 'i>> + use<'a, 'i, 'n> {
    node.children()
        .filter(move |c| c.is_element() && qname(*c) == name)
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    children(node, name).next()
}

fn own_text(node: Node) -> String {
    node.children()
        .filter(|c| c.is_text())
        .filter_map(|c| c.text())
        .collect()
}

/// Inner markup of an element, verbatim from the source.
fn inner_xml<'i>(doc_src: &'i str, node: Node) -> &'i str {
    let (Some(first), Some(last)) = (node.first_child(), node.last_child()) else {
        return "";
    };
    &doc_src[first.range().start..last.range().end]
}

/// xml2js value collapsed the way copyFromXML / getContent read it: the
/// element's text when it has any (its `_`), otherwise — when it has child
/// elements — the children re-serialised inside a `<div>`.
fn value(src: &str, node: Node) -> Option<String> {
    let text = own_text(node);
    let has_children = node.children().any(|c| c.is_element());
    if !has_children {
        return Some(text).filter(|t| !t.is_empty());
    }
    if !text.trim().is_empty() {
        return Some(text);
    }
    let attrs: String = node
        .attributes()
        .map(|a| format!(" {}=\"{}\"", a.name(), escape_attr(a.value())))
        .collect();
    Some(format!("<div{attrs}>{}</div>", inner_xml(src, node)))
}

/// Plain field read (copyFromXML): text only.
fn field(src: &str, node: Node, name: &str) -> Option<String> {
    let c = child(node, name)?;
    let has_children = c.children().any(|n| n.is_element());
    if has_children {
        let t = own_text(c);
        return Some(t).filter(|t| !t.trim().is_empty());
    }
    value(src, c)
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

fn link_rel<'a>(links: &[Node<'a, 'a>], rel: &str, fallback: usize) -> Option<String> {
    links
        .iter()
        .find(|l| l.attribute("rel") == Some(rel))
        .or_else(|| links.get(fallback))
        .and_then(|l| l.attribute("href"))
        .map(str::to_string)
}

fn parse_xml(body: &str) -> Result<ParsedFeed> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = Document::parse_with_options(body, opts).map_err(|e| anyhow!("{e}"))?;
    let root = doc.root_element();
    let name = qname(root);
    let version = root.attribute("version").unwrap_or("");
    if name == "feed" {
        Ok(atom_feed(body, root))
    } else if name == "rss" && version.starts_with('2') {
        let channel = child(root, "channel").ok_or_else(|| anyhow!("RSS feed has no channel"))?;
        Ok(rss_feed(body, channel, children(channel, "item").collect()))
    } else if name == "rdf:RDF" {
        let channel = child(root, "channel").ok_or_else(|| anyhow!("RSS feed has no channel"))?;
        Ok(rss_feed(body, channel, children(root, "item").collect()))
    } else if name == "rss" && version.contains("0.9") {
        let channel = child(root, "channel").ok_or_else(|| anyhow!("RSS feed has no channel"))?;
        Ok(rss_feed(body, channel, children(channel, "item").collect()))
    } else {
        bail!("Feed not recognized as RSS 1 or 2.")
    }
}

fn rss_feed(src: &str, channel: Node, items: Vec<Node>) -> ParsedFeed {
    ParsedFeed {
        title: field(src, channel, "title").or_else(|| field(src, channel, "dc:title")),
        link: field(src, channel, "link"),
        description: field(src, channel, "description"),
        items: items.into_iter().map(|i| rss_item(src, i)).collect(),
    }
}

fn rss_item(src: &str, item: Node) -> Item {
    let pub_date = field(src, item, "pubDate");
    let date = field(src, item, "dc:date");
    Item {
        guid: child(item, "guid").map(own_text).filter(|g| !g.is_empty()),
        title: field(src, item, "title").or_else(|| field(src, item, "dc:title")),
        link: field(src, item, "link"),
        content_encoded: field(src, item, "content:encoded"),
        content: child(item, "description").and_then(|d| value(src, d)),
        summary: field(src, item, "summary"),
        creator: field(src, item, "dc:creator").or_else(|| field(src, item, "author")),
        author: field(src, item, "author"),
        iso_date: pub_date
            .as_deref()
            .or(date.as_deref())
            .and_then(js_iso_date),
        pub_date,
        id: None,
    }
}

fn atom_feed(src: &str, feed: Node) -> ParsedFeed {
    let links: Vec<Node> = children(feed, "link").collect();
    ParsedFeed {
        title: child(feed, "title").map(own_text).filter(|t| !t.is_empty()),
        link: link_rel(&links, "alternate", 0),
        description: None,
        items: children(feed, "entry").map(|e| atom_item(src, e)).collect(),
    }
}

fn atom_item(src: &str, entry: Node) -> Item {
    let links: Vec<Node> = children(entry, "link").collect();
    let date_of = |name: &str| {
        child(entry, name)
            .map(own_text)
            .filter(|d| !d.is_empty())
            .and_then(|d| js_iso_date(&d))
    };
    let pub_date = date_of("published").or_else(|| date_of("updated"));
    let author = child(entry, "author")
        .and_then(|a| child(a, "name"))
        .map(own_text)
        .filter(|n| !n.is_empty());
    Item {
        title: child(entry, "title")
            .map(own_text)
            .filter(|t| !t.is_empty()),
        link: if links.is_empty() {
            None
        } else {
            link_rel(&links, "alternate", 0)
        },
        content: child(entry, "content").and_then(|c| value(src, c)),
        summary: child(entry, "summary").and_then(|c| value(src, c)),
        id: child(entry, "id").map(own_text).filter(|i| !i.is_empty()),
        author,
        iso_date: pub_date.as_deref().and_then(js_iso_date),
        pub_date,
        ..Default::default()
    }
}

/// `new Date(s).toISOString()` for the date shapes feeds use; `None` where
/// JS would throw RangeError.
pub fn js_iso_date(s: &str) -> Option<String> {
    let s = s.trim();
    let fmt = |d: DateTime<Utc>| d.to_rfc3339_opts(SecondsFormat::Millis, true);
    if let Ok(d) = DateTime::parse_from_rfc2822(s) {
        return Some(fmt(d.with_timezone(&Utc)));
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(fmt(d.with_timezone(&Utc)));
    }
    // Date-only ISO strings are UTC in JS; date-times without a zone are local.
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(fmt(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?)));
    }
    for f in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(d) = NaiveDateTime::parse_from_str(s, f) {
            return Local
                .from_local_datetime(&d)
                .single()
                .map(|d| fmt(d.with_timezone(&Utc)));
        }
    }
    for f in [
        "%Y-%m-%d %H:%M:%S%z",
        "%Y-%m-%dT%H:%M:%S%z",
        "%a, %d %b %Y %H:%M:%S %Z",
    ] {
        if let Ok(d) = DateTime::parse_from_str(s, f) {
            return Some(fmt(d.with_timezone(&Utc)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:dc="http://purl.org/dc/elements/1.1/">
<channel>
  <title>Example Blog</title>
  <link>https://example.com/</link>
  <description>Posts</description>
  <item>
    <title>First</title>
    <link>https://example.com/1</link>
    <guid isPermaLink="false">id-1</guid>
    <pubDate>Mon, 06 Jan 2025 10:00:00 GMT</pubDate>
    <description><![CDATA[<p>Short</p>]]></description>
    <content:encoded><![CDATA[<p>Full &amp; long</p>]]></content:encoded>
    <dc:creator>Ann</dc:creator>
  </item>
  <item>
    <title>No guid</title>
    <link>https://example.com/2</link>
    <description>Plain &amp; simple</description>
    <author>bob@example.com (Bob)</author>
    <pubDate>not a date</pubDate>
  </item>
</channel>
</rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title type="text">Atom Site</title>
  <link rel="self" href="https://a.example/feed.xml"/>
  <link rel="alternate" href="https://a.example/"/>
  <entry>
    <title type="html">Post &lt;One&gt;</title>
    <link rel="alternate" href="https://a.example/p1"/>
    <id>urn:uuid:1</id>
    <updated>2025-02-03T04:05:06Z</updated>
    <author><name>Cat</name></author>
    <summary>Sum</summary>
    <content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>Hi</p></div></content>
  </entry>
</feed>"#;

    #[test]
    fn rss2_matches_rss_parser_fields() {
        let feed = parse("application/rss+xml", RSS).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Example Blog"));
        assert_eq!(feed.link.as_deref(), Some("https://example.com/"));
        let e = normalize_entries(&feed.items);
        assert_eq!(e[0].guid, "id-1");
        assert_eq!(e[0].content.as_deref(), Some("<p>Full &amp; long</p>"));
        assert_eq!(e[0].author.as_deref(), Some("Ann"));
        assert_eq!(
            e[0].published_at.as_deref(),
            Some("2025-01-06T10:00:00.000Z")
        );
        // No guid: falls back to the link; unparseable date stays raw.
        assert_eq!(e[1].guid, "https://example.com/2");
        assert_eq!(e[1].content.as_deref(), Some("Plain & simple"));
        assert_eq!(e[1].author.as_deref(), Some("bob@example.com (Bob)"));
        assert_eq!(e[1].published_at.as_deref(), Some("not a date"));
    }

    #[test]
    fn atom_matches_rss_parser_fields() {
        let feed = parse("application/atom+xml", ATOM).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Atom Site"));
        assert_eq!(feed.link.as_deref(), Some("https://a.example/"));
        let e = normalize_entries(&feed.items);
        assert_eq!(e[0].guid, "urn:uuid:1");
        assert_eq!(e[0].title.as_deref(), Some("Post <One>"));
        assert_eq!(e[0].link.as_deref(), Some("https://a.example/p1"));
        assert_eq!(e[0].author.as_deref(), Some("Cat"));
        assert_eq!(
            e[0].published_at.as_deref(),
            Some("2025-02-03T04:05:06.000Z")
        );
        assert_eq!(
            e[0].content.as_deref(),
            Some(
                r#"<div type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>Hi</p></div></div>"#
            )
        );
    }

    #[test]
    fn json_feed() {
        let body = r#"{"version":"https://jsonfeed.org/version/1.1","title":"J","home_page_url":"https://j.example/",
          "authors":[{"name":"Feed Author"}],
          "items":[{"id":"1","url":"https://j.example/1","content_html":"<b>x</b>","date_published":"2025-01-01T00:00:00Z"},
                   {"id":2,"title":"T","content_text":"y","authors":[{"name":"A"},{"name":"B"}]}]}"#;
        let feed = parse("application/feed+json", body).unwrap();
        assert_eq!(feed.title.as_deref(), Some("J"));
        let e = normalize_entries(&feed.items);
        assert_eq!(e[0].guid, "1");
        assert_eq!(e[0].author.as_deref(), Some("Feed Author"));
        assert_eq!(e[0].published_at.as_deref(), Some("2025-01-01T00:00:00Z"));
        assert_eq!(e[1].guid, "2");
        assert_eq!(e[1].author.as_deref(), Some("A, B"));
    }

    #[test]
    fn rss1_rdf() {
        let body = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns="http://purl.org/rss/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/">
          <channel rdf:about="x"><title>R1</title><link>https://r1/</link></channel>
          <item rdf:about="https://r1/a"><title>A</title><link>https://r1/a</link><dc:date>2024-05-05T01:02:03+02:00</dc:date></item>
        </rdf:RDF>"#;
        let feed = parse("text/xml", body).unwrap();
        assert_eq!(feed.title.as_deref(), Some("R1"));
        let e = normalize_entries(&feed.items);
        assert_eq!(e[0].guid, "https://r1/a");
        // dc:date feeds isoDate (setISODate reads item.date).
        assert_eq!(
            e[0].published_at.as_deref(),
            Some("2024-05-04T23:02:03.000Z")
        );
    }

    #[test]
    fn rejects_unknown_xml() {
        assert!(parse("text/xml", "<html/>").is_err());
    }
}

/// Differential check against the Electron parser. Not run by default:
///   FEED_DIFF_DIR=<dir with f<N>.body + h<N>.txt> cargo test -p threadliner-core dump_for_diff -- --ignored
/// writes <dir>/rust.json in the same shape as the rss-parser reference.
#[cfg(test)]
#[test]
#[ignore]
fn dump_for_diff() {
    let dir = std::path::PathBuf::from(std::env::var("FEED_DIFF_DIR").expect("FEED_DIFF_DIR"));
    let mut out = serde_json::Map::new();
    for i in 1.. {
        let Ok(body) = std::fs::read_to_string(dir.join(format!("f{i}.body"))) else {
            break;
        };
        let headers = std::fs::read_to_string(dir.join(format!("h{i}.txt"))).unwrap_or_default();
        let ct = headers
            .lines()
            .filter_map(|l| {
                l.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                    .map(|(_, v)| v.trim().to_string())
            })
            .next_back()
            .unwrap_or_default();
        let v = match parse(&ct, &body) {
            Ok(f) => serde_json::json!({
                "title": f.title, "link": f.link, "description": f.description,
                "entries": normalize_entries(&f.items).iter().map(|e| serde_json::json!({
                    "guid": e.guid, "title": e.title, "link": e.link, "content": e.content,
                    "author": e.author, "publishedAt": e.published_at,
                })).collect::<Vec<_>>(),
            }),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        out.insert(i.to_string(), v);
    }
    std::fs::write(
        dir.join("rust.json"),
        serde_json::to_string_pretty(&out).unwrap(),
    )
    .unwrap();
}
