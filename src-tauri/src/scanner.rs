//! Finds files that can go without affecting your work.
//!
//! Scanning only reads the disk. Deleting happens in `cleaner`, and only for
//! items a scan produced.

use crate::disk::{disk_info, DiskInfo};
use crate::registry::KNOWN_APPS;
use crate::sizes::{disk_usage, newest_file_mtime, path_mtime};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MB: u64 = 1_000_000;
const DAY: i64 = 86_400;

/// How careful the user should be before deleting an item.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Safety {
    /// Comes back on its own (caches, build output).
    Safe,
    /// Probably unneeded, but holds something the user may want.
    Review,
    /// Deletes data that can't be recovered.
    Danger,
    /// Shown for information only; Sweep won't delete it.
    Info,
}

/// What cleaning an item does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Remove(Vec<PathBuf>),
    PnpmPrune { bin: PathBuf },
    DockerPrune { bin: PathBuf },
    /// Deletes Docker's disk image. Refused while Docker is running.
    DockerWipe { disk: PathBuf },
    Nothing,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub category: String,
    pub title: String,
    pub detail: String,
    pub paths: Vec<String>,
    pub bytes: u64,
    /// `bytes` is an upper bound: the action frees only part of it.
    pub approx: bool,
    /// Sweep couldn't read the folder, so `bytes` is unknown.
    pub size_unknown: bool,
    pub safety: Safety,
    pub preselect: bool,
    /// False when the item can't be cleaned right now; `detail` says why.
    pub enabled: bool,
    pub last_used: Option<i64>,
    /// Items sharing a group are shown together (one project, for example).
    pub group: Option<String>,
    #[serde(skip)]
    pub action: Action,
}

impl Item {
    fn deletable(
        category: &str,
        title: impl Into<String>,
        detail: impl Into<String>,
        paths: Vec<PathBuf>,
        bytes: u64,
        safety: Safety,
    ) -> Item {
        let first = paths.first().map(|p| p.display().to_string()).unwrap_or_default();
        Item {
            id: format!("{category}:{first}"),
            category: category.to_string(),
            title: title.into(),
            detail: detail.into(),
            paths: paths.iter().map(|p| p.display().to_string()).collect(),
            bytes,
            approx: false,
            size_unknown: false,
            safety,
            preselect: safety == Safety::Safe,
            enabled: true,
            last_used: paths.iter().filter_map(|p| path_mtime(p)).max(),
            group: None,
            action: Action::Remove(paths),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: &'static str,
    pub title: &'static str,
    pub items: Vec<Item>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub home: String,
    pub disk: DiskInfo,
    pub categories: Vec<Category>,
    pub scanned_at: i64,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptions {
    /// Folders to search for projects. Empty means the whole home folder.
    #[serde(default)]
    pub project_roots: Vec<String>,
    /// Build folders in projects untouched for this long are preselected.
    #[serde(default = "default_inactive_days")]
    pub inactive_days: u32,
}

fn default_inactive_days() -> u32 {
    30
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            project_roots: Vec::new(),
            inactive_days: default_inactive_days(),
        }
    }
}

/// Where to look. Tests point this at a temporary folder.
pub struct Env {
    pub home: PathBuf,
    /// Folders that hold installed apps.
    pub app_dirs: Vec<PathBuf>,
    /// Folders searched for command-line tools.
    pub bin_dirs: Vec<PathBuf>,
    /// Items smaller than this aren't worth listing.
    pub min_bytes: u64,
    pub now: i64,
    /// Look at running processes (for Docker). Off in tests.
    pub check_processes: bool,
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Env {
    pub fn system() -> Env {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let app_dirs = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/Applications/Utilities"),
            PathBuf::from("/System/Applications"),
            home.join("Applications"),
        ];
        // Apps opened from Finder get a minimal PATH, so list the usual places.
        let mut bin_dirs: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"]
            .iter()
            .map(PathBuf::from)
            .collect();
        for rel in [
            "Library/pnpm",
            "Library/pnpm/bin",
            ".local/bin",
            ".cargo/bin",
            ".volta/bin",
            ".bun/bin",
            "bin",
        ] {
            bin_dirs.push(home.join(rel));
        }
        if let Ok(read) = fs::read_dir(home.join(".nvm/versions/node")) {
            bin_dirs.extend(read.flatten().map(|e| e.path().join("bin")));
        }
        Env {
            home,
            app_dirs,
            bin_dirs,
            min_bytes: 5 * MB,
            now: now_secs(),
            check_processes: true,
        }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    pub fn find_bin(&self, name: &str) -> Option<PathBuf> {
        self.bin_dirs
            .iter()
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    }

    /// PATH for tools Sweep runs, so scripts like pnpm can find node.
    pub fn search_path(&self) -> String {
        let dirs: Vec<String> = self.bin_dirs.iter().map(|d| d.display().to_string()).collect();
        dirs.join(":")
    }

    fn app_installed(&self, bundle: &str) -> bool {
        self.app_dirs.iter().any(|dir| dir.join(bundle).exists())
    }

    /// Installed apps as (display name, normalized name).
    fn installed_apps(&self) -> Vec<(String, String)> {
        let mut apps = Vec::new();
        for dir in &self.app_dirs {
            let Ok(read) = fs::read_dir(dir) else { continue };
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let display = name.strip_suffix(".app").unwrap_or(&name).to_string();
                let norm = normalize(&display);
                if !norm.is_empty() {
                    apps.push((display, norm));
                }
            }
        }
        apps
    }

