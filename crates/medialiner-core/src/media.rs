//! What kind of file a path is, by its extension. A photo is confirmed by
//! reading its header when it's decoded; one that fails shows as broken.

use std::path::Path;

/// A file's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// Shown in the grid, the preview, and the Info pane.
    Photo,
    /// Recognised but not shown yet: listed with the other files.
    Video,
    Other,
}

/// The photo formats MediaLiner decodes.
pub const PHOTO_EXTENSIONS: [&str; 9] = [
    "jpg", "jpeg", "jpe", "png", "webp", "gif", "bmp", "tif", "tiff",
];

/// Video formats, recognised for later.
pub const VIDEO_EXTENSIONS: [&str; 8] = ["mp4", "m4v", "mov", "mkv", "avi", "webm", "mts", "3gp"];

impl MediaKind {
    pub fn of(path: &Path) -> Self {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return MediaKind::Other;
        };
        let ext = ext.to_ascii_lowercase();
        if PHOTO_EXTENSIONS.contains(&ext.as_str()) {
            MediaKind::Photo
        } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            MediaKind::Video
        } else {
            MediaKind::Other
        }
    }

    pub fn is_photo(self) -> bool {
        self == MediaKind::Photo
    }
}

/// The format's name for the Info pane, from the extension.
pub fn format_name(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "jpe" => "JPEG".into(),
        "tif" | "tiff" => "TIFF".into(),
        "" => "File".into(),
        other => other.to_ascii_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(MediaKind::of(Path::new("a/IMG.JPG")), MediaKind::Photo);
        assert_eq!(MediaKind::of(Path::new("a/b.tiff")), MediaKind::Photo);
        assert_eq!(MediaKind::of(Path::new("clip.MOV")), MediaKind::Video);
        assert_eq!(MediaKind::of(Path::new("photo.heic")), MediaKind::Other);
        assert_eq!(MediaKind::of(Path::new("README")), MediaKind::Other);
        assert_eq!(format_name(Path::new("x.jpeg")), "JPEG");
        assert_eq!(format_name(Path::new("x.webp")), "WEBP");
    }
}
