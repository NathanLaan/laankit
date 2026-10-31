//! Background work for one open PDF: page renders, text extraction and
//! search, on worker threads that each parse their own copy of the document,
//! plus the page-texture cache, capped by bytes rather than pages (a page at
//! HiDPI fit-width is ~17 MB of RGBA, so 20 pages can reach 335 MB; see the
//! Phase 0 results).
//!
//! The UI replaces the render requests every frame with what it still needs
//! (nearest pages first), so pages scrolled past are never rendered. Text and
//! search requests are kept until done. Dropping [`Workers`] stops the threads.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

use egui::{ColorImage, TextureHandle, TextureOptions};
use hayro::hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro::RenderCache;

use super::doc;
use super::text::{self, PageText};

/// Render threads per open book.
const WORKERS: usize = 2;
/// Render scales are rounded to eighths, so small zoom changes reuse textures.
pub const SCALE_STEPS: f32 = 8.0;
/// Page textures kept, in bytes of RGBA (the pages on screen are never evicted).
pub const CACHE_BYTES: usize = 128 * 1024 * 1024;

/// A texture key: 0-based page index and render scale in eighths.
pub type Key = (usize, u32);

/// A finished piece of work.
pub enum Done {
    Image(Key, ColorImage),
    Text(usize, PageText),
    /// A page matching search `generation`.
    SearchHit {
        generation: u64,
        page: usize,
    },
    Searched {
        generation: u64,
    },
}

#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
    ready: Condvar,
    shutdown: AtomicBool,
    /// The current search; a worker drops a search once it's superseded.
    search_generation: AtomicU64,
}

#[derive(Default)]
struct QueueState {
    renders: VecDeque<Key>,
    /// Renders a worker has taken and not finished.
    rendering: HashSet<Key>,
    texts: VecDeque<usize>,
    search: Option<(u64, String)>,
}

enum Job {
    Render(Key),
    Text(usize),
    Search(u64, String),
}

/// The worker threads for one document.
pub struct Workers {
    queue: Arc<Queue>,
    rx: Receiver<Done>,
}

impl Workers {
    /// Starts the threads; each repaints `ctx` when it finishes something.
    pub fn start(bytes: Arc<Vec<u8>>, ctx: &egui::Context) -> Self {
        let queue = Arc::new(Queue::default());
        let (tx, rx) = channel();
        for i in 0..WORKERS {
            let (bytes, queue, tx, ctx) = (bytes.clone(), queue.clone(), tx.clone(), ctx.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("pageliner-pdf-{i}"))
                .spawn(move || work(bytes, queue, tx, ctx));
            if let Err(e) = spawned {
                log::error!("couldn't start a PDF worker: {e}");
            }
        }
        Self { queue, rx }
    }

    /// Everything finished since the last call.
    pub fn poll(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }

    /// Marks a render as finished (its texture is now in the cache).
    pub fn rendered(&self, key: Key) {
        self.queue
            .state
            .lock()
            .expect("queue")
            .rendering
            .remove(&key);
    }

    /// Replaces the pending renders with `wanted` (most urgent first),
    /// skipping what's cached or already being rendered.
    pub fn request_renders(
        &self,
        wanted: impl IntoIterator<Item = Key>,
        cached: impl Fn(&Key) -> bool,
    ) {
        let mut state = self.queue.state.lock().expect("queue");
        let QueueState {
            renders, rendering, ..
        } = &mut *state;
        renders.clear();
        renders.extend(
            wanted
                .into_iter()
                .filter(|k| !cached(k) && !rendering.contains(k)),
        );
        drop(state);
        self.queue.ready.notify_all();
    }

    /// Asks for a page's text layer.
    pub fn request_text(&self, page: usize) {
        self.queue
            .state
            .lock()
            .expect("queue")
            .texts
            .push_back(page);
        self.queue.ready.notify_all();
    }

    /// Starts a search for `query` (lower-cased), superseding any other.
    pub fn search(&self, generation: u64, query: String) {
        self.queue
            .search_generation
            .store(generation, Ordering::SeqCst);
        self.queue.state.lock().expect("queue").search = Some((generation, query));
        self.queue.ready.notify_all();
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.queue.shutdown.store(true, Ordering::SeqCst);
        self.queue
            .search_generation
            .store(u64::MAX, Ordering::SeqCst);
        self.queue.ready.notify_all();
    }
}