    fn tilde(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rel) if rel.as_os_str().is_empty() => "~".to_string(),
            Ok(rel) => format!("~/{}", rel.display()),
            Err(_) => path.display().to_string(),
        }
    }
}

/// Scans everything. `progress` receives a short note before each step.
pub fn scan(env: &Env, opts: &ScanOptions, progress: &(dyn Fn(&str) + Sync)) -> ScanResult {
    progress("Measuring app caches");
    let caches = scan_app_caches(env);
    progress("Measuring developer caches");
    let dev = scan_dev_caches(env);
    progress("Looking for build folders in your projects");
    let projects = scan_projects(env, opts);
    progress("Looking for leftovers from deleted apps");
    let leftovers = scan_leftovers(env);
    progress("Checking installers in Downloads");
    let installers = scan_installers(env);
    progress("Checking Docker");
    let docker = scan_docker(env);
    progress("Checking other large folders");
    let manual = scan_manual(env);

    let mut categories = vec![
        Category {
            id: "caches",
            title: "App caches",
            items: caches,
        },
        Category {
            id: "dev",
            title: "Developer caches",
            items: dev,
        },
        Category {
            id: "projects",
            title: "Build folders",
            items: projects,
        },
        Category {
            id: "leftovers",
            title: "App leftovers",
            items: leftovers,
        },
        Category {
            id: "installers",
            title: "Installers",
            items: installers,
        },
        Category {
            id: "docker",
            title: "Docker",
            items: docker,
        },
        Category {
            id: "manual",
            title: "Check yourself",
            items: manual,
        },
    ];
    for category in &mut categories {
        category.items.sort_by_key(|item| std::cmp::Reverse(item.bytes));
    }
    ScanResult {
        home: env.home.display().to_string(),
        disk: disk_info(),
        categories,
        scanned_at: env.now,
    }
}

// ---------------------------------------------------------------- caches

/// Caches macOS manages itself or won't let apps touch.
const PROTECTED_CACHES: &[&str] = &["CloudKit", "FamilyCircle", "PassKit", "Metadata"];

fn scan_app_caches(env: &Env) -> Vec<Item> {
    let entries: Vec<PathBuf> = children(&env.at("Library/Caches"))
        .into_iter()
        .filter(|path| {
            let name = name_of(path);
            !name.starts_with('.')
                && !name.starts_with("com.apple.")
                && !PROTECTED_CACHES.contains(&name.as_str())
        })
        .collect();
    entries
        .par_iter()
        .filter_map(|path| {
            let bytes = disk_usage(path);
            if bytes < env.min_bytes {
                return None;
            }
            let (title, detail, safety) = describe_cache(&name_of(path));
            Some(Item::deletable("caches", title, detail, vec![path.clone()], bytes, safety))
        })
        .collect()
}

