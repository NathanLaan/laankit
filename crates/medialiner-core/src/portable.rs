//! Windows only: drive letters, and cameras and phones that connect as a
//! media device (MTP or PTP), which have no drive letter and are read
//! through the Windows Portable Devices API.
//!
//! A portable device's COM objects belong to the thread that made them:
//! open a [`PortableSource`] on the thread that will use it.

use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use anyhow::{Context, Result};
use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::PortableDevices::{
    IEnumPortableDeviceObjectIDs, IPortableDevice, IPortableDeviceContent,
    IPortableDeviceKeyCollection, IPortableDeviceManager, IPortableDevicePropVariantCollection,
    IPortableDeviceProperties, IPortableDeviceResources, IPortableDeviceValues, PortableDeviceFTM,
    PortableDeviceKeyCollection, PortableDeviceManager, PortableDevicePropVariantCollection,
    PortableDeviceValues, PORTABLE_DEVICE_DELETE_NO_RECURSION, WPD_CLIENT_DESIRED_ACCESS,
    WPD_CLIENT_NAME, WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE, WPD_CONTENT_TYPE_FOLDER,
    WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT, WPD_DEVICE_OBJECT_ID, WPD_OBJECT_CONTENT_TYPE,
    WPD_OBJECT_DATE_MODIFIED, WPD_OBJECT_NAME, WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_SIZE,
    WPD_RESOURCE_DEFAULT,
};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IStream, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Variant::{VT_DATE, VT_LPWSTR};

use crate::devices::{Location, Volume};
use crate::import::{skipped_file, skipped_folder, Item, Source};
use crate::media::MediaKind;

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const SECURITY_IMPERSONATION: u32 = 0x0002_0000;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// The drives with a letter: the ones you plug in, and fixed drives
/// (which count only when they appear while the app is open).
pub fn drives() -> Vec<Volume> {
    let mut out = Vec::new();
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let root_w = wide(&root);
        // SAFETY: `root_w` is a NUL-terminated string that outlives the call.
        let kind = unsafe { GetDriveTypeW(PCWSTR(root_w.as_ptr())) };
        if kind != DRIVE_REMOVABLE && kind != DRIVE_FIXED {
            continue;
        }
        let mut label = [0u16; 261];
        let mut serial = 0u32;
        // SAFETY: the buffers outlive the call. A drive with nothing in it
        // (an empty card reader) fails here and is left out.
        let ok = unsafe {
            GetVolumeInformationW(
                PCWSTR(root_w.as_ptr()),
                Some(&mut label),
                Some(&mut serial),
                None,
                None,
                None,
            )
        };
        if ok.is_err() {
            continue;
        }
        let label = from_wide(&label);
        let name = if label.trim().is_empty() {
            format!("USB Drive ({letter}:)")
        } else {
            format!("{} ({letter}:)", label.trim())
        };
        out.push(Volume {
            name,
            key: format!("serial:{serial:08x}"),
            location: Location::Folder(root.into()),
            removable: kind == DRIVE_REMOVABLE,
        });
    }
    out
}

