//! A small EPUB built in memory (books used for manual testing can't be committed): a container,
//! an OPF, an EPUB 3 nav document or an EPUB 2 NCX, a stylesheet, two chapters and an image.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

const OPF_NAV: &str = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Synthetic Book</dc:title><dc:creator>A. Writer</dc:creator>
    <dc:identifier id="uid">urn:uuid:pageliner-epub-test</dc:identifier>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="css" href="style/book.css" media-type="text/css"/>
    <item id="c1" href="text/chapter%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="img" href="images/dot.png" media-type="image/png"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#;

const OPF_NCX: &str = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Synthetic Book</dc:title><dc:identifier id="uid">urn:uuid:pageliner-epub-test</dc:identifier>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="css" href="style/book.css" media-type="text/css"/>
    <item id="c1" href="text/chapter%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="img" href="images/dot.png" media-type="image/png"/>
  </manifest>
  <spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#;

const NAV: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Nav</title></head>
<body><nav epub:type="toc"><ol>
  <li><a href="text/chapter%201.xhtml">One</a><ol><li><a href="text/chapter%201.xhtml#a">One A</a></li></ol></li>
  <li><a href="text/ch2.xhtml">Two</a></li>
</ol></nav></body></html>"#;

const NCX: &str = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="n1" playOrder="1"><navLabel><text>One</text></navLabel><content src="text/chapter%201.xhtml"/>
      <navPoint id="n1a" playOrder="2"><navLabel><text>One A</text></navLabel><content src="text/chapter%201.xhtml#a"/></navPoint>
    </navPoint>
    <navPoint id="n2" playOrder="3"><navLabel><text>Two</text></navLabel><content src="text/ch2.xhtml"/></navPoint>
  </navMap>
</ncx>"#;

const CSS: &str =
    ".title { text-align: center; font-weight: bold } span.it { font-style: italic } \
                   p.body { text-indent: 1.5em; text-align: justify }";

const CH1: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>1</title><link rel="stylesheet" href="../style/book.css"/></head>
<body><h1 class="title">Chapter One</h1><p class="body">Plain <span class="it">italic</span> text in the first chapter.</p>
<p id="a" class="body">See <a href="ch2.xhtml#end">the end</a>, or <a href="https://example.com/">the web</a>.</p>
<div><img src="../images/dot.png" alt="dot"/></div>
<p class="body">The last paragraph of the first chapter.</p></body></html>"#;

const CH2: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>2</title></head>
<body><h2>Chapter Two</h2><p>Second chapter text.</p><p class="title" id="end">The End</p></body></html>"#;

/// The EPUB's bytes: EPUB 3 with a nav document (`nav = true`) or EPUB 2 with an NCX.
pub fn synthetic_epub(nav: bool) -> Vec<u8> {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(64, 48, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    let (opf, toc_name, toc): (&str, &str, &str) = if nav {
        (OPF_NAV, "OEBPS/nav.xhtml", NAV)
    } else {
        (OPF_NCX, "OEBPS/toc.ncx", NCX)
    };
    let files: [(&str, &[u8]); 7] = [
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        (toc_name, toc.as_bytes()),
        ("OEBPS/style/book.css", CSS.as_bytes()),
        ("OEBPS/text/chapter 1.xhtml", CH1.as_bytes()),
        ("OEBPS/text/ch2.xhtml", CH2.as_bytes()),
        ("OEBPS/images/dot.png", &png),
    ];
    for (name, data) in files {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