fn describe_cache(name: &str) -> (String, String, Safety) {
    const NAMES: &[(&str, &str)] = &[
        ("Google", "Google Chrome"),
        ("BraveSoftware", "Brave Browser"),
        ("Firefox", "Firefox"),
        ("com.microsoft.edgemac", "Microsoft Edge"),
        ("com.anthropic.claudefordesktop.ShipIt", "Claude app update downloads"),
        ("electron", "Electron downloads"),
        ("electron-builder", "electron-builder downloads"),
        ("JetBrains", "JetBrains IDEs"),
        ("pnpm", "pnpm metadata"),
        ("Yarn", "Yarn"),
        ("composer", "Composer (PHP)"),
        ("CocoaPods", "CocoaPods"),
        ("next-swc", "Next.js compiler"),
        ("node-gyp", "node-gyp headers"),
        ("Homebrew", "Homebrew downloads"),
        ("vscode-cpptools", "VS Code C/C++ IntelliSense"),
        ("pip", "pip (Python)"),
        ("go-build", "Go build cache"),
        ("deno", "Deno"),
        ("typescript", "TypeScript"),
        ("com.spotify.client", "Spotify"),
        ("arduino", "Arduino"),
    ];
    if name.starts_with("ms-playwright") {
        let flavor = name.strip_prefix("ms-playwright-").map(|f| format!(" ({f})")).unwrap_or_default();
        return (
            format!("Playwright browsers{flavor}"),
            "Restore with `npx playwright install`".to_string(),
            Safety::Review,
        );
    }
    let known = NAMES.iter().find(|(key, _)| *key == name).map(|(_, t)| t.to_string());
    if let Some(app) = name.strip_suffix("-updater").or_else(|| name.strip_suffix(".ShipIt")) {
        return (
            known.unwrap_or_else(|| format!("{} update downloads", pretty_name(app))),
            String::new(),
            Safety::Safe,
        );
    }
    (
        known.unwrap_or_else(|| pretty_name(name)),
        String::new(),
        Safety::Safe,
    )
}

/// "com.tinyapp.DBngin" → "DBngin", "@opencode-ai" → "opencode ai".
fn pretty_name(raw: &str) -> String {
    let raw = raw.trim_start_matches('@');
    let looks_like_bundle_id = raw.matches('.').count() >= 2 && !raw.contains(' ');
    let base = if looks_like_bundle_id {
        raw.rsplit('.').next().unwrap_or(raw)
    } else {
        raw
    };
    base.replace(['-', '_'], " ")
}

// ------------------------------------------------------- developer caches

struct DevCache {
    rel: &'static str,
    title: &'static str,
    detail: &'static str,
    safety: Safety,
}

const DEV_CACHES: &[DevCache] = &[
    DevCache {
        rel: ".npm/_cacache",
        title: "npm download cache",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".npm/_npx",
        title: "npx packages",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".npm/_logs",
        title: "npm logs",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: ".yarn/berry/cache",
        title: "Yarn download cache",
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
        rel: "Library/Arduino15/staging",
        title: "Arduino download archives",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "Library/Developer/Xcode/DerivedData",
        title: "Xcode build data",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "Library/Developer/CoreSimulator/Caches",
        title: "Simulator caches",
        detail: "",
        safety: Safety::Safe,
    },
    DevCache {
        rel: "Library/Developer/Xcode/iOS DeviceSupport",
        title: "iOS device support files",
        detail: "Recreated when you connect a device",
        safety: Safety::Review,
    },
    DevCache {
        rel: "Library/Developer/Xcode/Archives",
        title: "Xcode archives",
        detail: "Needed only to resubmit old builds",
        safety: Safety::Review,
    },
];

fn scan_dev_caches(env: &Env) -> Vec<Item> {
    let mut items: Vec<Item> = DEV_CACHES
        .par_iter()
        .filter_map(|cache| {
            let path = env.at(cache.rel);
            if fs::symlink_metadata(&path).is_err() {
                return None;
            }
            let bytes = disk_usage(&path);
            (bytes >= env.min_bytes)
                .then(|| Item::deletable("dev", cache.title, cache.detail, vec![path], bytes, cache.safety))
        })
        .collect();
    items.extend(old_puppeteer_browsers(env));
    items.extend(codex_install_leftovers(env));
    items.extend(pnpm_store(env));
    items
}

