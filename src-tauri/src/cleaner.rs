//! Deletes what a scan found, and nothing else.

use crate::disk::{disk_info, DiskInfo};
use crate::os::{command, Os};
use crate::scanner::{docker_running, old_entries, Action, Env, Item};
use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub id: String,
    pub title: String,
    /// Everything was removed.
    pub ok: bool,
    /// Some files were removed, but not all.
    pub partial: bool,
    pub message: Option<String>,
    /// What the user can do about a failure. A second line, if any, is a
    /// Terminal command.
    pub hint: Option<String>,
    /// Files are owned by root; `take_ownership` can fix that.
    pub needs_admin: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanReport {
    pub outcomes: Vec<Outcome>,
    pub disk_before: DiskInfo,
    pub disk_after: DiskInfo,
    /// Growth in free space while cleaning.
    pub freed: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

pub fn clean(env: &Env, items: &[Item], progress: &dyn Fn(CleanProgress)) -> CleanReport {
    let disk_before = disk_info();
    let mut outcomes = Vec::with_capacity(items.len());
    for (done, item) in items.iter().enumerate() {
        progress(CleanProgress {
            done,
            total: items.len(),
            current: item.title.clone(),
        });
        outcomes.push(clean_item(env, item));
    }
    progress(CleanProgress {
        done: items.len(),
        total: items.len(),
        current: String::new(),
    });
    let disk_after = disk_info();
    CleanReport {
        outcomes,
        disk_before,
        disk_after,
        freed: disk_after.free.saturating_sub(disk_before.free),
    }
}

fn clean_item(env: &Env, item: &Item) -> Outcome {
    let mut out = Outcome {
        id: item.id.clone(),
        title: item.title.clone(),
        ok: true,
        partial: false,
        message: None,
        hint: None,
        needs_admin: false,
    };
    let fail = |mut out: Outcome, message: String| {
        out.ok = false;
        out.message = Some(message);
        out
    };
    if !item.enabled {
        return fail(out, "This can't be cleaned right now.".to_string());
    }
    match &item.action {
        Action::Remove(paths) => {
            for path in paths {
                if let Err(why) = check_target_on(env.os, &env.home, path) {
                    return fail(out, why);
                }
            }
            let stats = RemoveStats::new(env.os);
            for path in paths {
                remove_tree(path, &stats);
            }
            stats.describe(&mut out, &paths[0]);
        }
        Action::DockerWipe { disk } => {
            if env.check_processes && docker_running(env.os) {
                return fail(out, "Docker is running. Quit Docker Desktop first.".to_string());
            }
            if let Err(why) = check_target_on(env.os, &env.home, disk) {
                return fail(out, why);
            }
            let stats = RemoveStats::new(env.os);
            remove_tree(disk, &stats);
            stats.describe(&mut out, disk);
        }
        Action::EmptyFolder { dir, older_than } => {
            let entries = old_entries(dir, *older_than);
            for path in &entries {
                if let Err(why) = check_target_on(env.os, &env.home, path) {
                    return fail(out, why);
                }
            }
            let stats = RemoveStats::new(env.os);
            entries.par_iter().for_each(|path| remove_tree(path, &stats));
            // Files still in use are expected here; skip them quietly.
            let skipped = stats.failed.load(Ordering::Relaxed);
            if skipped > 0 {
                out.message = Some(format!("Skipped {skipped} files in use."));
            }
        }
        Action::PnpmPrune { bin, store } => {
            // Point pnpm at the store; on its own it may pick one next to
            // the working folder.
            let store = store.display().to_string();
            run_tool(env, bin, &["store", "prune", "--store-dir", &store], &mut out)
        }
        Action::DockerPrune { bin } => run_tool(env, bin, &["system", "prune", "-a", "-f"], &mut out),
        Action::Nothing => return fail(out, "Sweep doesn't delete this.".to_string()),
    }
    out
}

/// Folders Sweep must never delete, relative to the home folder.
const PROTECTED_MAC: &[&str] = &[
    "Library",
    "Library/Caches",
    "Library/Application Support",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Developer",
    "Library/Preferences",
    "Documents",
    "Desktop",
    "Downloads",
    "Pictures",
    "Movies",
    "Music",
    "Public",
    "Applications",
    ".config",
    ".local",
    ".local/share",
    ".cache",
    ".Trash",
];

/// Folders Sweep must never delete anything inside.
const OFF_LIMITS_MAC: &[&str] = &[
    ".ssh",
    ".gnupg",
    "Library/Keychains",
    "Library/Mobile Documents",
    "Library/Mail",
    "Library/Messages",
    "Library/Photos",
    "Pictures/Photos Library.photoslibrary",
];

const PROTECTED_WINDOWS: &[&str] = &[
    "AppData",
    "AppData/Local",
    "AppData/LocalLow",
    "AppData/Roaming",
    "AppData/Local/Temp",
    "AppData/Local/Programs",
    "AppData/Local/Microsoft",
    "AppData/Roaming/Microsoft",
    "AppData/Local/Google",
    "AppData/Local/Mozilla",
    "AppData/Roaming/Mozilla",
    "Desktop",
    "Documents",
    "Downloads",
    "Pictures",
    "Videos",
    "Music",
    "OneDrive",
    "Favorites",
    "Contacts",
    "Links",
    "Saved Games",
    "Searches",
    ".config",
    ".local",
    ".cache",
];

const OFF_LIMITS_WINDOWS: &[&str] = &[
    ".ssh",
    ".gnupg",
    "AppData/Local/Packages",
    "AppData/Local/Microsoft/Windows",
    "AppData/Roaming/Microsoft/Windows",
    "AppData/Local/Microsoft/Credentials",
    "AppData/Roaming/Microsoft/Credentials",
    "AppData/Roaming/Microsoft/Protect",
    "AppData/Roaming/Microsoft/Crypto",
    "AppData/Roaming/Microsoft/SystemCertificates",
    "AppData/Local/Microsoft/Vault",
    "AppData/Roaming/Microsoft/Vault",
    "AppData/Roaming/Apple Computer/MobileSync",
    "Apple/MobileSync",
];

/// Last line of defence before deleting: the path must sit inside the home
/// folder and must not be, or be inside, anything important.
pub fn check_target(home: &Path, path: &Path) -> Result<(), String> {
    check_target_on(Os::current(), home, path)
}

/// `check_target` for a given system's folder layout. Windows compares names
/// without regard to case, as Windows itself does.
pub fn check_target_on(os: Os, home: &Path, path: &Path) -> Result<(), String> {
    let refuse = |why: &str| Err(format!("Refused to delete {}: {why}.", path.display()));
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return refuse("not a plain absolute path");
    }
    let Ok(rel) = path.strip_prefix(home) else {
        return refuse("it's outside your home folder");
    };
    if rel.as_os_str().is_empty() {
        return refuse("it's your home folder");
    }
    let (protected, off_limits) = match os {
        Os::Mac => (PROTECTED_MAC, OFF_LIMITS_MAC),
        Os::Windows => (PROTECTED_WINDOWS, OFF_LIMITS_WINDOWS),
    };
    let key = |text: &str| if os == Os::Windows { text.to_lowercase() } else { text.to_string() };
    let rel_key = key(&rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
    if protected.iter().any(|p| rel_key == key(p)) {
        return refuse("it's a protected folder");
    }
    if off_limits.iter().any(|p| {
        let p = key(p);
        rel_key == p || rel_key.starts_with(&format!("{p}/"))
    }) {
        return refuse("it holds personal data");
    }
    // A symlinked parent could point somewhere else entirely.
    if let (Some(parent), Ok(real_home)) = (path.parent(), home.canonicalize()) {
        match parent.canonicalize() {
            Ok(real_parent) if !real_parent.starts_with(&real_home) => {
                return refuse("its folder links outside your home folder");
            }
            _ => {}
        }
    }
    Ok(())
}

struct RemoveStats {
    os: Os,
    failed: AtomicUsize,
    /// Failures because the file or its folder belongs to another user.
    not_owned: AtomicUsize,
    /// Failures because the system blocked access to our own files.
    blocked: AtomicUsize,
    first_error: Mutex<Option<String>>,
}

/// Whether a file belongs to another user, such as root after an old `sudo`.
#[cfg(unix)]
fn owned_by_someone_else(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: getuid can't fail.
    let uid = unsafe { libc::getuid() };
    fs::symlink_metadata(path).map(|m| m.uid() != uid).unwrap_or(false)
}

#[cfg(not(unix))]
fn owned_by_someone_else(_path: &Path) -> bool {
    false
}

impl RemoveStats {
    fn new(os: Os) -> RemoveStats {
        RemoveStats {
            os,
            failed: AtomicUsize::new(0),
            not_owned: AtomicUsize::new(0),
            blocked: AtomicUsize::new(0),
            first_error: Mutex::new(None),
        }
    }

    fn record(&self, path: &Path, err: &io::Error) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        if err.kind() == ErrorKind::PermissionDenied {
            if owned_by_someone_else(path) || path.parent().is_some_and(owned_by_someone_else) {
                self.not_owned.fetch_add(1, Ordering::Relaxed);
            } else {
                self.blocked.fetch_add(1, Ordering::Relaxed);
            }
        }
        let mut first = self.first_error.lock().unwrap();
        if first.is_none() {
            *first = Some(format!("{}: {err}", path.display()));
        }
    }

    fn describe(&self, out: &mut Outcome, root: &Path) {
        let failed = self.failed.load(Ordering::Relaxed);
        if failed == 0 {
            return;
        }
        out.ok = false;
        out.partial = root.exists();
        let files = if failed == 1 { "1 file".to_string() } else { format!("{failed} files") };
        out.message = Some(format!("{files} couldn't be deleted."));
        out.needs_admin = self.not_owned.load(Ordering::Relaxed) > 0;
        out.hint = if out.needs_admin {
            Some("They're owned by the system (root), usually from an old sudo command.".to_string())
        } else if self.blocked.load(Ordering::Relaxed) > 0 {
            Some(match self.os {
                Os::Mac => "macOS blocked access. Quit the app using these files, or give Sweep Full Disk Access.",
                Os::Windows => "Windows blocked access. Close the app using these files, then try again.",
            }.to_string())
        } else {
            self.first_error.lock().unwrap().clone()
        };
    }
}

