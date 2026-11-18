//! Where Windows keeps caches, apps and other files Sweep looks at.
//!
//! Paths are relative to the user folder (C:\Users\name) and written with `/`.
//! Everything here is plain file access except the registry lookup, so tests
//! can check the Windows layout on any system.

use super::{
    children, docker_prune, docker_running, join_rel, name_of, normalize, old_entries, pretty_name, Action,
    DevCache, Env, Item, Safety, DAY, MB,
};
use crate::os::Os;
use crate::registry::KnownApp;
use crate::sizes::disk_usage;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn system_env() -> Env {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let home = var("USERPROFILE").unwrap_or_else(|| PathBuf::from("C:\\"));

    let mut app_dirs = Vec::new();
    if let Some(data) = var("ProgramData") {
        app_dirs.push(join_rel(&data, "Microsoft/Windows/Start Menu/Programs"));
    }
    app_dirs.push(join_rel(&home, "AppData/Roaming/Microsoft/Windows/Start Menu/Programs"));
    app_dirs.push(join_rel(&home, "AppData/Local/Programs"));
    app_dirs.extend(["ProgramFiles", "ProgramFiles(x86)"].iter().filter_map(|v| var(v)));

    // Windows apps get the full PATH, unlike apps opened from the macOS Finder.
    let mut bin_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    for rel in ["AppData/Roaming/npm", "AppData/Local/pnpm", ".cargo/bin", ".bun/bin", "AppData/Local/Volta/bin"] {
        bin_dirs.push(join_rel(&home, rel));
    }

    Env {
        os: Os::Windows,
        home,
        app_dirs,
        bin_dirs,
        min_bytes: 5 * MB,
        now: super::now_secs(),
        check_processes: true,
        full_disk_access: true,
    }
}

/// Folders in the user folder that hold no projects.
pub(super) const PRIVATE_HOME_FOLDERS: &[&str] = &[
    "AppData",
    "Music",
    "Pictures",
    "Videos",
    "Saved Games",
    "Contacts",
    "Favorites",
    "Links",
    "Searches",
    "3D Objects",
];

// ---------------------------------------------------------------- caches

/// Folders Chromium and Electron apps keep caches in. They're rebuilt as
/// needed.
const CHROMIUM_CACHES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "ShaderCache",
    "CachedData",
    "CachedExtensionVSIXs",
];

/// AppData folders that belong to Windows, are listed elsewhere (browsers,
/// developer tools) or aren't one app's own.
const SKIP_APPDATA: &[&str] = &[
    "Microsoft",
    "Packages",
    "Temp",
    "Programs",
    "Docker",
    "Google",
    "BraveSoftware",
    "Mozilla",
    "npm-cache",
    "npm",
    "pnpm",
    "Yarn",
    "pip",
    "go-build",
    "electron",
    "electron-builder",
    "node-gyp",
    "ms-playwright",
    "NuGet",
    "Arduino15",
    "Spotify",
    "CrashDumps",
    "D3DSCache",
    "NVIDIA",
    "AMD",
    "Apple Computer",
    "deno",
];

/// Friendly names for app folders under AppData.
const APP_NAMES: &[(&str, &str)] = &[
    ("Code", "Visual Studio Code"),
    ("Code - Insiders", "Visual Studio Code Insiders"),
    ("discord", "Discord"),
    ("slack", "Slack"),
    ("obsidian", "Obsidian"),
    ("Claude", "Claude app"),
];

struct CacheSet {
    title: &'static str,
    /// Folders to clear. A `*` part matches any folder, such as a browser
    /// profile.
    patterns: &'static [&'static str],
}