/// Puppeteer keeps every browser version it ever downloaded; keep the newest.
fn old_puppeteer_browsers(env: &Env) -> Vec<Item> {
    let mut items = Vec::new();
    for product in children_dirs(&env.at(".cache/puppeteer")) {
        let mut versions = children_dirs(&product);
        if versions.len() < 2 {
            continue;
        }
        versions.sort_by_key(|path| version_key(&name_of(path)));
        versions.pop(); // keep the newest
        for old in versions {
            let bytes = disk_usage(&old);
            if bytes < env.min_bytes {
                continue;
            }
            let title = format!(
                "Old Puppeteer {} {}",
                name_of(&product),
                version_label(&name_of(&old))
            );
            items.push(Item::deletable("dev", title, "", vec![old], bytes, Safety::Safe));
        }
    }
    items
}

fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn version_label(name: &str) -> String {
    name.rsplit('-').next().unwrap_or(name).to_string()
}

fn codex_install_leftovers(env: &Env) -> Option<Item> {
    let paths: Vec<PathBuf> = children_dirs(&env.at(".cache/codex-runtimes"))
        .into_iter()
        .filter(|p| name_of(p).starts_with("codex-runtime-install-"))
        .collect();
    let bytes: u64 = paths.iter().map(|p| disk_usage(p)).sum();
    (bytes >= env.min_bytes).then(|| {
        Item::deletable(
            "dev",
            "Codex install leftovers",
            "",
            paths,
            bytes,
            Safety::Safe,
        )
    })
}

fn pnpm_store(env: &Env) -> Option<Item> {
    let store = ["Library/pnpm/store", ".local/share/pnpm/store", ".pnpm-store"]
        .iter()
        .map(|rel| env.at(rel))
        .find(|path| path.is_dir())?;
    let bytes = disk_usage(&store);
    if bytes < env.min_bytes {
        return None;
    }
    let mut item = Item::deletable(
        "dev",
        "pnpm package store",
        "Runs `pnpm store prune`, which removes only unused packages",
        vec![store],
        bytes,
        Safety::Safe,
    );
    item.id = "dev:pnpm-prune".to_string();
    item.approx = true;
    match env.find_bin("pnpm") {
        Some(bin) => item.action = Action::PnpmPrune { bin },
        None => {
            item.enabled = false;
            item.preselect = false;
            item.action = Action::Nothing;
            item.detail = "pnpm not found".to_string();
        }
    }
    Some(item)
}

// ------------------------------------------------------------- projects

struct Artifact {
    /// Folder name.
    dir: &'static str,
    /// The folder only counts if its parent has one of these files.
    markers: &'static [&'static str],
    label: &'static str,
    restore: &'static str,
    safety: Safety,
}

const PYTHON_MARKERS: &[&str] = &["requirements.txt", "pyproject.toml", "Pipfile", "setup.py"];

const ARTIFACTS: &[Artifact] = &[
    Artifact {
        dir: "node_modules",
        markers: &["package.json"],
        label: "Node packages",
        restore: "npm install",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".next",
        markers: &["package.json"],
        label: "Next.js build",
        restore: "npm run dev",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".nuxt",
        markers: &["package.json"],
        label: "Nuxt build",
        restore: "npm run dev",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".svelte-kit",
        markers: &["package.json"],
        label: "SvelteKit build",
        restore: "npm run dev",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".turbo",
        markers: &["package.json", "turbo.json"],
        label: "Turborepo cache",
        restore: "npm run build",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".parcel-cache",
        markers: &["package.json"],
        label: "Parcel cache",
        restore: "npm run build",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".angular",
        markers: &["angular.json"],
        label: "Angular cache",
        restore: "ng build",
        safety: Safety::Safe,
    },
    Artifact {
        dir: "target",
        markers: &["Cargo.toml"],
        label: "Rust build",
        restore: "cargo build",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".pio",
        markers: &["platformio.ini"],
        label: "PlatformIO build",
        restore: "pio run",
        safety: Safety::Safe,
    },
    Artifact {
        dir: "Pods",
        markers: &["Podfile"],
        label: "CocoaPods",
        restore: "pod install",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".gradle",
        markers: &["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"],
        label: "Gradle cache",
        restore: "./gradlew build",
        safety: Safety::Safe,
    },
    Artifact {
        dir: "build",
        markers: &["build.gradle", "build.gradle.kts"],
        label: "Gradle build",
        restore: "./gradlew build",
        safety: Safety::Safe,
    },
    Artifact {
        dir: ".venv",
        markers: PYTHON_MARKERS,
        label: "Python environment",
        restore: "pip install -r requirements.txt",
        safety: Safety::Review,
    },
    Artifact {
        dir: "venv",
        markers: PYTHON_MARKERS,
        label: "Python environment",
        restore: "pip install -r requirements.txt",
        safety: Safety::Review,
    },
];

