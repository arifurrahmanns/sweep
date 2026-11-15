//! Measuring how much space files really take, and how recently they changed.

use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

/// Bytes a file or folder occupies on disk, the way `du` counts them.
///
/// Counts allocated blocks rather than file length, so a sparse file such as
/// Docker's disk image reports what it really uses. A file hard-linked in
/// several places (pnpm does this a lot) is counted once.
pub fn disk_usage(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    let seen = Mutex::new(HashSet::new());
    if meta.is_dir() {
        allocated(&meta) + dir_usage(path, &seen)
    } else {
        allocated(&meta)
    }
}

fn allocated(meta: &fs::Metadata) -> u64 {
    meta.blocks() * 512
}

fn dir_usage(dir: &Path, seen: &Mutex<HashSet<(u64, u64)>>) -> u64 {
    let entries: Vec<fs::DirEntry> = match fs::read_dir(dir) {
        Ok(read) => read.filter_map(Result::ok).collect(),
        Err(_) => return 0,
    };
    entries
        .par_iter()
        .map(|entry| {
            // DirEntry::metadata does not follow symlinks.
            let Ok(meta) = entry.metadata() else {
                return 0;
            };
            if meta.is_dir() {
                allocated(&meta) + dir_usage(&entry.path(), seen)
            } else if meta.nlink() > 1 && !seen.lock().unwrap().insert((meta.dev(), meta.ino())) {
                0
            } else {
                allocated(&meta)
            }
        })
        .sum()
}

/// Seconds since 1970 when a file was last modified.
pub fn mtime(meta: &fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(since.as_secs() as i64)
}

/// Modification time of a path itself, without following symlinks.
pub fn path_mtime(path: &Path) -> Option<i64> {
    mtime(&fs::symlink_metadata(path).ok()?)
}

/// Newest modification time of any file under `dir`, looking at most `depth`
/// folders down and skipping folders whose name `skip` returns true for.
pub fn newest_file_mtime(dir: &Path, depth: usize, skip: &dyn Fn(&str) -> bool) -> Option<i64> {
    let mut newest: Option<i64> = None;
    let Ok(read) = fs::read_dir(dir) else {
        return None;
    };
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let time = if meta.is_dir() {
            if depth == 0 || skip(&entry.file_name().to_string_lossy()) {
                continue;
            }
            newest_file_mtime(&entry.path(), depth - 1, skip)
        } else {
            mtime(&meta)
        };
        newest = newest.max(time);
    }
    newest
}