const KNOWN_CACHES: &[CacheSet] = &[
    CacheSet {
        title: "Google Chrome",
        patterns: &[
            "AppData/Local/Google/Chrome/User Data/*/Cache",
            "AppData/Local/Google/Chrome/User Data/*/Code Cache",
            "AppData/Local/Google/Chrome/User Data/*/GPUCache",
        ],
    },
    CacheSet {
        title: "Microsoft Edge",
        patterns: &[
            "AppData/Local/Microsoft/Edge/User Data/*/Cache",
            "AppData/Local/Microsoft/Edge/User Data/*/Code Cache",
            "AppData/Local/Microsoft/Edge/User Data/*/GPUCache",
        ],
    },
    CacheSet {
        title: "Brave Browser",
        patterns: &[
            "AppData/Local/BraveSoftware/Brave-Browser/User Data/*/Cache",
            "AppData/Local/BraveSoftware/Brave-Browser/User Data/*/Code Cache",
            "AppData/Local/BraveSoftware/Brave-Browser/User Data/*/GPUCache",
        ],
    },
    CacheSet {
        title: "Firefox",
        patterns: &["AppData/Local/Mozilla/Firefox/Profiles/*/cache2"],
    },
    CacheSet {
        title: "Spotify",
        patterns: &["AppData/Local/Spotify/Data"],
    },
    CacheSet {
        title: "Crash reports",
        patterns: &["AppData/Local/CrashDumps"],
    },
    CacheSet {
        title: "DirectX shader cache",
        patterns: &["AppData/Local/D3DSCache"],
    },
    CacheSet {
        title: "NVIDIA shader cache",
        patterns: &["AppData/Local/NVIDIA/DXCache", "AppData/Local/NVIDIA/GLCache"],
    },
    CacheSet {
        title: "AMD shader cache",
        patterns: &[
            "AppData/Local/AMD/DxCache",
            "AppData/Local/AMD/DxcCache",
            "AppData/Local/AMD/GLCache",
            "AppData/Local/AMD/VkCache",
        ],
    },
];

pub(super) fn app_caches(env: &Env) -> Vec<Item> {
    let mut items: Vec<Item> = KNOWN_CACHES
        .par_iter()
        .filter_map(|set| {
            let paths: Vec<PathBuf> = set.patterns.iter().flat_map(|p| expand(&env.home, p)).collect();
            cache_item(env, set.title, paths)
        })
        .collect();

    let apps: Vec<PathBuf> = ["AppData/Roaming", "AppData/Local"]
        .iter()
        .flat_map(|base| children(&env.at(base)))
        .filter(|path| {
            let name = name_of(path);
            is_dir(path) && !name.starts_with('.') && !SKIP_APPDATA.iter().any(|s| s.eq_ignore_ascii_case(&name))
        })
        .collect();
    items.extend(apps.par_iter().filter_map(|app| {
        let paths: Vec<PathBuf> = CHROMIUM_CACHES.iter().map(|c| app.join(c)).filter(|p| is_dir(p)).collect();
        cache_item(env, &app_title(&name_of(app)), paths)
    }).collect::<Vec<_>>());

    items.extend(temp_files(env));
    items
}

fn cache_item(env: &Env, title: &str, paths: Vec<PathBuf>) -> Option<Item> {
    if paths.is_empty() {
        return None;
    }
    let bytes: u64 = paths.iter().map(|p| disk_usage(p)).sum();
    (bytes >= env.min_bytes).then(|| Item::deletable("caches", title, "", paths, bytes, Safety::Safe))
}

fn app_title(folder: &str) -> String {
    if let Some((_, name)) = APP_NAMES.iter().find(|(key, _)| key.eq_ignore_ascii_case(folder)) {
        return name.to_string();
    }
    let name = pretty_name(folder);
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => name,
    }
}

/// Folders a pattern like `AppData/Local/Google/Chrome/User Data/*/Cache`
/// matches.
fn expand(home: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut found = vec![home.to_path_buf()];
    for part in pattern.split('/').filter(|p| !p.is_empty()) {
        found = if part == "*" {
            found.iter().flat_map(|dir| children(dir)).filter(|p| is_dir(p)).collect()
        } else {
            found.iter().map(|dir| dir.join(part)).filter(|p| is_dir(p)).collect()
        };
    }
    found
}

/// Old files in the Temp folder. Installers and apps leave a lot behind there.
fn temp_files(env: &Env) -> Option<Item> {
    let dir = env.at("AppData/Local/Temp");
    let cutoff = env.now - 2 * DAY;
    let bytes: u64 = old_entries(&dir, cutoff).par_iter().map(|p| disk_usage(p)).sum();
    if bytes < env.min_bytes {
        return None;
    }
    let mut item = Item::deletable("caches", "Temporary files", "Older than 2 days", vec![dir.clone()], bytes, Safety::Safe);
    item.id = "caches:temp".to_string();
    item.action = Action::EmptyFolder { dir, older_than: cutoff };
    Some(item)
}

// ------------------------------------------------------- developer caches

