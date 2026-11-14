//! The drives and media devices plugged into the computer.
//!
//! | Platform | Drives | Media devices (MTP and PTP) |
//! |----------|--------|-----------------------------|
//! | Linux    | Mounts under `/media` and `/run/media` in `/proc/self/mountinfo`, where the desktop puts a drive you plug in | The folders GNOME mounts under `$XDG_RUNTIME_DIR/gvfs` |
//! | Windows  | Drive letters | Windows Portable Devices, see [`crate::portable`] |
//!
//! [`volumes`] is the list now; [`added`] is what's new since an earlier
//! list. MediaLiner never mounts a drive itself.

use std::path::{Path, PathBuf};

/// Where a device's files are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// A folder: a mounted drive, or a media device the desktop mounted.
    Folder(PathBuf),
    /// A Windows Portable Device, by its device id.
    Portable(String),
}

/// A drive or media device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// Its name for the dialog: the volume's label, or the device's name.
    pub name: String,
    /// The same for this device each time it's plugged in: the volume's
    /// UUID or serial number where there is one.
    pub key: String,
    pub location: Location,
    /// A drive you plug in (a card, a stick, a camera). A fixed drive
    /// counts only when it appears while the app is open.
    pub removable: bool,
}

impl Volume {
    /// A folder as a device (tests, and folders the desktop mounted).
    pub fn folder(path: &Path, name: &str, key: &str) -> Self {
        Self {
            name: name.to_owned(),
            key: key.to_owned(),
            location: Location::Folder(path.to_owned()),
            removable: true,
        }
    }

    /// The folder its files are in, for a drive.
    pub fn path(&self) -> Option<&Path> {
        match &self.location {
            Location::Folder(p) => Some(p),
            Location::Portable(_) => None,
        }
    }

    /// Whether it's still plugged in.
    pub fn present(&self) -> bool {
        match &self.location {
            Location::Folder(p) => p.is_dir(),
            Location::Portable(_) => volumes().iter().any(|v| v.location == self.location),
        }
    }
}

/// The volumes in `after` that aren't in `before`.
pub fn added(before: &[Volume], after: &[Volume]) -> Vec<Volume> {
    after
        .iter()
        .filter(|v| !before.iter().any(|b| b.location == v.location))
        .cloned()
        .collect()
}

/// Undoes mountinfo's escapes: `\040` is a space, `\011` a tab, `\134` a
/// backslash.
fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 4 <= bytes.len() {
            let digits = std::str::from_utf8(&bytes[i + 1..i + 4]).unwrap_or("");
            if let Ok(n) = u8::from_str_radix(digits, 8) {
                out.push(n);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The drives the desktop mounted, from `/proc/self/mountinfo`'s text: each
/// mount under `/media` or `/run/media`, as `(mount point, device)`.
pub fn parse_mountinfo(text: &str) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        // "36 35 98:0 /root /mount/point opts ... - fstype source superopts"
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let Some(mount) = left.split(' ').nth(4).map(unescape) else {
            continue;
        };
        let source = right.split(' ').nth(1).map(unescape).unwrap_or_default();
        let path = Path::new(&mount);
        let removable = [Path::new("/media"), Path::new("/run/media")]
            .iter()
            .any(|base| path.starts_with(base) && path != *base);
        if removable && !out.iter().any(|(p, _)| p == path) {
            out.push((path.to_owned(), source));
        }
    }
    out
}

/// A media device's name from its gvfs folder: `mtp:host=SONY_ILCE-6000_1234`
/// is "SONY ILCE-6000 1234".
pub fn gvfs_name(folder: &str) -> Option<String> {
    let (scheme, rest) = folder.split_once(':')?;
    if scheme != "mtp" && scheme != "gphoto2" {
        return None;
    }
    let host = rest.strip_prefix("host=").unwrap_or(rest);
    let name = unescape_percent(host).replace('_', " ");
    Some(if name.trim().is_empty() {
        "Camera".to_owned()
    } else {
        name.trim().to_owned()
    })
}

