//! Display metadata (title, author) and cover extraction. Port of
//! `apps/pageliner/src/main/metadata.js`: EPUB is parsed from its OPF manifest; PDF falls
//! back to the filename. Best-effort throughout — import never fails here.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use regex::Regex;

pub struct Cover {
    pub data: Vec<u8>,
    pub ext: String,
}

pub struct Metadata {
    pub title: String,
    pub author: Option<String>,
    pub cover: Option<Cover>,
}

pub fn title_from_filename(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

pub fn format_from_ext(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("epub") => "epub".into(),
        Some("pdf") => "pdf".into(),
        Some(other) if !other.is_empty() => other.into(),
        _ => "unknown".into(),
    }
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

fn ext_from_media_type(media_type: Option<&str>, href: Option<&str>) -> String {
    if let Some(ext) = href
        .and_then(|h| Path::new(h).extension())
        .and_then(|e| e.to_str())
    {
        return ext.to_lowercase();
    }
    let mt = media_type.unwrap_or("");
    for (needle, ext) in [
        ("jpeg", "jpg"),
        ("jpg", "jpg"),
        ("png", "png"),
        ("gif", "gif"),
        ("svg", "svg"),
        ("webp", "webp"),
    ] {
        if mt.contains(needle) {
            return ext.into();
        }
    }
    "png".into()
}

/// Resolve an OPF-relative href against the OPF's directory, normalised to
/// the forward-slash paths ZIP entries use.
fn resolve_zip_path(opf_path: &str, href: &str) -> String {
    let dir = match opf_path.rfind('/') {
        Some(i) => &opf_path[..i],
        None => "",
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in dir.split('/').chain(href.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let re = Regex::new(&format!(r#"(?i){name}="([^"]+)""#)).ok()?;
    re.captures(tag).and_then(|c| c.get(1)).map(|m| m.as_str())
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn extract_epub(path: &Path) -> Option<Metadata> {
    let mut zip = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
    let fallback = || Metadata {
        title: title_from_filename(path),
        author: None,
        cover: None,
    };

    let Some(container) = read_entry(&mut zip, "META-INF/container.xml") else {
        return Some(fallback());
    };
    let container = String::from_utf8_lossy(&container);
    let rootfile = Regex::new(r#"(?i)<rootfile[^>]*full-path="([^"]+)""#).unwrap();
    let Some(opf_path) = rootfile.captures(&container).map(|c| c[1].to_string()) else {
        return Some(fallback());
    };
    let Some(opf) = read_entry(&mut zip, &opf_path) else {
        return Some(fallback());
    };
    let opf = String::from_utf8_lossy(&opf).to_string();

    let dc = |tag: &str| {
        Regex::new(&format!(r"(?is)<dc:{tag}[^>]*>(.*?)</dc:{tag}>"))
            .unwrap()
            .captures(&opf)
            .map(|c| decode_entities(&c[1]))
            .filter(|s| !s.is_empty())
    };
    let title = dc("title").unwrap_or_else(|| title_from_filename(path));
    let author = dc("creator");

    // Cover: <meta name="cover" content="id"> → manifest item, else an item
    // flagged properties="cover-image".
    let mut cover_href = None;
    let mut cover_type = None;
    let meta_cover = Regex::new(r#"(?i)<meta[^>]*name="cover"[^>]*content="([^"]+)""#)
        .unwrap()
        .captures(&opf)
        .or_else(|| {
            Regex::new(r#"(?i)<meta[^>]*content="([^"]+)"[^>]*name="cover""#)
                .unwrap()
                .captures(&opf)
        })
        .map(|c| c[1].to_string());
    if let Some(id) = meta_cover {
        let item = Regex::new(&format!(
            r#"(?i)<item[^>]*id="{}"[^>]*>"#,
            regex::escape(&id)
        ))
        .unwrap();
        if let Some(m) = item.find(&opf) {
            cover_href = attr(m.as_str(), "href").map(str::to_string);
            cover_type = attr(m.as_str(), "media-type").map(str::to_string);
        }
    }
    if cover_href.is_none() {
        let item = Regex::new(r#"(?i)<item[^>]*properties="[^"]*cover-image[^"]*"[^>]*>"#).unwrap();
        if let Some(m) = item.find(&opf) {
            cover_href = attr(m.as_str(), "href").map(str::to_string);
            cover_type = attr(m.as_str(), "media-type").map(str::to_string);
        }
    }

    let cover = cover_href.and_then(|href| {
        let entry = resolve_zip_path(&opf_path, &decode_entities(&href));
        read_entry(&mut zip, &entry).map(|data| Cover {
            data,
            ext: ext_from_media_type(cover_type.as_deref(), Some(&href)),
        })
    });

    Some(Metadata {
        title,
        author,
        cover,
    })
}

pub fn extract(path: &Path) -> Metadata {
    let is_epub = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("epub"));
    if is_epub {
        if let Some(m) = extract_epub(path) {
            return m;
        }
    }
    Metadata {
        title: title_from_filename(path),
        author: None,
        cover: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_epub(dir: &Path) -> std::path::PathBuf {
        let p = dir.join("My Book.epub");
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("META-INF/container.xml", opts).unwrap();
        z.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        z.start_file("OEBPS/content.opf", opts).unwrap();
        z.write_all(br#"<package><metadata><dc:title>Dune &amp; Co</dc:title><dc:creator opf:role="aut">Frank Herbert</dc:creator><meta name="cover" content="cov"/></metadata><manifest><item id="cov" href="../images/c.jpg" media-type="image/jpeg"/></manifest></package>"#).unwrap();
        z.start_file("images/c.jpg", opts).unwrap();
        z.write_all(b"JPEGDATA").unwrap();
        z.finish().unwrap();
        p
    }

    #[test]
    fn extracts_epub_title_author_cover() {
        let dir = tempfile::tempdir().unwrap();
        let m = extract(&make_epub(dir.path()));
        assert_eq!(m.title, "Dune & Co");
        assert_eq!(m.author.as_deref(), Some("Frank Herbert"));
        let c = m.cover.unwrap();
        assert_eq!(c.data, b"JPEGDATA");
        assert_eq!(c.ext, "jpg");
    }

    #[test]
    fn pdf_falls_back_to_filename() {
        let m = extract(Path::new("/x/Some Paper.v2.pdf"));
        assert_eq!(m.title, "Some Paper.v2");
        assert_eq!(format_from_ext(Path::new("a.PDF")), "pdf");
    }

    #[test]
    fn zip_paths_resolve_like_posix_normalize() {
        assert_eq!(
            resolve_zip_path("OEBPS/content.opf", "../images/c.jpg"),
            "images/c.jpg"
        );
        assert_eq!(resolve_zip_path("content.opf", "./c.jpg"), "c.jpg");
    }
}
