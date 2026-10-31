//! The library as the UI shows it (`stores/library.svelte.js`): sort modes,
//! the format filter and the counts, over `pageliner_core`'s books.

use std::cmp::Ordering;

pub use pageliner_core::library::Book;

/// `sortMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortMode {
    /// `addedAt-desc`: recently added first.
    #[default]
    AddedDesc,
    /// `title-asc`.
    TitleAsc,
    /// `author-asc`: books without an author last.
    AuthorAsc,
}

impl SortMode {
    /// The `<select>` options: (id, label).
    pub const OPTIONS: [(&'static str, &'static str); 3] = [
        ("addedAt-desc", "Recently added"),
        ("title-asc", "Title (A–Z)"),
        ("author-asc", "Author (A–Z)"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            SortMode::AddedDesc => "addedAt-desc",
            SortMode::TitleAsc => "title-asc",
            SortMode::AuthorAsc => "author-asc",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "title-asc" => SortMode::TitleAsc,
            "author-asc" => SortMode::AuthorAsc,
            _ => SortMode::AddedDesc,
        }
    }
}

/// `formatFilter`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormatFilter {
    #[default]
    All,
    Epub,
    Pdf,
}

impl FormatFilter {
    pub const ALL: [FormatFilter; 3] = [FormatFilter::All, FormatFilter::Epub, FormatFilter::Pdf];

    pub fn label(self) -> &'static str {
        match self {
            FormatFilter::All => "All Books",
            FormatFilter::Epub => "EPUB",
            FormatFilter::Pdf => "PDF",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            FormatFilter::All => marina_egui::icons::LAYER_GROUP,
            FormatFilter::Epub => marina_egui::icons::BOOK,
            FormatFilter::Pdf => marina_egui::icons::FILE_PDF,
        }
    }

    fn accepts(self, book: &Book) -> bool {
        match self {
            FormatFilter::All => true,
            FormatFilter::Epub => book.format == "epub",
            FormatFilter::Pdf => book.format == "pdf",
        }
    }
}

/// `counts`: all, EPUB and PDF.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: usize,
    pub epub: usize,
    pub pdf: usize,
}

impl Counts {
    pub fn of(books: &[Book]) -> Self {
        let mut c = Counts {
            all: books.len(),
            ..Default::default()
        };
        for b in books {
            match b.format.as_str() {
                "epub" => c.epub += 1,
                "pdf" => c.pdf += 1,
                _ => {}
            }
        }
        c
    }

    pub fn get(&self, filter: FormatFilter) -> usize {
        match filter {
            FormatFilter::All => self.all,
            FormatFilter::Epub => self.epub,
            FormatFilter::Pdf => self.pdf,
        }
    }
}

/// `visibleBooks`: the books sorted by `sort` and narrowed to `filter`.
pub fn visible_books(books: &[Book], sort: SortMode, filter: FormatFilter) -> Vec<&Book> {
    let mut list: Vec<&Book> = books.iter().filter(|b| filter.accepts(b)).collect();
    match sort {
        SortMode::AddedDesc => list.sort_by(|a, b| b.added_at.cmp(&a.added_at)),
        SortMode::TitleAsc => list.sort_by(|a, b| locale_cmp(&a.title, &b.title)),
        SortMode::AuthorAsc => list.sort_by(|a, b| {
            let key = |b: &Book| b.author.clone().unwrap_or_else(|| "~".into());
            locale_cmp(&key(a), &key(b))
        }),
    }
    list
}

/// Close to `String.prototype.localeCompare`: case-insensitive first, then by case.
fn locale_cmp(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

/// `book.format` as the badge shows it.
pub fn format_icon(format: &str) -> &'static str {
    if format == "pdf" {
        marina_egui::icons::FILE_PDF
    } else {
        marina_egui::icons::BOOK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn book(id: &str, title: &str, author: Option<&str>, format: &str, added: &str) -> Book {
        Book {
            id: id.into(),
            title: title.into(),
            author: author.map(str::to_string),
            format: format.into(),
            file: format!("{id}.{format}"),
            cover: None,
            tags: Vec::new(),
            added_at: added.into(),
            extra: Map::new(),
        }
    }

    fn ids(list: Vec<&Book>) -> Vec<&str> {
        list.into_iter().map(|b| b.id.as_str()).collect()
    }

    #[test]
    fn sorts_and_filters_like_the_store() {
        let books = vec![
            book("a", "zebra", Some("Ann"), "pdf", "2026-01-01T00:00:00.000Z"),
            book("b", "Apple", None, "epub", "2026-03-01T00:00:00.000Z"),
            book(
                "c",
                "mango",
                Some("bob"),
                "epub",
                "2026-02-01T00:00:00.000Z",
            ),
        ];
        assert_eq!(
            ids(visible_books(
                &books,
                SortMode::AddedDesc,
                FormatFilter::All
            )),
            ["b", "c", "a"]
        );
        assert_eq!(
            ids(visible_books(&books, SortMode::TitleAsc, FormatFilter::All)),
            ["b", "c", "a"]
        );
        assert_eq!(
            ids(visible_books(
                &books,
                SortMode::AuthorAsc,
                FormatFilter::All
            )),
            ["a", "c", "b"]
        );
        assert_eq!(
            ids(visible_books(
                &books,
                SortMode::TitleAsc,
                FormatFilter::Epub
            )),
            ["b", "c"]
        );
        assert_eq!(
            Counts::of(&books),
            Counts {
                all: 3,
                epub: 2,
                pdf: 1
            }
        );
        assert_eq!(
            SortMode::from_id(SortMode::AuthorAsc.id()),
            SortMode::AuthorAsc
        );
    }
}