const MAX_PROJECT_DEPTH: usize = 8;

fn scan_projects(env: &Env, opts: &ScanOptions) -> Vec<Item> {
    let mut roots: Vec<PathBuf> = if opts.project_roots.is_empty() {
        vec![env.home.clone()]
    } else {
        opts.project_roots
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.starts_with(&env.home) && p.is_dir())
            .collect()
    };
    roots.sort();
    roots.dedup();
    // A folder inside another chosen folder is already covered.
    let roots: Vec<PathBuf> = roots
        .iter()
        .filter(|r| !roots.iter().any(|other| other != *r && r.starts_with(other)))
        .cloned()
        .collect();

    let found: Vec<(PathBuf, &'static Artifact, PathBuf)> = roots
        .par_iter()
        .flat_map(|root| {
            find_artifacts(env, root, MAX_PROJECT_DEPTH)
                .into_iter()
                .map(|(path, art)| (path, art, root.clone()))
                .collect::<Vec<_>>()
        })
        .collect();

    let project_of: HashMap<PathBuf, PathBuf> = found
        .iter()
        .map(|(path, _, root)| (path.clone(), project_root(path, root)))
        .collect();
    let projects: Vec<PathBuf> = project_of.values().cloned().collect::<HashSet<_>>().into_iter().collect();
    let activity: HashMap<PathBuf, Option<i64>> = projects
        .par_iter()
        .map(|project| {
            let last = if *project == env.home {
                None // never walk the whole home folder for this
            } else {
                newest_file_mtime(project, 6, &skip_for_activity)
            };
            (project.clone(), last)
        })
        .collect();
    let cutoff = env.now - i64::from(opts.inactive_days) * DAY;

    found
        .par_iter()
        .filter_map(|(path, art, _)| {
            let bytes = disk_usage(path);
            if bytes < env.min_bytes {
                return None;
            }
            let project = &project_of[path];
            let parent = path.parent().unwrap_or(path);
            let last = activity.get(project).copied().flatten().or_else(|| {
                // Fall back to the marker file, e.g. package.json.
                art.markers.iter().filter_map(|m| path_mtime(&parent.join(m))).max()
            });
            let inactive = last.is_none_or(|t| t < cutoff);
            let title = match parent.strip_prefix(project) {
                Ok(rel) if !rel.as_os_str().is_empty() => format!("{} in {}", art.label, rel.display()),
                _ => art.label.to_string(),
            };
            let detail = format!("Restore with `{}`", art.restore);
            let mut item = Item::deletable("projects", title, detail, vec![path.clone()], bytes, art.safety);
            item.group = Some(env.tilde(project));
            item.last_used = last;
            item.preselect = art.safety == Safety::Safe && inactive;
            Some(item)
        })
        .collect()
}

fn find_artifacts(env: &Env, dir: &Path, depth: usize) -> Vec<(PathBuf, &'static Artifact)> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut subdirs = Vec::new();
    for entry in read.flatten() {
        // file_type doesn't follow symlinks, so linked folders are skipped.
        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let artifact = ARTIFACTS
            .iter()
            .find(|a| a.dir == name && a.markers.iter().any(|m| dir.join(m).is_file()));
        if let Some(art) = artifact {
            found.push((path, art));
            continue; // never look inside build folders
        }
        if name == "node_modules"
            || name.starts_with('.')
            || is_bundle(&name)
            || is_private_home_folder(env, &path)
        {
            continue;
        }
        if depth > 1 {
            subdirs.push(path);
        }
    }
    found.extend(
        subdirs
            .par_iter()
            .flat_map(|sub| find_artifacts(env, sub, depth - 1))
            .collect::<Vec<_>>(),
    );
    found
}

/// The project a build folder belongs to: the nearest enclosing git
/// repository inside the scanned folder, or else the folder it sits in.
fn project_root(artifact: &Path, scan_root: &Path) -> PathBuf {
    let start = artifact.parent().unwrap_or(artifact);
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        if dir == scan_root {
            break;
        }
        match dir.parent() {
            Some(parent) if parent.starts_with(scan_root) => dir = parent,
            _ => break,
        }
    }
    start.to_path_buf()
}

