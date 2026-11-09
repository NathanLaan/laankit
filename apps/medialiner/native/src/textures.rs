//! The pictures on the GPU: thumbnails, kept in a least-recently-used set
//! of about [`THUMB_TEXTURES`], and the preview with the one decoded ahead.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use egui::{ColorImage, TextureHandle, TextureOptions};

/// Thumbnails kept on the GPU.
pub const THUMB_TEXTURES: usize = 600;
/// Thumbnails uploaded per frame, so a burst doesn't stall one frame.
const UPLOADS_PER_FRAME: usize = 24;
/// Previews kept: the one showing and the next.
const PREVIEWS: usize = 2;

/// A thumbnail's state.
pub enum Thumb {
    Ready(TextureHandle),
    /// The file couldn't be decoded.
    Failed,
}

struct Entry {
    key: String,
    thumb: Thumb,
    /// The embedded EXIF thumbnail; the real one is on its way.
    quick: bool,
    used: u64,
}

struct Upload {
    path: PathBuf,
    key: String,
    image: Option<ColorImage>,
    quick: bool,
}

#[derive(Default)]
pub struct Textures {
    thumbs: HashMap<PathBuf, Entry>,
    uploads: VecDeque<Upload>,
    frame: u64,
    previews: Vec<(PathBuf, Result<TextureHandle, String>)>,
}

impl Textures {
    /// Uploads waiting thumbnails and frees the least recently used.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        for _ in 0..UPLOADS_PER_FRAME {
            let Some(u) = self.uploads.pop_front() else { break };
            // A quick thumbnail never replaces the real one.
            if u.quick && self.thumbs.get(&u.path).is_some_and(|e| e.key == u.key) {
                continue;
            }
            let thumb = match u.image {
                Some(img) => Thumb::Ready(ctx.load_texture(
                    format!("thumb:{}", u.path.display()),
                    img,
                    TextureOptions::LINEAR,
                )),
                None => Thumb::Failed,
            };
            self.thumbs.insert(
                u.path,
                Entry {
                    key: u.key,
                    thumb,
                    quick: u.quick,
                    used: self.frame,
                },
            );
        }
        if !self.uploads.is_empty() {
            ctx.request_repaint();
        }
        if self.thumbs.len() > THUMB_TEXTURES {
            let mut by_age: Vec<(u64, PathBuf)> =
                self.thumbs.iter().map(|(p, e)| (e.used, p.clone())).collect();
            by_age.sort();
            for (_, p) in by_age.into_iter().take(self.thumbs.len() - THUMB_TEXTURES) {
                self.thumbs.remove(&p);
            }
        }
    }

    pub fn push_thumb(&mut self, path: PathBuf, key: String, image: Option<ColorImage>, quick: bool) {
        self.uploads.push_back(Upload {
            path,
            key,
            image,
            quick,
        });
    }

    /// The thumbnail for `path` at `key`, marking it used.
    pub fn thumb(&mut self, path: &Path, key: &str) -> Option<&Thumb> {
        let frame = self.frame;
        let e = self.thumbs.get_mut(path).filter(|e| e.key == key)?;
        e.used = frame;
        Some(&e.thumb)
    }

    /// Whether `path` at `key` still has to be asked for.
    pub fn needs(&self, path: &Path, key: &str) -> bool {
        let have = self
            .thumbs
            .get(path)
            .is_some_and(|e| e.key == key && !e.quick);
        let coming = self.uploads.iter().any(|u| u.path == path && u.key == key && !u.quick)
            || self.thumbs.get(path).is_some_and(|e| e.key == key && e.quick);
        !have && !coming
    }

    pub fn set_preview(&mut self, ctx: &egui::Context, path: PathBuf, image: Result<ColorImage, String>) {
        let tex = image.map(|img| {
            ctx.load_texture(
                format!("preview:{}", path.display()),
                img,
                TextureOptions::LINEAR,
            )
        });
        self.previews.retain(|(p, _)| *p != path);
        self.previews.push((path, tex));
        if self.previews.len() > PREVIEWS + 2 {
            let _ = self.previews.remove(0);
        }
    }

    pub fn preview(&self, path: &Path) -> Option<&Result<TextureHandle, String>> {
        self.previews.iter().find(|(p, _)| p == path).map(|(_, t)| t)
    }

    /// Keeps only the previews of `keep`.
    pub fn keep_previews(&mut self, keep: &[&Path]) {
        self.previews.retain(|(p, _)| keep.contains(&p.as_path()));
    }

    pub fn has_preview(&self, path: &Path) -> bool {
        self.preview(path).is_some()
    }
}