pub(super) const DEV_CACHES: &[DevCache] = &[
    DevCache {
        rel: "AppData/Local/npm-cache/_cacache",
        title: "npm download cache",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/npm-cache/_npx",
        title: "npx packages",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/npm-cache/_logs",
        title: "npm logs",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/Yarn/Cache",
        title: "Yarn download cache",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/Yarn/Berry/cache",
        title: "Yarn Berry download cache",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".bun/install/cache",
        title: "Bun download cache",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".cargo/registry/cache",
        title: "Rust crate downloads",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".gradle/caches",
        title: "Gradle cache",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".m2/repository",
        title: "Maven packages",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/pip/Cache",
        title: "pip (Python)",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/go-build",
        title: "Go build cache",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/NuGet/v3-cache",
        title: "NuGet download cache",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".nuget/packages",
        title: "NuGet packages",
        detail: "Downloaded again on the next build",
        safety: Safety::Review,
    },
    DevCache {
        rel: ".cache/prisma",
        title: "Prisma engines",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".platformio/.cache",
        title: "PlatformIO downloads",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/Arduino15/staging",
        title: "Arduino download archives",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/electron/Cache",
        title: "Electron downloads",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/electron-builder/Cache",
        title: "electron-builder downloads",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/node-gyp/Cache",
        title: "node-gyp headers",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/Microsoft/TypeScript",
        title: "TypeScript",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/Microsoft/vscode-cpptools",
        title: "VS Code C/C++ IntelliSense",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/deno",
        title: "Deno",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "AppData/Local/ms-playwright",
        title: "Playwright browsers",
        detail: "Restore with `npx playwright install`",
        safety: Safety::Review,
    },
];

// -------------------------------------------------------- installed apps

/// Names too general to say which app is installed.
const GENERIC_NAMES: &[&str] = &[
    "microsoft",
    "windows",
    "commonfiles",
    "internetexplorer",
    "windowsapps",
    "uninstallinformation",
    "modifiablewindowsapps",
    "referenceassemblies",
    "msbuild",
    "dotnet",
    "accessibility",
    "accessories",
    "administrativetools",
    "maintenance",
    "startup",
    "systemtools",
    "windowspowershell",
];

/// Installed programs as (display name, normalized name), from Start menu
/// shortcuts, program folders and the list in Settings → Apps.
pub(super) fn installed_names(env: &Env) -> Vec<(String, String)> {
    let mut names: Vec<(String, String)> = Vec::new();
    let mut add = |name: &str| {
        let display = name.trim().to_string();
        let norm = normalize(&display);
        if norm.len() >= 2 && !GENERIC_NAMES.contains(&norm.as_str()) {
            names.push((display, norm));
        }
    };
    for dir in &env.app_dirs {
        for entry in children(dir) {
            let name = name_of(&entry);
            if is_dir(&entry) {
                add(&name);
                for inner in children(&entry) {
                    if let Some(stem) = shortcut_name(&name_of(&inner)) {
                        add(stem);
                    }
                }
            } else if let Some(stem) = shortcut_name(&name) {
                add(stem);
            }
        }
    }
    if env.check_processes {
        for name in registered_programs() {
            add(&name);
        }
    }
    names.sort();
    names.dedup();
    names
}

/// "Slack.lnk" → "Slack".
fn shortcut_name(file: &str) -> Option<&str> {
    let dot = file.len().checked_sub(4)?;
    file.get(dot..)?.eq_ignore_ascii_case(".lnk").then(|| &file[..dot])
}

/// Whether a known app is installed: one of its usual install folders
/// exists, or Windows lists a program with its name.
pub(super) fn app_installed(env: &Env, app: &KnownApp, installed_names: &[(String, String)]) -> bool {
    let target = normalize(app.name);
    app.installs.iter().filter_map(|spec| resolve(env, spec)).any(|path| path.exists())
        || installed_names
            .iter()
            .any(|(_, name)| name.contains(&target) || (name.len() >= 4 && target.contains(name.as_str())))
}

/// A path from the known-apps list: relative to the user folder, or starting
/// with an environment variable such as `%ProgramFiles%`.
fn resolve(env: &Env, spec: &str) -> Option<PathBuf> {
    match spec.strip_prefix('%') {
        Some(rest) => {
            let (var, rel) = rest.split_once('%')?;
            let base = std::env::var_os(var)?;
            Some(join_rel(Path::new(&base), rel))
        }
        None => Some(env.at(spec)),
    }
}