/// The free space where `path` is, in bytes.
pub fn free_space(path: &Path) -> Option<u64> {
    let w = wide(&path.to_string_lossy());
    let mut free = 0u64;
    // SAFETY: `w` is NUL-terminated and `free` outlives the call.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

fn com() {
    // SAFETY: initializing COM on this thread; an "already initialized"
    // result is fine, and the thread keeps COM for its life.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// Takes a string the system allocated, and frees it.
unsafe fn take_string(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller passes a NUL-terminated string allocated with
    // CoTaskMemAlloc, which is not used again.
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// A device id that is a drive under another name: it already has a drive
/// letter, so it's listed by [`drives`].
fn is_volume(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    id.contains("wpdbusenum") || id.starts_with(r"\\?\storage#")
}

/// The media devices plugged in, as `(device id, name)`.
pub fn devices() -> Vec<(String, String)> {
    com();
    let mut out = Vec::new();
    // SAFETY: plain COM calls with buffers that outlive each call.
    unsafe {
        let Ok(manager) = CoCreateInstance::<_, IPortableDeviceManager>(
            &PortableDeviceManager,
            None,
            CLSCTX_INPROC_SERVER,
        ) else {
            return out;
        };
        let _ = manager.RefreshDeviceList();
        let mut count = 0u32;
        if manager
            .GetDevices(std::ptr::null_mut(), &mut count)
            .is_err()
            || count == 0
        {
            return out;
        }
        let mut ids = vec![PWSTR::null(); count as usize];
        if manager.GetDevices(ids.as_mut_ptr(), &mut count).is_err() {
            return out;
        }
        for p in ids.into_iter().take(count as usize) {
            let id = take_string(p);
            if id.is_empty() || is_volume(&id) {
                continue;
            }
            let id_w = wide(&id);
            let mut len = 0u32;
            let _ = manager.GetDeviceFriendlyName(PCWSTR(id_w.as_ptr()), PWSTR::null(), &mut len);
            let mut name = String::new();
            if len > 0 {
                let mut buf = vec![0u16; len as usize + 1];
                if manager
                    .GetDeviceFriendlyName(PCWSTR(id_w.as_ptr()), PWSTR(buf.as_mut_ptr()), &mut len)
                    .is_ok()
                {
                    name = from_wide(&buf);
                }
            }
            if name.trim().is_empty() {
                name = "Camera".to_owned();
            }
            out.push((id, name));
        }
    }
    out
}

/// A media device's files.
pub struct PortableSource {
    device: IPortableDevice,
    content: IPortableDeviceContent,
    properties: IPortableDeviceProperties,
    resources: IPortableDeviceResources,
    keys: IPortableDeviceKeyCollection,
}

fn open_device(id: &str, access: u32) -> windows::core::Result<IPortableDevice> {
    let id_w = wide(id);
    let name = wide("MediaLiner");
    // SAFETY: plain COM calls; the strings outlive them.
    unsafe {
        let info: IPortableDeviceValues =
            CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
        info.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(name.as_ptr()))?;
        info.SetUnsignedIntegerValue(
            &WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE,
            SECURITY_IMPERSONATION,
        )?;
        info.SetUnsignedIntegerValue(&WPD_CLIENT_DESIRED_ACCESS, access)?;
        let device: IPortableDevice =
            CoCreateInstance(&PortableDeviceFTM, None, CLSCTX_INPROC_SERVER)?;
        device.Open(PCWSTR(id_w.as_ptr()), &info)?;
        Ok(device)
    }
}

impl PortableSource {
    /// Opens the device for reading and deleting, or for reading only when
    /// it refuses the first.
    pub fn open(id: &str) -> Result<Self> {
        com();
        let device = open_device(id, GENERIC_READ | GENERIC_WRITE)
            .or_else(|_| open_device(id, GENERIC_READ))
            .context("opening the device")?;
        // SAFETY: plain COM calls on the device just opened.
        unsafe {
            let content = device.Content()?;
            let properties = content.Properties()?;
            let resources = content.Transfer()?;
            let keys: IPortableDeviceKeyCollection =
                CoCreateInstance(&PortableDeviceKeyCollection, None, CLSCTX_INPROC_SERVER)?;
            for key in [
                &WPD_OBJECT_NAME,
                &WPD_OBJECT_ORIGINAL_FILE_NAME,
                &WPD_OBJECT_SIZE,
                &WPD_OBJECT_CONTENT_TYPE,
                &WPD_OBJECT_DATE_MODIFIED,
            ] {
                keys.Add(key)?;
            }
            Ok(Self {
                device,
                content,
                properties,
                resources,
                keys,
            })
        }
    }

    fn children(&self, parent: &[u16]) -> Vec<String> {
        let mut out = Vec::new();
        // SAFETY: `parent` is NUL-terminated; each id returned is taken
        // (and freed) once.
        unsafe {
            let Ok(list): windows::core::Result<IEnumPortableDeviceObjectIDs> =
                self.content.EnumObjects(0, PCWSTR(parent.as_ptr()), None)
            else {
                return out;
            };
            loop {
                let mut ids = [PWSTR::null(); 32];
                let mut fetched = 0u32;
                let hr = list.Next(&mut ids, &mut fetched);
                for p in ids.into_iter().take(fetched as usize) {
                    out.push(take_string(p));
                }
                if hr.is_err() || fetched == 0 {
                    break;
                }
            }
        }
        out
    }

    fn walk(
        &self,
        parent: &[u16],
        rel: &str,
        out: &mut Vec<Item>,
        cancel: &AtomicBool,
        found: &mut dyn FnMut(usize),
    ) {
        for id in self.children(parent) {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let id_w = wide(&id);
            // SAFETY: plain COM calls; `id_w` is NUL-terminated.
            let Ok(values) =
                (unsafe { self.properties.GetValues(PCWSTR(id_w.as_ptr()), &self.keys) })
            else {
                continue;
            };
            let name = string_value(&values, &WPD_OBJECT_ORIGINAL_FILE_NAME)
                .filter(|n| !n.is_empty())
                .or_else(|| string_value(&values, &WPD_OBJECT_NAME))
                .unwrap_or_default();
            // SAFETY: a plain COM call.
            let kind: GUID =
                unsafe { values.GetGuidValue(&WPD_OBJECT_CONTENT_TYPE) }.unwrap_or_default();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if kind == WPD_CONTENT_TYPE_FOLDER || kind == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT {
                if !skipped_folder(&name) {
                    self.walk(&id_w, &child_rel, out, cancel, found);
                }
                continue;
            }
            if name.is_empty() || skipped_file(&name) {
                continue;
            }
            // SAFETY: a plain COM call.
            let size =
                unsafe { values.GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE) }.unwrap_or(0);
            out.push(Item {
                id,
                kind: MediaKind::of(Path::new(&name)),
                name,
                rel: child_rel,
                size,
                modified: date_value(&values),
            });
            found(out.len());
        }
    }
}

fn string_value(
    values: &IPortableDeviceValues,
    key: &windows::Win32::Foundation::PROPERTYKEY,
) -> Option<String> {
    // SAFETY: the string returned is taken (and freed) once.
    unsafe { values.GetStringValue(key).ok().map(|p| take_string(p)) }
}

/// The object's modification date. Devices give it as a local date and
/// time, in days since the end of 1899.
fn date_value(values: &IPortableDeviceValues) -> Option<SystemTime> {
    use chrono::TimeZone;
    // SAFETY: the value is read by its type tag, then cleared.
    let days = unsafe {
        let mut value: PROPVARIANT = values.GetValue(&WPD_OBJECT_DATE_MODIFIED).ok()?;
        let days = if value.Anonymous.Anonymous.vt == VT_DATE {
            Some(value.Anonymous.Anonymous.Anonymous.date)
        } else {
            None
        };
        let _ = PropVariantClear(&mut value);
        days?
    };
    // 25569 days from the end of 1899 to the start of 1970.
    let secs = ((days - 25_569.0) * 86_400.0).round() as i64;
    let naive = chrono::DateTime::from_timestamp(secs, 0)?.naive_utc();
    let local = chrono::Local.from_local_datetime(&naive).earliest()?;
    Some(SystemTime::from(local))
}

struct StreamReader(IStream);

impl Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut read = 0u32;
        let len = buf.len().min(u32::MAX as usize) as u32;
        // SAFETY: `buf` has room for `len` bytes and `read` outlives the call.
        let hr = unsafe {
            self.0
                .Read(buf.as_mut_ptr() as *mut _, len, Some(&mut read))
        };
        if hr.is_err() {
            return Err(io::Error::other(format!("reading from the device: {hr:?}")));
        }
        Ok(read as usize)
    }
}