fn skip_for_activity(name: &str) -> bool {
    name.starts_with('.')
        || matches!(name, "dist" | "build" | "out" | "coverage")
        || ARTIFACTS.iter().any(|a| a.dir == name)
}

fn is_bundle(name: &str) -> bool {
    [
        ".app",
        ".photoslibrary",
        ".musiclibrary",
        ".tvlibrary",
        ".framework",
        ".bundle",
        ".xcarchive",
        ".dSYM",
    ]
    .iter()
    .any(|ext| name.ends_with(ext))
}

fn is_private_home_folder(env: &Env, path: &Path) -> bool {
    ["Library", "Applications", "Music", "Movies", "Pictures", "Public"]
        .iter()
        .any(|rel| path == env.at(rel))
}

// ------------------------------------------------------------ leftovers

fn scan_leftovers(env: &Env) -> Vec<Item> {
    KNOWN_APPS
        .par_iter()
        .filter_map(|app| {
            let installed = app.bundles.iter().any(|b| env.app_installed(b))
                || app.clis.iter().any(|c| env.find_bin(c).is_some());
            if installed {
                return None;
            }
            let paths: Vec<PathBuf> = app
                .paths
                .iter()
                .map(|rel| env.at(rel))
                .filter(|p| fs::symlink_metadata(p).is_ok())
                .collect();
            let bytes: u64 = paths.iter().map(|p| disk_usage(p)).sum();
            if paths.is_empty() || bytes < env.min_bytes {
                return None;
            }
            let detail = if app.note.is_empty() {
                "Not installed".to_string()
            } else {
                format!("Not installed · {}", app.note)
            };
            let mut item = Item::deletable("leftovers", app.name, detail, paths, bytes, Safety::Review);
            item.preselect = false;
            Some(item)
        })
        .collect()
}

// ----------------------------------------------------------- installers

const INSTALLER_EXTS: &[&str] = &["dmg", "pkg", "mpkg", "iso", "xip"];
const ARCHIVE_EXTS: &[&str] = &["zip", "tgz", "gz", "xz", "bz2", "7z", "rar", "tar"];
const PLATFORM_WORDS: &[&str] = &[
    "mac", "macos", "osx", "darwin", "universal", "arm64", "aarch64", "x64", "x86", "intel",
    "apple", "silicon", "installer", "setup", "release", "latest", "stable", "beta", "education",
    "signed", "full",
];

fn scan_installers(env: &Env) -> Vec<Item> {
    let downloads = env.at("Downloads");
    let apps = env.installed_apps();
    let mut items = Vec::new();
    for path in children(&downloads) {
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if !meta.is_file() {
            continue;
        }
        let name = name_of(&path);
        let ext = name.rsplit('.').next().unwrap_or_default().to_lowercase();
        let is_installer = INSTALLER_EXTS.contains(&ext.as_str());
        let is_archive = ARCHIVE_EXTS.contains(&ext.as_str());
        let bytes = disk_usage(&path);
        let big_enough = if is_installer {
            bytes >= env.min_bytes
        } else {
            bytes >= env.min_bytes * 10
        };
        if !(is_installer || is_archive) || !big_enough {
            continue;
        }

        let base = product_base(&name);
        let installed = matching_app(&apps, &base);
        let unpacked = if is_archive {
            [env.at(&base), downloads.join(&base), downloads.join(strip_extensions(&name))]
                .into_iter()
                .find(|p| !base.is_empty() && p.is_dir())
        } else {
            None
        };
        let intel_on_arm = cfg!(target_arch = "aarch64") && is_intel_build(&name.to_lowercase());

        let (mut detail, safety) = if let Some(app) = &installed {
            (format!("{app} is installed"), Safety::Safe)
        } else if let Some(dir) = &unpacked {
            (format!("Unpacked to {}", env.tilde(dir)), Safety::Safe)
        } else if is_installer {
            ("Installer".to_string(), Safety::Review)
        } else {
            ("Archive · check before deleting".to_string(), Safety::Review)
        };
        if intel_on_arm {
            detail.push_str(" · Intel version");
        }
        items.push(Item::deletable("installers", name, detail, vec![path], bytes, safety));
    }
    items
}

