//! Deletes what a scan found, and nothing else.

use crate::disk::{disk_info, DiskInfo};
use crate::scanner::{docker_running, Action, Env, Item};
use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::process::Command;
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
                if let Err(why) = check_target(&env.home, path) {
                    return fail(out, why);
                }
            }
            let stats = RemoveStats::default();
            for path in paths {
                remove_tree(path, &stats);
            }
            stats.describe(&mut out, &paths[0]);
        }
        Action::DockerWipe { disk } => {
            if env.check_processes && docker_running() {
                return fail(out, "Docker is running. Quit Docker Desktop first.".to_string());
            }
            if let Err(why) = check_target(&env.home, disk) {
                return fail(out, why);
            }
            let stats = RemoveStats::default();
            remove_tree(disk, &stats);
            stats.describe(&mut out, disk);
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
const PROTECTED: &[&str] = &[
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
const OFF_LIMITS: &[&str] = &[
    ".ssh",
    ".gnupg",
    "Library/Keychains",
    "Library/Mobile Documents",
    "Library/Mail",
    "Library/Messages",
    "Library/Photos",
    "Pictures/Photos Library.photoslibrary",
];

/// Last line of defence before deleting: the path must sit inside the home
/// folder and must not be, or be inside, anything important.
pub fn check_target(home: &Path, path: &Path) -> Result<(), String> {
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
    if PROTECTED.iter().any(|p| rel == Path::new(p)) {
        return refuse("it's a protected folder");
    }
    if OFF_LIMITS.iter().any(|p| rel.starts_with(p)) {
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

#[derive(Default)]
struct RemoveStats {
    failed: AtomicUsize,
    /// Failures because the file or its folder belongs to another user.
    not_owned: AtomicUsize,
    /// Failures because macOS blocked access to our own files.
    blocked: AtomicUsize,
    first_error: Mutex<Option<String>>,
}

impl RemoveStats {
    fn record(&self, path: &Path, err: &io::Error) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        if err.kind() == ErrorKind::PermissionDenied {
            let uid = unsafe { libc::getuid() };
            let owned = |p: &Path| fs::symlink_metadata(p).map(|m| m.uid() == uid).unwrap_or(true);
            if !owned(path) || path.parent().is_some_and(|p| !owned(p)) {
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
            Some("macOS blocked access. Quit the app using these files, or give Sweep Full Disk Access.".to_string())
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
        if let Err(err) = fs::remove_dir(path) {
            // A folder that still has files in it was already reported above.
            if !matches!(err.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) {
                stats.record(path, &err);
            }
        }
    } else if let Err(err) = fs::remove_file(path) {
        if err.kind() != ErrorKind::NotFound {
            stats.record(path, &err);
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
    let result = Command::new(bin)
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
    let Action::Remove(paths) = &item.action else {
        return Err("This item can't be fixed this way.".to_string());
    };
    for path in paths {
        check_target(&env.home, path)?;
    }
    // SAFETY: getuid and getgid can't fail.
    let owner = unsafe { format!("{}:{}", libc::getuid(), libc::getgid()) };
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
    let output = Command::new("/usr/bin/osascript")
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