impl Source for PortableSource {
    fn list(&mut self, cancel: &AtomicBool, found: &mut dyn FnMut(usize)) -> Result<Vec<Item>> {
        let mut out = Vec::new();
        // SAFETY: the constant is a NUL-terminated string.
        let root: Vec<u16> = unsafe { WPD_DEVICE_OBJECT_ID.as_wide() }
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .collect();
        self.walk(&root, "", &mut out, cancel, found);
        Ok(out)
    }

    fn open(&mut self, item: &Item) -> io::Result<Box<dyn Read + '_>> {
        let id_w = wide(&item.id);
        let mut stream: Option<IStream> = None;
        let mut best = 0u32;
        // SAFETY: `id_w` is NUL-terminated; the out values outlive the call.
        unsafe {
            self.resources.GetStream(
                PCWSTR(id_w.as_ptr()),
                &WPD_RESOURCE_DEFAULT,
                STGM_READ.0,
                &mut best,
                &mut stream,
            )
        }
        .map_err(|e| io::Error::other(format!("opening {} on the device: {e}", item.name)))?;
        let stream = stream.ok_or_else(|| io::Error::other("the device gave no data"))?;
        Ok(Box::new(StreamReader(stream)))
    }

    fn delete(&mut self, item: &Item) -> io::Result<()> {
        let mut id_w = wide(&item.id);
        // SAFETY: the collection copies the value it's given, so the
        // string only has to outlive `Add`.
        unsafe {
            let ids: IPortableDevicePropVariantCollection = CoCreateInstance(
                &PortableDevicePropVariantCollection,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .map_err(io::Error::other)?;
            let mut value = PROPVARIANT::default();
            (*value.Anonymous.Anonymous).vt = VT_LPWSTR;
            (*value.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(id_w.as_mut_ptr());
            ids.Add(&value).map_err(io::Error::other)?;
            let mut results = None;
            self.content
                .Delete(
                    PORTABLE_DEVICE_DELETE_NO_RECURSION.0 as u32,
                    &ids,
                    &mut results,
                )
                .map_err(|e| io::Error::other(format!("the device refused: {e}")))
        }
    }

    fn present(&self) -> bool {
        // A device that has gone refuses to list its root.
        // SAFETY: the constant is a NUL-terminated string.
        unsafe {
            self.content
                .EnumObjects(0, WPD_DEVICE_OBJECT_ID, None)
                .is_ok()
        }
    }
}

impl Drop for PortableSource {
    fn drop(&mut self) {
        // SAFETY: closing the device this source opened.
        let _ = unsafe { self.device.Close() };
    }
}
