//! The book model and the saved state, against the synthetic EPUB and pageliner-core.

mod common;

use marina_richtext::{Align, Block, Position};
use pageliner_epub::{Book, Location, NativeState};
use serde_json::json;

#[test]
fn opens_metadata_spine_toc_and_one_document() {
    for nav in [true, false] {
        let b = Book::from_bytes(common::synthetic_epub(nav)).unwrap();
        assert_eq!(b.title, "Synthetic Book");
        assert_eq!(b.id, "urn:uuid:pageliner-epub-test");
        assert_eq!(
            b.spine,
            ["OEBPS/text/chapter 1.xhtml", "OEBPS/text/ch2.xhtml"]
        );
        let toc: Vec<_> = b
            .toc
            .iter()
            .map(|t| (t.label.as_str(), t.target.as_str(), t.depth))
            .collect();
        assert_eq!(
            toc,
            [
                ("One", "OEBPS/text/chapter 1.xhtml", 0),
                ("One A", "OEBPS/text/chapter 1.xhtml#a", 1),
                ("Two", "OEBPS/text/ch2.xhtml", 0)
            ],
            "nav document: {nav}"
        );
        assert_eq!(b.chapter_lengths(), [5, 3]);
    }
    let b = Book::from_bytes(common::synthetic_epub(true)).unwrap();
    assert_eq!(b.author.as_deref(), Some("A. Writer"));
}

#[test]
fn css_links_images_and_locations() {
    let mut b = Book::from_bytes(common::synthetic_epub(true)).unwrap();
    let doc = b.document.clone();
    let Block::Text {
        align: Align::Center,
        spans,
        ..
    } = &doc.blocks[0]
    else {
        panic!("{:?}", doc.blocks[0])
    };
    assert!(spans[0].style.bold);
    let Block::Text { align, indent, .. } = &doc.blocks[1] else {
        panic!()
    };
    assert_eq!((*align, indent.first_line), (Align::Justify, 1.5));
    // An in-book link and a TOC target land in the second chapter.
    let Block::Text { spans, .. } = &doc.blocks[2] else {
        panic!()
    };
    let link = spans.iter().find_map(|s| s.link.clone()).unwrap();
    assert_eq!(link, "OEBPS/text/ch2.xhtml#end");
    assert_eq!(
        b.target(&link),
        Some(Position {
            block: 7,
            offset: 0
        })
    );
    assert_eq!(
        b.target("OEBPS/text/ch2.xhtml#nope"),
        Some(Position {
            block: 5,
            offset: 0
        })
    );
    assert_eq!(
        b.target("OEBPS/text/chapter 1.xhtml#a"),
        Some(Position {
            block: 2,
            offset: 0
        })
    );
    // The image reads from the archive with `/` paths on every platform.
    let Block::Image { src, .. } = &doc.blocks[3] else {
        panic!("{:?}", doc.blocks[3])
    };
    assert_eq!(src, "OEBPS/images/dot.png");
    assert!(b
        .resource(src)
        .is_some_and(|bytes| bytes.starts_with(b"\x89PNG")));

    // Global positions ↔ spine locations.
    let loc = b.location(Position {
        block: 6,
        offset: 4,
    });
    assert_eq!(
        loc,
        Location {
            spine: 1,
            block: 1,
            offset: 4
        }
    );
    assert_eq!(
        b.position(loc),
        Position {
            block: 6,
            offset: 4
        }
    );
    assert_eq!(
        b.position(Location {
            spine: 9,
            block: 0,
            offset: 0
        }),
        Position::default()
    );
    assert_eq!(b.label_at(0), "One");
    assert_eq!(b.label_at(3), "One A");
    assert_eq!(b.label_at(6), "Two");
}

#[test]
fn native_state_leaves_the_other_builds_keys_alone() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("Book.epub");
    std::fs::write(&src, common::synthetic_epub(true)).unwrap();
    let mut lib = pageliner_core::library::Library::open(dir.path().join("lib")).unwrap();
    let book = lib.add(&src).unwrap();

    // What the Tauri/Electron build wrote (epub.js CFIs).
    let theirs = json!({
        "cfi": "epubcfi(/6/4!/4/2/1:0)",
        "format": "epub",
        "bookmarks": [{ "id": "x", "kind": "bookmark", "cfi": "epubcfi(/6/4)", "label": "One" }],
        "highlights": [{ "id": "y", "kind": "highlight", "cfi": "epubcfi(/6/4,/1:0,/1:5)", "text": "Plain" }]
    });
    lib.set_state(&book.id, &theirs).unwrap();

    // The native reader starts fresh (positions are reset), then saves its own key only.
    let ours = NativeState::from_book_state(&lib.get_state(&book.id));
    assert_eq!(ours, NativeState::default());
    let mut ours = ours;
    ours.position = Some(Location {
        spine: 1,
        block: 2,
        offset: 3,
    });
    let patch = ours.patch();
    assert_eq!(
        patch.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["native"]
    );
    let merged = lib.set_state(&book.id, &patch).unwrap();
    for key in ["cfi", "format", "bookmarks", "highlights"] {
        assert_eq!(merged[key], theirs[key], "{key}");
    }
    assert_eq!(NativeState::from_book_state(&merged), ours);

    // And the other build's later save keeps ours.
    let merged = lib
        .set_state(&book.id, &json!({ "cfi": "epubcfi(/6/8)" }))
        .unwrap();
    assert_eq!(NativeState::from_book_state(&merged), ours);
}