/// Product name from a download's file name:
/// "oss-cad-suite-darwin-arm64-2026.tgz" → "oss-cad-suite",
/// "Gowin_V1.9.11_macOS.dmg" → "gowin".
pub fn product_base(file_name: &str) -> String {
    let stem = strip_extensions(file_name);
    let mut words = Vec::new();
    for word in stem.split(['-', '_', ' ', '.', '(', ')']) {
        if word.is_empty() {
            continue;
        }
        let lower = word.to_lowercase();
        if lower.chars().any(|c| c.is_ascii_digit()) || PLATFORM_WORDS.contains(&lower.as_str()) {
            break;
        }
        words.push(lower);
    }
    words.join("-")
}

fn strip_extensions(file_name: &str) -> String {
    let lower = file_name.to_lowercase();
    for double in [".tar.gz", ".tar.xz", ".tar.bz2"] {
        if lower.ends_with(double) {
            return file_name[..file_name.len() - double.len()].to_string();
        }
    }
    match file_name.rfind('.') {
        Some(dot) if dot > 0 => file_name[..dot].to_string(),
        _ => file_name.to_string(),
    }
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The installed app a download is for. `base` comes from `product_base`.
///
/// Matches whole words only: "docker-desktop" matches Docker and "gowin"
/// matches GowinIDE, but "pagesmith" doesn't match Pages.
fn matching_app(apps: &[(String, String)], base: &str) -> Option<String> {
    let words: Vec<&str> = base.split('-').filter(|w| !w.is_empty()).collect();
    let prefixes: Vec<String> = (1..=words.len()).map(|n| normalize(&words[..n].concat())).collect();
    let full = prefixes.last()?;
    if full.len() < 3 {
        return None;
    }
    apps.iter()
        .find(|(_, app)| {
            app == full
                || (app.len() >= 4 && prefixes.contains(app))
                || (full.len() >= 4 && app.starts_with(full.as_str()))
        })
        .map(|(display, _)| display.clone())
}

fn is_intel_build(lower_name: &str) -> bool {
    ["x64", "x86_64", "x86-64", "intel", "amd64"]
        .iter()
        .any(|tag| lower_name.contains(tag))
}

// --------------------------------------------------------------- docker

fn scan_docker(env: &Env) -> Vec<Item> {
    let disk = env.at("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw");
    if !disk.is_file() {
        return Vec::new();
    }
    let bytes = disk_usage(&disk);
    if bytes < env.min_bytes {
        return Vec::new();
    }
    let running = env.check_processes && docker_running();
    let bin = [PathBuf::from("/Applications/Docker.app/Contents/Resources/bin/docker")]
        .into_iter()
        .find(|p| p.is_file())
        .or_else(|| env.find_bin("docker"));

    let mut prune = Item::deletable(
        "docker",
        "Unused images and build cache",
        "Runs `docker system prune -a`. Keeps volumes",
        vec![disk.clone()],
        bytes,
        Safety::Review,
    );
    prune.id = "docker:prune".to_string();
    prune.approx = true;
    prune.preselect = false;
    match (&bin, running) {
        (Some(bin), true) => prune.action = Action::DockerPrune { bin: bin.clone() },
        (Some(_), false) => {
            prune.enabled = false;
            prune.action = Action::Nothing;
            prune.detail.push_str(" · open Docker Desktop first");
        }
        (None, _) => {
            prune.enabled = false;
            prune.action = Action::Nothing;
            prune.detail.push_str(" · docker not found");
        }
    }

    let mut wipe = Item::deletable(
        "docker",
        "All Docker data",
        "Every image, container and volume",
        vec![disk.clone()],
        bytes,
        Safety::Danger,
    );
    wipe.id = "docker:wipe".to_string();
    wipe.preselect = false;
    wipe.action = Action::DockerWipe { disk };
    if running {
        wipe.enabled = false;
        wipe.detail.push_str(" · quit Docker Desktop first");
    }
    vec![prune, wipe]
}

pub fn docker_running() -> bool {
    Command::new("/usr/bin/pgrep")
        .args(["-f", "com.docker.backend|Docker Desktop.app/Contents/MacOS"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

// --------------------------------------------------------------- manual

fn scan_manual(env: &Env) -> Vec<Item> {
    const FOLDERS: &[(&str, &str, &str)] = &[
        (
            "Library/Group Containers/group.net.whatsapp.WhatsApp.shared",
            "WhatsApp photos and videos",
            "WhatsApp → Settings → Storage and Data",
        ),
        (
            "Library/Application Support/MobileSync/Backup",
            "iPhone and iPad backups",
            "Finder → your device → Manage Backups",
        ),
        (
            "Library/Application Support/Claude/vm_bundles",
            "Claude app virtual machine",
            "Used by the Claude app",
        ),
        (
            "Library/Containers/com.apple.mail/Data/Library/Mail Downloads",
            "Mail attachments you opened",
            "Copies of opened attachments",
        ),
        (".Trash", "Trash", "Empty it in Finder"),
    ];
    FOLDERS
        .par_iter()
        .filter_map(|(rel, title, tip)| {
            let path = env.at(rel);
            fs::symlink_metadata(&path).ok()?;
            let readable = fs::read_dir(&path).is_ok();
            let bytes = if readable { disk_usage(&path) } else { 0 };
            let is_trash = *rel == ".Trash";
            if readable && bytes < 100 * MB && !(is_trash && bytes >= env.min_bytes) {
                return None;
            }
            let mut item = Item::deletable("manual", *title, *tip, vec![path], bytes, Safety::Info);
            item.preselect = false;
            item.enabled = false;
            item.size_unknown = !readable;
            item.action = Action::Nothing;
            Some(item)
        })
        .collect()
}

// -------------------------------------------------------------- helpers

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn children(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(read) => read.flatten().map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    };
    paths.sort();
    paths
}

fn children_dirs(dir: &Path) -> Vec<PathBuf> {
    children(dir)
        .into_iter()
        .filter(|p| fs::symlink_metadata(p).map(|m| m.is_dir()).unwrap_or(false))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_names_from_downloads() {
        assert_eq!(product_base("Gowin_V1.9.11.03Education_macOS.dmg"), "gowin");
        assert_eq!(product_base("oss-cad-suite-darwin-arm64-20261024.tgz"), "oss-cad-suite");
        assert_eq!(product_base("Envry-macos-universal.dmg"), "envry");
        assert_eq!(product_base("Visual Studio Code.dmg"), "visual-studio-code");
        assert_eq!(product_base("Postman-osx-11.2.0-arm64.zip"), "postman");
        assert_eq!(product_base("googlechrome.dmg"), "googlechrome");
        assert_eq!(product_base("Docker (1).dmg"), "docker");
    }

    #[test]
    fn installers_match_installed_apps() {
        let apps: Vec<(String, String)> = ["GowinIDE", "Google Chrome", "Visual Studio Code", "Pages"]
            .iter()
            .map(|a| (a.to_string(), normalize(a)))
            .collect();
        assert_eq!(matching_app(&apps, "gowin").as_deref(), Some("GowinIDE"));
        assert_eq!(matching_app(&apps, "googlechrome").as_deref(), Some("Google Chrome"));
        assert_eq!(matching_app(&apps, "visual-studio-code").as_deref(), Some("Visual Studio Code"));
        assert_eq!(matching_app(&apps, "docker-desktop"), None);
        assert_eq!(matching_app(&apps, "pagesmith"), None, "must match whole words");
        assert_eq!(matching_app(&apps, "zz"), None);
    }

    #[test]
    fn newest_puppeteer_version_sorts_last() {
        let mut names = ["mac_arm-147.0.7727.57", "mac_arm-131.0.6778.204", "mac_arm-143.0.7499.192"];
        names.sort_by_key(|n| version_key(n));
        assert_eq!(names.last(), Some(&"mac_arm-147.0.7727.57"));
        assert_eq!(version_label("mac_arm-147.0.7727.57"), "147.0.7727.57");
    }

    #[test]
    fn friendly_cache_names() {
        assert_eq!(describe_cache("Google").0, "Google Chrome");
        assert_eq!(describe_cache("beekeeper-studio-updater").0, "beekeeper studio update downloads");
        assert_eq!(describe_cache("ms-playwright").2, Safety::Review);
        assert_eq!(pretty_name("com.tinyapp.DBngin"), "DBngin");
    }
}
