//! Free and used space on the startup disk.

use serde::Serialize;
use std::ffi::CString;
use std::mem::MaybeUninit;

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub total: u64,
    pub free: u64,
    pub used: u64,
}

/// Space on the volume that holds your files.
///
/// On macOS that is the Data volume; `/` is only the read-only system volume,
/// so its numbers would look far too small.
pub fn disk_info() -> DiskInfo {
    ["/System/Volumes/Data", "/"]
        .iter()
        .find_map(|path| stat(path))
        .unwrap_or_default()
}

#[allow(clippy::unnecessary_cast)] // field widths differ between platforms
fn stat(path: &str) -> Option<DiskInfo> {
    let c_path = CString::new(path).ok()?;
    let mut raw = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `c_path` is a valid C string and `raw` is large enough for statfs.
    if unsafe { libc::statfs(c_path.as_ptr(), raw.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs returned 0, so it filled the struct.
    let s = unsafe { raw.assume_init() };
    let block = s.f_bsize as u64;
    let total = s.f_blocks as u64 * block;
    let free = s.f_bavail as u64 * block;
    Some(DiskInfo {
        total,
        free,
        used: total.saturating_sub(free),
    })
}
