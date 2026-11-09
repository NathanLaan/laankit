//! A file's details for the Info pane: from the file system, the image
//! header, and EXIF data when there is some.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use exif::{Exif, In, Tag, Value};

use crate::media::{self, MediaKind};
use crate::scan::file_name;

/// What the Info pane shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileInfo {
    pub path: PathBuf,
    pub name: String,
    pub folder: PathBuf,
    pub size: u64,
    pub format: String,
    /// As shown: width and height after the EXIF orientation.
    pub dimensions: Option<(u32, u32)>,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub photo: Option<PhotoInfo>,
}

/// From EXIF. Each value is ready to show, units included.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhotoInfo {
    pub taken: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub focal_length: Option<String>,
    pub aperture: Option<String>,
    pub exposure: Option<String>,
    pub iso: Option<String>,
    pub flash: Option<String>,
    pub orientation: Option<String>,
    /// Latitude and longitude in degrees, south and west negative.
    pub location: Option<(f64, f64)>,
}

impl PhotoInfo {
    pub fn is_empty(&self) -> bool {
        *self == PhotoInfo::default()
    }
}

/// Reads `path`'s details. Only a file that can't be read at all is an
/// error; missing EXIF or an unreadable header leave those fields empty.
pub fn read(path: &Path) -> io::Result<FileInfo> {
    let meta = std::fs::metadata(path)?;
    let exif = if MediaKind::of(path).is_photo() {
        read_exif(path)
    } else {
        None
    };
    let orientation = exif
        .as_ref()
        .and_then(|e| e.get_field(Tag::Orientation, In::PRIMARY))
        .and_then(|f| f.value.get_uint(0));
    let dimensions = if MediaKind::of(path).is_photo() {
        image::image_dimensions(path).ok().map(|(w, h)| match orientation {
            Some(5..=8) => (h, w),
            _ => (w, h),
        })
    } else {
        None
    };
    Ok(FileInfo {
        path: path.to_owned(),
        name: file_name(path),
        folder: path.parent().map(Path::to_owned).unwrap_or_default(),
        size: meta.len(),
        format: media::format_name(path),
        dimensions,
        created: meta.created().ok(),
        modified: meta.modified().ok(),
        photo: exif.map(|e| photo_info(&e)).filter(|p| !p.is_empty()),
    })
}

fn read_exif(path: &Path) -> Option<Exif> {
    let file = File::open(path).ok()?;
    exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()
}

/// An ASCII field's text, without quotes and trailing NULs.
fn text(exif: &Exif, tag: Tag) -> Option<String> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    match &field.value {
        Value::Ascii(parts) => {
            let s = parts
                .iter()
                .map(|p| String::from_utf8_lossy(p).trim_matches(char::from(0)).trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            (!s.is_empty()).then_some(s)
        }
        _ => Some(field.display_value().to_string()),
    }
}

/// A field shown with its unit, as `kamadak-exif` formats it.
fn with_unit(exif: &Exif, tag: Tag) -> Option<String> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let s = field.display_value().with_unit(exif).to_string();
    (!s.is_empty()).then_some(s)
}