/// Deletes a file or folder tree, carrying on past files it can't delete.
/// Symlinks are removed, never followed.
fn remove_tree(path: &Path, stats: &RemoveStats) {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == ErrorKind::NotFound => return,
        Err(err) => return stats.record(path, &err),
    };
    if meta.is_dir() {
        match fs::read_dir(path) {
            Ok(read) => {
                let children: Vec<_> = read.filter_map(Result::ok).map(|e| e.path()).collect();
                children.par_iter().for_each(|child| remove_tree(child, stats));
            }
            Err(err) => return stats.record(path, &err),
        }
        if let Err(err) = writable_retry(path, &meta, |p| fs::remove_dir(p)) {
            // A folder that still has files in it was already reported above.
            if !matches!(err.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) {
                stats.record(path, &err);
            }
        }
    } else if let Err(err) = remove_entry(path, &meta) {
        if err.kind() != ErrorKind::NotFound {
            stats.record(path, &err);
        }
    }
}

/// Deletes a file or a link. On Windows a link to a folder, including the
/// junctions pnpm makes, is deleted as a folder: just the link, never what it
/// points to.
fn remove_entry(path: &Path, meta: &fs::Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if meta.file_type().is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    writable_retry(path, meta, |p| fs::remove_file(p))
}

/// Runs a delete. On Windows, files marked read-only (common in `.git` and
/// `node_modules`) refuse to be deleted, so the flag is cleared and the delete
/// tried once more.
fn writable_retry(path: &Path, meta: &fs::Metadata, delete: impl Fn(&Path) -> io::Result<()>) -> io::Result<()> {
    match delete(path) {
        #[cfg(windows)]
        Err(err) if err.kind() == ErrorKind::PermissionDenied && meta.permissions().readonly() => {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)] // Windows: only clears the read-only flag
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions).map_err(|_| err)?;
            delete(path)
        }
        result => {
            let _ = meta;
            result
        }
    }
}