fn unescape_percent(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            let digits = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(n) = u8::from_str_radix(digits, 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The volumes plugged in now.
#[cfg(target_os = "linux")]
pub fn volumes() -> Vec<Volume> {
    use crate::scan::file_name;
    let mut out = Vec::new();
    let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    for (path, source) in parse_mountinfo(&text) {
        let name = file_name(&path);
        let key = uuid_of(&source).unwrap_or_else(|| format!("label:{name}"));
        out.push(Volume {
            name,
            key,
            location: Location::Folder(path),
            removable: true,
        });
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let gvfs = PathBuf::from(runtime).join("gvfs");
        if let Ok(rd) = std::fs::read_dir(&gvfs) {
            let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            dirs.sort();
            for dir in dirs {
                let folder = file_name(&dir);
                if let Some(name) = gvfs_name(&folder) {
                    out.push(Volume {
                        name,
                        key: folder,
                        location: Location::Folder(dir),
                        removable: true,
                    });
                }
            }
        }
    }
    out
}

/// The UUID of the file system on `device` (`/dev/sdb1`), from the links
/// in `/dev/disk/by-uuid`.
#[cfg(target_os = "linux")]
fn uuid_of(device: &str) -> Option<String> {
    let device = std::fs::canonicalize(device).ok()?;
    for entry in std::fs::read_dir("/dev/disk/by-uuid").ok()?.flatten() {
        if std::fs::canonicalize(entry.path()).ok().as_ref() == Some(&device) {
            return Some(format!("uuid:{}", entry.file_name().to_string_lossy()));
        }
    }
    None
}

/// The volumes plugged in now: drive letters, then media devices.
#[cfg(windows)]
pub fn volumes() -> Vec<Volume> {
    let mut out = crate::portable::drives();
    for (id, name) in crate::portable::devices() {
        out.push(Volume {
            name,
            key: format!("wpd:{id}"),
            location: Location::Portable(id),
            removable: true,
        });
    }
    out
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn volumes() -> Vec<Volume> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
25 30 0:23 / /proc rw,nosuid shared:13 - proc proc rw
30 1 254:1 / / rw,relatime shared:1 - ext4 /dev/mapper/vg-root rw
98 30 8:17 / /media/laan/SANDISK\\04032 rw,nosuid,nodev shared:60 - vfat /dev/sdb1 rw,uid=1000
99 30 8:33 / /run/media/laan/EOS_DIGITAL rw,nosuid,nodev shared:61 - exfat /dev/sdc1 rw
100 30 0:50 / /run/user/1000/gvfs rw,nosuid,nodev shared:62 - fuse.gvfsd-fuse gvfsd-fuse rw
101 30 8:49 / /media rw - tmpfs tmpfs rw
";

    #[test]
    fn mountinfo_lists_the_drives_the_desktop_mounted() {
        let drives = parse_mountinfo(MOUNTINFO);
        assert_eq!(
            drives,
            vec![
                (PathBuf::from("/media/laan/SANDISK 32"), "/dev/sdb1".into()),
                (
                    PathBuf::from("/run/media/laan/EOS_DIGITAL"),
                    "/dev/sdc1".into()
                ),
            ]
        );
    }

    #[test]
    fn media_devices_are_named_from_their_folders() {
        assert_eq!(
            gvfs_name("mtp:host=SONY_ILCE-6000_1234").as_deref(),
            Some("SONY ILCE-6000 1234")
        );
        assert_eq!(
            gvfs_name("gphoto2:host=%5Busb%3A001%2C004%5D").as_deref(),
            Some("[usb:001,004]")
        );
        assert_eq!(gvfs_name("smb-share:server=nas,share=photos"), None);
    }

    #[test]
    fn added_finds_the_new_volume() {
        let a = Volume::folder(Path::new("/media/u/A"), "A", "a");
        let b = Volume::folder(Path::new("/media/u/B"), "B", "b");
        assert_eq!(
            added(std::slice::from_ref(&a), &[a.clone(), b.clone()]),
            [b]
        );
        assert!(added(std::slice::from_ref(&a), std::slice::from_ref(&a)).is_empty());
    }
}