/// A worker: its own parsed copy of the PDF and caches.
fn work(bytes: Arc<Vec<u8>>, queue: Arc<Queue>, tx: Sender<Done>, ctx: egui::Context) {
    let Ok(pdf) = doc::open(bytes) else { return };
    let cache = RenderCache::new();
    let interp = InterpreterCache::new();
    let settings = InterpreterSettings::default();
    let pages = pdf.pages();
    loop {
        let job = {
            let mut state = queue.state.lock().expect("queue");
            loop {
                if queue.shutdown.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(page) = state.texts.pop_front() {
                    break Job::Text(page);
                }
                if let Some(key) = state.renders.pop_front() {
                    state.rendering.insert(key);
                    break Job::Render(key);
                }
                if let Some((generation, query)) = state.search.take() {
                    break Job::Search(generation, query);
                }
                state = queue.ready.wait(state).expect("queue");
            }
        };
        let send = |done: Done| {
            let ok = tx.send(done).is_ok();
            ctx.request_repaint();
            ok
        };
        let alive = match job {
            Job::Render((page, scale)) => match pages.get(page) {
                Some(p) => send(Done::Image(
                    (page, scale),
                    doc::render_page(p, &cache, &settings, scale as f32 / SCALE_STEPS),
                )),
                None => true,
            },
            Job::Text(page) => match pages.get(page) {
                Some(p) => send(Done::Text(page, text::extract(p, &interp, &settings))),
                None => true,
            },
            Job::Search(generation, query) => {
                let current = || queue.search_generation.load(Ordering::SeqCst) == generation;
                let mut alive = true;
                for (i, p) in pages.iter().enumerate() {
                    if !current() || !alive {
                        break;
                    }
                    let text = text::extract(p, &interp, &settings).text().to_lowercase();
                    if text.contains(&query) {
                        alive = send(Done::SearchHit {
                            generation,
                            page: i,
                        });
                    }
                }
                alive && send(Done::Searched { generation })
            }
        };
        if !alive {
            return;
        }
    }
}

/// Page textures, evicted least-recently-drawn first once over [`CACHE_BYTES`].
#[derive(Default)]
pub struct TextureCache {
    entries: HashMap<Key, Entry>,
    bytes: usize,
}

struct Entry {
    texture: TextureHandle,
    last_used: u64,
    bytes: usize,
}

impl TextureCache {
    pub fn insert(&mut self, ctx: &egui::Context, key: Key, image: ColorImage, frame: u64) {
        let bytes = image.width() * image.height() * 4;
        let texture = ctx.load_texture(
            format!("pdf-page-{}-{}", key.0, key.1),
            image,
            TextureOptions::LINEAR,
        );
        if let Some(old) = self.entries.insert(
            key,
            Entry {
                texture,
                last_used: frame,
                bytes,
            },
        ) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.entries.contains_key(key)
    }

    /// The texture for `key`, or else the closest scale of the same page
    /// already rendered (shown while the exact one is on its way).
    pub fn best(&mut self, key: Key, frame: u64) -> Option<&TextureHandle> {
        let found = if self.entries.contains_key(&key) {
            Some(key)
        } else {
            self.entries
                .keys()
                .filter(|k| k.0 == key.0)
                .min_by_key(|k| k.1.abs_diff(key.1))
                .copied()
        }?;
        let entry = self.entries.get_mut(&found)?;
        entry.last_used = frame;
        Some(&entry.texture)
    }

    /// Drops the least recently drawn textures (never this frame's) until the
    /// cache fits its budget.
    pub fn evict(&mut self, frame: u64, budget: usize) {
        while self.bytes > budget {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, e)| e.last_used < frame)
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k);
            let Some(key) = oldest else { break };
            if let Some(e) = self.entries.remove(&key) {
                self.bytes -= e.bytes;
            }
        }
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: usize, h: usize) -> ColorImage {
        ColorImage::new([w, h], vec![egui::Color32::WHITE; w * h])
    }

    #[test]
    fn evicts_by_bytes_but_keeps_this_frames_pages() {
        let ctx = egui::Context::default();
        let mut cache = TextureCache::default();
        // Each 100×100 page is 40,000 bytes.
        cache.insert(&ctx, (0, 8), image(100, 100), 1);
        cache.insert(&ctx, (1, 8), image(100, 100), 2);
        cache.insert(&ctx, (2, 8), image(100, 100), 3);
        assert_eq!(cache.bytes(), 120_000);
        cache.evict(3, 80_000);
        assert!(!cache.contains(&(0, 8)), "the oldest goes first");
        assert!(cache.contains(&(1, 8)) && cache.contains(&(2, 8)));
        // Everything drawn this frame stays, even over budget.
        cache.best((1, 8), 4);
        cache.best((2, 8), 4);
        cache.evict(4, 0);
        assert_eq!(cache.len(), 2);
        // A missing scale falls back to the nearest one of that page.
        assert!(cache.best((1, 12), 5).is_some());
        assert!(cache.best((7, 8), 5).is_none());
    }

    #[test]
    fn workers_render_extract_and_search() {
        let ctx = egui::Context::default();
        let bytes = Arc::new(super::super::fixtures::text_pdf(&[
            &["Hello world"],
            &["Second page"],
        ]));
        let workers = Workers::start(bytes, &ctx);
        workers.request_renders([(1, 8)], |_| false);
        workers.request_text(0);
        workers.search(1, "second".into());
        let (mut image, mut text, mut hits, mut done) = (None, None, Vec::new(), false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !(image.is_some() && text.is_some() && done) {
            assert!(std::time::Instant::now() < deadline, "workers timed out");
            for d in workers.poll() {
                match d {
                    Done::Image(key, img) => image = Some((key, img.size)),
                    Done::Text(page, t) => text = Some((page, t.text())),
                    Done::SearchHit {
                        generation: 1,
                        page,
                    } => hits.push(page),
                    Done::Searched { generation: 1 } => done = true,
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(image, Some(((1, 8), [612, 792])));
        assert_eq!(text, Some((0, "Hello world".to_string())));
        assert_eq!(hits, vec![1]);
    }
}
