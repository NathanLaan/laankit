//! Decoding photos the right way up, thumbnails, and the thumbnail cache.
//!
//! A thumbnail is the photo turned by its EXIF orientation and scaled to
//! [`THUMB_SIZE`] on its long side. The cache keeps them as files named by
//! a hash of the photo's path, size, and modification time, so a changed
//! photo gets a new thumbnail and the old one ages out. Showing a cached
//! thumbnail updates its file's time; [`Cache::clean`] removes the ones not
//! shown for a while.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use image::{DynamicImage, ImageDecoder, ImageReader, RgbaImage};

/// A thumbnail's long side, in pixels.
pub const THUMB_SIZE: u32 = 320;
/// The largest preview, in pixels on a side (GPU texture limits).
pub const PREVIEW_MAX: u32 = 8192;

/// Decodes `path`, turned by its EXIF orientation, and scaled down (never
/// up) to fit `max` pixels on its long side.
pub fn decode(path: &Path, max: u32) -> Result<RgbaImage> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .with_guessed_format()?;
    let mut decoder = reader
        .into_decoder()
        .with_context(|| format!("reading {}", path.display()))?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder)
        .with_context(|| format!("decoding {}", path.display()))?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(fit(img, max).into_rgba8())
}

/// The photo's embedded EXIF thumbnail, turned the right way up, when it's
/// at least 160 pixels on its long side: a quick first image while the
/// real thumbnail is made.
pub fn quick(path: &Path) -> Option<RgbaImage> {
    let (bytes, orientation) = crate::info::embedded_thumbnail(path, 160)?;
    let mut img = image::load_from_memory(&bytes).ok()?;
    if let Some(o) = orientation.and_then(|o| image::metadata::Orientation::from_exif(o as u8)) {
        img.apply_orientation(o);
    }
    Some(fit(img, THUMB_SIZE).into_rgba8())
}

fn fit(img: DynamicImage, max: u32) -> DynamicImage {
    if img.width().max(img.height()) <= max {
        img
    } else if max <= THUMB_SIZE {
        img.thumbnail(max, max)
    } else {
        img.resize(max, max, image::imageops::FilterType::Triangle)
    }
}

/// The thumbnail cache folder.
#[derive(Clone, Debug)]
pub struct Cache {
    pub dir: PathBuf,
}

/// A photo's cache key: a hash of its path, size, and modification time.
pub fn key(path: &Path, size: u64, modified: Option<SystemTime>) -> String {
    // FNV-1a: stable across builds and platforms, unlike std's hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    eat(path.to_string_lossy().as_bytes());
    eat(&size.to_le_bytes());
    let nanos = modified
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    eat(&nanos.to_le_bytes());
    format!("{h:016x}")
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn file(&self, key: &str, ext: &str) -> PathBuf {
        self.dir.join(&key[..2]).join(format!("{key}.{ext}"))
    }

    /// A cached thumbnail, if there is one.
    pub fn load(&self, key: &str) -> Option<RgbaImage> {
        for ext in ["jpg", "png"] {
            let file = self.file(key, ext);
            if let Ok(img) = image::open(&file) {
                touch(&file);
                return Some(img.into_rgba8());
            }
        }
        None
    }

    /// Saves a thumbnail: JPEG, or PNG when it has transparency.
    pub fn store(&self, key: &str, img: &RgbaImage) -> Result<()> {
        let alpha = img.pixels().any(|p| p.0[3] < 255);
        let file = self.file(key, if alpha { "png" } else { "jpg" });
        fs::create_dir_all(file.parent().unwrap_or(&self.dir))?;
        let tmp = file.with_extension("tmp");
        if alpha {
            img.save_with_format(&tmp, image::ImageFormat::Png)?;
        } else {
            let rgb = DynamicImage::ImageRgba8(img.clone()).into_rgb8();
            let mut out = io::BufWriter::new(fs::File::create(&tmp)?);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&rgb)?;
        }
        fs::rename(&tmp, &file)?;
        Ok(())
    }

    /// The photo's thumbnail: from the cache, or made and cached.
    pub fn thumbnail(
        &self,
        path: &Path,
        size: u64,
        modified: Option<SystemTime>,
    ) -> Result<RgbaImage> {
        let key = key(path, size, modified);
        if let Some(img) = self.load(&key) {
            return Ok(img);
        }
        let img = decode(path, THUMB_SIZE)?;
        if let Err(e) = self.store(&key, &img) {
            log::warn!("couldn't cache a thumbnail: {e:#}");
        }
        Ok(img)
    }

    fn files(&self) -> Vec<(PathBuf, fs::Metadata)> {
        let mut out = Vec::new();
        let Ok(dirs) = fs::read_dir(&self.dir) else {
            return out;
        };
        for d in dirs.flatten() {
            let Ok(files) = fs::read_dir(d.path()) else {
                continue;
            };
            for f in files.flatten() {
                if let Ok(m) = f.metadata() {
                    if m.is_file() {
                        out.push((f.path(), m));
                    }
                }
            }
        }
        out
    }

    /// The cache's size in bytes, and its number of thumbnails.
    pub fn usage(&self) -> (u64, usize) {
        let files = self.files();
        (files.iter().map(|(_, m)| m.len()).sum(), files.len())
    }

    /// Removes every thumbnail.
    pub fn clear(&self) -> io::Result<()> {
        match fs::remove_dir_all(&self.dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Removes thumbnails not shown for `age`. Returns how many.
    pub fn clean(&self, age: Duration) -> usize {
        let Some(cutoff) = SystemTime::now().checked_sub(age) else {
            return 0;
        };
        let mut n = 0;
        for (file, meta) in self.files() {
            if meta.modified().is_ok_and(|m| m < cutoff) && fs::remove_file(&file).is_ok() {
                n += 1;
            }
        }
        n
    }
}

fn touch(file: &Path) {
    if let Ok(f) = fs::File::options().write(true).open(file) {
        let _ = f.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::tests::jpeg_with_exif;

    #[test]
    fn turns_by_orientation_and_scales_down() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.jpg");
        jpeg_with_exif(&p, 800, 400, 6);
        let img = decode(&p, THUMB_SIZE).unwrap();
        assert_eq!(img.dimensions(), (160, 320));
        // Orientation 6 turns it a quarter clockwise: red (left) goes on top.
        let top = img.get_pixel(80, 10).0;
        assert!(top[0] > 200 && top[2] < 60, "{top:?}");
        let small = tmp.path().join("small.png");
        image::RgbaImage::new(10, 5).save(&small).unwrap();
        assert_eq!(decode(&small, THUMB_SIZE).unwrap().dimensions(), (10, 5));
        assert!(decode(&tmp.path().join("missing.jpg"), 10).is_err());
    }

    #[test]
    fn caches_by_path_size_and_time_and_cleans() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.jpg");
        jpeg_with_exif(&p, 64, 32, 1);
        let cache = Cache::new(tmp.path().join("thumbs"));
        let m = fs::metadata(&p).unwrap();
        let t = Some(m.modified().unwrap());
        let img = cache.thumbnail(&p, m.len(), t).unwrap();
        assert_eq!(img.dimensions(), (64, 32));
        assert_eq!(cache.usage().1, 1);
        assert!(cache.load(&key(&p, m.len(), t)).is_some());
        // A changed file has a new key.
        assert_ne!(key(&p, m.len(), t), key(&p, m.len() + 1, t));
        assert_eq!(cache.clean(Duration::from_secs(3600)), 0);
        assert_eq!(cache.clean(Duration::ZERO), 1);
        cache.clear().unwrap();
        assert_eq!(cache.usage(), (0, 0));
    }
}
