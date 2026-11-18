//! Free and used space on the startup disk.

use serde::Serialize;

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
#[cfg(unix)]
pub fn disk_info() -> DiskInfo {
    ["/System/Volumes/Data", "/"]
        .iter()
        .find_map(|path| stat(path))
        .unwrap_or_default()
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // field widths differ between platforms
fn stat(path: &str) -> Option<DiskInfo> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;

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

/// Space on the drive that holds the user folder, usually C:.
#[cfg(windows)]
pub fn disk_info() -> DiskInfo {
    use std::path::PathBuf;

    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"));
    let root = home.ancestors().last().unwrap_or(&home).to_path_buf();
    space(&root).unwrap_or_default()
}

#[cfg(windows)]
fn space(path: &std::path::Path) -> Option<DiskInfo> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is a NUL-terminated UTF-16 path and the out pointers are valid.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) };
    (ok != 0).then(|| DiskInfo {
        total,
        free,
        used: total.saturating_sub(free),
    })
}