fn photo_info(exif: &Exif) -> PhotoInfo {
    let make = text(exif, Tag::Make);
    let model = text(exif, Tag::Model);
    let camera = match (make, model) {
        // "Canon" + "Canon EOS R6" shows once.
        (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => {
            Some(model)
        }
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (make, model) => make.or(model),
    };
    let taken = exif
        .get_field(Tag::DateTimeOriginal, In::PRIMARY)
        .or_else(|| exif.get_field(Tag::DateTime, In::PRIMARY))
        .map(|f| f.display_value().to_string());
    PhotoInfo {
        taken,
        camera,
        lens: text(exif, Tag::LensModel),
        focal_length: with_unit(exif, Tag::FocalLength),
        aperture: exif
            .get_field(Tag::FNumber, In::PRIMARY)
            .map(|f| f.display_value().to_string()),
        exposure: with_unit(exif, Tag::ExposureTime),
        iso: exif
            .get_field(Tag::PhotographicSensitivity, In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            .map(|n| format!("ISO {n}")),
        flash: exif
            .get_field(Tag::Flash, In::PRIMARY)
            .map(|f| f.display_value().to_string()),
        orientation: exif
            .get_field(Tag::Orientation, In::PRIMARY)
            .map(|f| f.display_value().to_string()),
        location: location(exif),
    }
}

fn degrees(exif: &Exif, tag: Tag, reference: Tag, negative: &str) -> Option<f64> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let Value::Rational(parts) = &field.value else {
        return None;
    };
    let part = |i: usize| parts.get(i).map(|r| r.to_f64()).filter(|v| v.is_finite());
    let value = part(0)? + part(1).unwrap_or(0.0) / 60.0 + part(2).unwrap_or(0.0) / 3600.0;
    let sign = match text(exif, reference) {
        Some(r) if r.eq_ignore_ascii_case(negative) => -1.0,
        _ => 1.0,
    };
    Some(sign * value)
}

fn location(exif: &Exif) -> Option<(f64, f64)> {
    let lat = degrees(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, "S")?;
    let lon = degrees(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, "W")?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

/// The JPEG thumbnail embedded in a photo's EXIF data, when it's at least
/// `min` pixels on its long side: a quick first image while the real
/// thumbnail is made. Also returns the photo's EXIF orientation, which
/// the embedded thumbnail doesn't have applied.
pub fn embedded_thumbnail(path: &Path, min: u32) -> Option<(Vec<u8>, Option<u32>)> {
    let exif = read_exif(path)?;
    let offset = exif
        .get_field(Tag::JPEGInterchangeFormat, In::THUMBNAIL)?
        .value
        .get_uint(0)? as usize;
    let len = exif
        .get_field(Tag::JPEGInterchangeFormatLength, In::THUMBNAIL)?
        .value
        .get_uint(0)? as usize;
    let bytes = exif.buf().get(offset..offset.checked_add(len)?)?.to_vec();
    let size = image::ImageReader::new(io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    let orientation = exif
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    (size.0.max(size.1) >= min).then_some((bytes, orientation))
}

/// An OpenStreetMap link to a location.
pub fn map_url((lat, lon): (f64, f64)) -> String {
    format!("https://www.openstreetmap.org/?mlat={lat:.6}&mlon={lon:.6}#map=15/{lat:.6}/{lon:.6}")
}

/// A size for people: "3.2 MB".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return if bytes == 1 { "1 byte".into() } else { format!("{bytes} bytes") };
    }
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if v < 10.0 {
        format!("{v:.1} {}", UNITS[unit])
    } else {
        format!("{v:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A JPEG with an EXIF block: orientation, make and model, and GPS.
    pub fn jpeg_with_exif(path: &Path, w: u32, h: u32, orientation: u16) {
        let img = image::RgbImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&img)
            .unwrap();
        let exif = exif_block(orientation);
        // APP1 after SOI.
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&exif);
        out.extend_from_slice(&jpeg[2..]);
        std::fs::write(path, out).unwrap();
    }

    fn exif_block(orientation: u16) -> Vec<u8> {
        use exif::experimental::Writer;
        use exif::{Field, Rational};
        let fields = [
            Field { tag: Tag::Orientation, ifd_num: In::PRIMARY, value: Value::Short(vec![orientation]) },
            Field { tag: Tag::Make, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"Canon".to_vec()]) },
            Field { tag: Tag::Model, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"Canon EOS R6".to_vec()]) },
            Field { tag: Tag::PhotographicSensitivity, ifd_num: In::PRIMARY, value: Value::Short(vec![400]) },
            Field { tag: Tag::GPSLatitudeRef, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"N".to_vec()]) },
            Field {
                tag: Tag::GPSLatitude,
                ifd_num: In::PRIMARY,
                value: Value::Rational(vec![Rational::from((49, 1)), Rational::from((15, 1)), Rational::from((0, 1))]),
            },
            Field { tag: Tag::GPSLongitudeRef, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"W".to_vec()]) },
            Field {
                tag: Tag::GPSLongitude,
                ifd_num: In::PRIMARY,
                value: Value::Rational(vec![Rational::from((123, 1)), Rational::from((6, 1)), Rational::from((0, 1))]),
            },
        ];
        let mut w = Writer::new();
        for f in &fields {
            w.push_field(f);
        }
        let mut buf = io::Cursor::new(Vec::new());
        w.write(&mut buf, false).unwrap();
        let mut out = b"Exif\0\0".to_vec();
        out.extend_from_slice(&buf.into_inner());
        out
    }

    #[test]
    fn reads_exif_and_turns_the_dimensions() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("IMG_1.jpg");
        jpeg_with_exif(&p, 40, 20, 6);
        let info = read(&p).unwrap();
        assert_eq!(info.dimensions, Some((20, 40)));
        assert_eq!(info.format, "JPEG");
        let photo = info.photo.unwrap();
        assert_eq!(photo.camera.as_deref(), Some("Canon EOS R6"));
        assert_eq!(photo.iso.as_deref(), Some("ISO 400"));
        let (lat, lon) = photo.location.unwrap();
        assert!((lat - 49.25).abs() < 1e-6 && (lon + 123.1).abs() < 1e-6);
    }

    #[test]
    fn other_files_and_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("notes.txt");
        std::fs::write(&p, "hello").unwrap();
        let info = read(&p).unwrap();
        assert_eq!(info.size, 5);
        assert!(info.photo.is_none() && info.dimensions.is_none());
        assert_eq!(human_size(1), "1 byte");
        assert_eq!(human_size(3_200_000), "3.2 MB");
        assert_eq!(human_size(42_000), "42 KB");
    }
}