/// Display names of programs in Settings → Apps (the Uninstall registry keys).
#[cfg(windows)]
fn registered_programs() -> Vec<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    use winreg::RegKey;

    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut names = Vec::new();
    for (root, view) in [
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, KEY_WOW64_64KEY),
    ] {
        let Ok(list) = RegKey::predef(root).open_subkey_with_flags(UNINSTALL, KEY_READ | view) else {
            continue;
        };
        for key in list.enum_keys().flatten() {
            if let Ok(name) = list.open_subkey(&key).and_then(|k| k.get_value::<String, _>("DisplayName")) {
                names.push(name);
            }
        }
    }
    names
}

#[cfg(not(windows))]
fn registered_programs() -> Vec<String> {
    Vec::new()
}

// ---------------------------------------------------------------- docker

/// Docker Desktop keeps everything in one virtual disk. Pruning frees space
/// inside it; deleting it is left to Docker Desktop, which also has to
/// unregister it from WSL.
pub(super) fn docker(env: &Env) -> Vec<Item> {
    let Some(disk) = ["AppData/Local/Docker/wsl/disk/docker_data.vhdx", "AppData/Local/Docker/wsl/data/ext4.vhdx"]
        .iter()
        .map(|rel| env.at(rel))
        .find(|path| path.is_file())
    else {
        return Vec::new();
    };
    let bytes = disk_usage(&disk);
    if bytes < env.min_bytes {
        return Vec::new();
    }
    let running = env.check_processes && docker_running(Os::Windows);
    let bin = resolve(env, "%ProgramFiles%/Docker/Docker/resources/bin/docker.exe")
        .filter(|path| path.is_file())
        .or_else(|| env.find_bin("docker"));
    let prune = docker_prune(&disk, bytes, bin, running);

    let mut purge = Item::deletable(
        "docker",
        "All Docker data",
        "Docker Desktop → Troubleshoot → Clean / Purge data",
        vec![disk],
        bytes,
        Safety::Info,
    );
    purge.id = "docker:purge".to_string();
    purge.preselect = false;
    purge.enabled = false;
    purge.action = Action::Nothing;
    vec![prune, purge]
}

// ---------------------------------------------------------------- manual

struct Manual {
    path: &'static str,
    title: &'static str,
    tip: &'static str,
    /// Measure the folder. Off for ones too slow or locked to measure.
    measure: bool,
}

const MANUAL: &[Manual] = &[
    Manual {
        path: "AppData/Local/Packages/5319275A.WhatsAppDesktop_cv1g1gvanyjgm/LocalState",
        title: "WhatsApp photos and videos",
        tip: "WhatsApp → Settings → Storage",
        measure: true,
    },
    Manual {
        path: "AppData/Roaming/Apple Computer/MobileSync/Backup",
        title: "iPhone and iPad backups",
        tip: "Apple Devices → your device → Manage Backups",
        measure: true,
    },
    Manual {
        path: "Apple/MobileSync/Backup",
        title: "iPhone and iPad backups",
        tip: "Apple Devices → your device → Manage Backups",
        measure: true,
    },
    Manual {
        path: "%SystemDrive%/Windows.old",
        title: "Previous version of Windows",
        tip: "Settings → System → Storage → Temporary files",
        measure: false,
    },
];

pub(super) fn manual(env: &Env) -> Vec<Item> {
    MANUAL
        .par_iter()
        .filter_map(|entry| {
            let path = resolve(env, entry.path)?;
            fs::symlink_metadata(&path).ok()?;
            let readable = entry.measure && fs::read_dir(&path).is_ok();
            let bytes = if readable { disk_usage(&path) } else { 0 };
            if readable && bytes < 100 * MB {
                return None;
            }
            let mut item = Item::deletable("manual", entry.title, entry.tip, vec![path], bytes, Safety::Info);
            item.preselect = false;
            item.enabled = false;
            item.size_unknown = !readable;
            item.action = Action::Nothing;
            Some(item)
        })
        .collect()
}

// -------------------------------------------------------------- helpers

/// A real folder, not a link or junction to one.
fn is_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_names() {
        assert_eq!(shortcut_name("Slack.lnk"), Some("Slack"));
        assert_eq!(shortcut_name("Docker Desktop.LNK"), Some("Docker Desktop"));
        assert_eq!(shortcut_name("readme.txt"), None);
        assert_eq!(shortcut_name("a"), None);
    }

    #[test]
    fn friendly_app_folder_names() {
        assert_eq!(app_title("Code"), "Visual Studio Code");
        assert_eq!(app_title("slack"), "Slack");
        assert_eq!(app_title("beekeeper-studio"), "Beekeeper studio");
    }
}