fn run_tool(env: &Env, bin: &Path, args: &[&str], out: &mut Outcome) {
    let last_line = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .map(str::to_string)
    };
    // Apps opened from Finder start in "/", which is read-only.
    let result = command(bin)
        .args(args)
        .current_dir(&env.home)
        .env("PATH", env.search_path())
        .output();
    match result {
        Ok(result) if result.status.success() => out.message = last_line(&result.stdout),
        Ok(result) => {
            out.ok = false;
            out.message = last_line(&result.stderr)
                .or_else(|| last_line(&result.stdout))
                .or_else(|| Some(format!("{} stopped with {}", bin.display(), result.status)));
        }
        Err(err) => {
            out.ok = false;
            out.message = Some(format!("Couldn't run {}: {err}", bin.display()));
        }
    }
}

/// Makes the user's account the owner of an item's files again, through the
/// standard macOS admin password prompt. An ordinary app can't delete files
/// that an old `sudo` left owned by root; after this it can.
pub fn take_ownership(env: &Env, item: &Item) -> Result<(), String> {
    if env.os != Os::Mac {
        return Err("This fix is only for macOS.".to_string());
    }
    let Action::Remove(paths) = &item.action else {
        return Err("This item can't be fixed this way.".to_string());
    };
    for path in paths {
        check_target_on(env.os, &env.home, path)?;
    }
    let owner = current_owner();
    // Paths are passed as arguments, never pasted into the script.
    let script = [
        "on run argv",
        "set targets to \"\"",
        "repeat with p in rest of argv",
        "set targets to targets & \" \" & quoted form of (p as text)",
        "end repeat",
        "do shell script \"/usr/sbin/chown -R \" & item 1 of argv & targets with prompt \"Sweep needs your password to delete files owned by the system.\" with administrator privileges",
        "end run",
    ];
    let output = command("/usr/bin/osascript")
        .args(script.iter().flat_map(|line| ["-e", *line]))
        .arg(&owner)
        .args(paths)
        .output()
        .map_err(|e| format!("Couldn't ask for your password: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let error = String::from_utf8_lossy(&output.stderr);
    if error.contains("-128") {
        Err("Cancelled.".to_string())
    } else {
        Err(error.trim().to_string())
    }
}

/// "uid:gid" of the user running Sweep.
#[cfg(unix)]
fn current_owner() -> String {
    // SAFETY: getuid and getgid can't fail.
    unsafe { format!("{}:{}", libc::getuid(), libc::getgid()) }
}

#[cfg(not(unix))]
fn current_owner() -> String {
    String::new()
}
