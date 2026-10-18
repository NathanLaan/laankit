//! Where images and links come from. ThreadLiner resolves against the web ([`WebResolver`]);
//! PageLiner resolves inside the book's archive ([`BundleResolver`]).
//!
//! Images are handed to egui as URIs and decoded by `egui_extras`' loaders on a background thread
//! (`egui_extras::install_image_loaders`, which the app calls once), so neither fetching nor
//! decoding runs on the UI thread. Bundle resources are read when first shown and registered as
//! `bytes://` URIs.

/// What to do with a clicked link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// Report it to the app as [`crate::Action::Open`].
    Open(String),
    /// Scroll to an anchor in the current document (`#note-3`).
    Anchor(String),
    /// Do nothing.
    Ignore,
}

/// Supplies images and decides what links do.
pub trait Resolver {
    /// An egui image URI for an image `src` (already resolved against the document's base), or
    /// `None` to show its alt text instead.
    fn image(&mut self, ctx: &egui::Context, src: &str) -> Option<String>;

    /// What a click on `target` does. By default `#anchor` links scroll the document and
    /// everything else is reported to the app.
    fn link(&mut self, target: &str) -> LinkAction {
        match target.strip_prefix('#') {
            Some(anchor) => LinkAction::Anchor(anchor.to_string()),
            None => LinkAction::Open(target.to_string()),
        }
    }
}

/// Articles: `http(s)` images go to egui_extras' HTTP loader, and `data:` images are decoded and
/// registered as bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct WebResolver;

impl Resolver for WebResolver {
    fn image(&mut self, ctx: &egui::Context, src: &str) -> Option<String> {
        if src.starts_with("http://") || src.starts_with("https://") {
            return Some(src.to_string());
        }
        data_uri(ctx, src)
    }
}

/// Registers a `data:…;base64,` image as bytes and returns its URI.
pub fn data_uri(ctx: &egui::Context, src: &str) -> Option<String> {
    use base64::Engine;
    let (meta, data) = src.strip_prefix("data:")?.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    let uri = format!("bytes://data/{:x}", fnv1a(src));
    if ctx.try_load_bytes(&uri).is_err() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .ok()?;
        ctx.include_bytes(uri.clone(), bytes);
    }
    Some(uri)
}

/// Images from a bundle (an EPUB's zip): `read` returns a resource's bytes by its path. Links
/// inside a book are archive paths (`OEBPS/ch2.xhtml#sec1`), reported for the app to navigate.
pub struct BundleResolver<F> {
    /// Distinguishes bundles in egui's byte cache (e.g. the book's id).
    scope: String,
    read: F,
}

impl<F: FnMut(&str) -> Option<Vec<u8>>> BundleResolver<F> {
    pub fn new(scope: impl Into<String>, read: F) -> Self {
        Self {
            scope: scope.into(),
            read,
        }
    }
}

impl<F: FnMut(&str) -> Option<Vec<u8>>> Resolver for BundleResolver<F> {
    fn image(&mut self, ctx: &egui::Context, src: &str) -> Option<String> {
        if src.starts_with("data:") {
            return data_uri(ctx, src);
        }
        // The extension stays last so egui_extras picks the right decoder.
        let uri = format!("bytes://{}/{src}", self.scope);
        if ctx.try_load_bytes(&uri).is_err() {
            ctx.include_bytes(uri.clone(), (self.read)(src)?);
        }
        Some(uri)
    }
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_links() {
        let mut r = WebResolver;
        assert_eq!(r.link("#fn1"), LinkAction::Anchor("fn1".into()));
        assert_eq!(
            r.link("https://a.example/"),
            LinkAction::Open("https://a.example/".into())
        );
    }

    #[test]
    fn data_and_bundle_images() {
        let ctx = egui::Context::default();
        let mut web = WebResolver;
        assert_eq!(
            web.image(&ctx, "https://a.example/x.png").as_deref(),
            Some("https://a.example/x.png")
        );
        let uri = web
            .image(&ctx, "data:image/png;base64,iVBORw0KGgo=")
            .unwrap();
        assert!(ctx.try_load_bytes(&uri).is_ok());
        assert_eq!(web.image(&ctx, "data:text/plain,hi"), None);

        let mut reads = 0;
        let mut bundle = BundleResolver::new("book", |p: &str| {
            reads += 1;
            (p == "OEBPS/cover.jpg").then(|| vec![1, 2, 3])
        });
        let uri = bundle.image(&ctx, "OEBPS/cover.jpg").unwrap();
        assert_eq!(uri, "bytes://book/OEBPS/cover.jpg");
        assert!(bundle.image(&ctx, "OEBPS/cover.jpg").is_some());
        assert_eq!(bundle.image(&ctx, "missing.png"), None);
        drop(bundle);
        // The second request hit egui's byte cache.
        assert_eq!(reads, 2);
    }
}
