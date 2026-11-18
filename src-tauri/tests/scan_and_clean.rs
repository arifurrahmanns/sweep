//! Scans and cleans fake home folders laid out like macOS and like Windows,
//! and checks that only the right things disappear. Both layouts run on any
//! system; a few Windows-only behaviors are tested only on Windows.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::time::{Duration, SystemTime};
use sweep_lib::cleaner::{check_target_on, clean};
use sweep_lib::os::Os;
use sweep_lib::scanner::{join_rel, now_secs, scan, Env, Item, Safety, ScanOptions, ScanResult};

/// `base/rel` with the system's own separators.
fn at(base: &Path, rel: &str) -> PathBuf {
    join_rel(base, rel)
}

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    File::create(path).unwrap().write_all(&vec![7u8; bytes]).unwrap();
}

fn age(path: &Path, days: u64) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
        .unwrap();
}

fn env_for(os: Os, home: &Path, app_dirs: Vec<PathBuf>) -> Env {
    Env {
        os,
        home: home.to_path_buf(),
        app_dirs,
        bin_dirs: Vec::new(),
        min_bytes: 1,
        now: now_secs(),
        check_processes: false,
        full_disk_access: true,
    }
}

fn items(result: &ScanResult, category: &str) -> Vec<Item> {
    result
        .categories
        .iter()
        .find(|c| c.id == category)
        .map(|c| c.items.clone())
        .unwrap_or_default()
}

/// An item with a path ending in `path_end` (written with `/`).
fn find<'a>(items: &'a [Item], path_end: &str) -> Option<&'a Item> {
    items
        .iter()
        .find(|i| i.paths.iter().any(|p| p.replace('\\', "/").ends_with(path_end)))
}

fn selected(result: &ScanResult) -> Vec<Item> {
    result
        .categories
        .iter()
        .flat_map(|c| c.items.iter())
        .filter(|i| i.preselect && i.enabled)
        .cloned()
        .collect()
}

fn group(rel: &str) -> String {
    format!("~{MAIN_SEPARATOR}{}", rel.replace('/', std::path::MAIN_SEPARATOR_STR))
}

struct Fixture {
    dir: tempfile::TempDir,
    home: PathBuf,
    env: Env,
}

/// Projects both layouts share: one untouched for 100 days, one in use.
fn add_projects(home: &Path) {
    let old = at(home, "code/old-site");
    write(&at(&old, "package.json"), 100);
    write(&at(&old, "src/index.js"), 100);
    write(&at(&old, "node_modules/react/index.js"), 10_000);
    fs::create_dir_all(at(&old, ".git")).unwrap();
    for f in ["package.json", "src/index.js", "node_modules/react/index.js"] {
        age(&at(&old, f), 100);
    }
    let new = at(home, "code/new-site");
    write(&at(&new, "package.json"), 100);
    write(&at(&new, "node_modules/x/index.js"), 10_000);
    write(&at(&new, ".next/cache/a"), 10_000);
}

// ------------------------------------------------------------------ macOS

fn mac_fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let apps = dir.path().join("Applications");
    fs::create_dir_all(apps.join("Foo.app")).unwrap();

    for rel in [
        ".npm/_cacache/content/aa",
        "Library/Caches/SomeApp/blob",
        "Library/Caches/com.apple.Safari/blob",
        "Library/Application Support/KeepMe/data",
        ".cursor/extensions/x",
        "Downloads/Foo-2.1-arm64.dmg",
        "Downloads/Mystery.dmg",
        "Downloads/notes.txt",
    ] {
        write(&at(&home, rel), 10_000);
    }
    add_projects(&home);

    let env = env_for(Os::Mac, &home, vec![apps]);
    Fixture { dir, home, env }
}

#[test]
fn mac_scan_finds_the_right_things() {
    let f = mac_fixture();
    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});

    let dev = items(&result, "dev");
    assert!(find(&dev, ".npm/_cacache").unwrap().preselect);

    let caches = items(&result, "caches");
    assert!(find(&caches, "Library/Caches/SomeApp").unwrap().preselect);
    assert!(find(&caches, "com.apple.Safari").is_none(), "Apple's own caches are left alone");

    let leftovers = items(&result, "leftovers");
    let cursor = leftovers.iter().find(|i| i.title == "Cursor").expect("Cursor leftovers");
    assert_eq!(cursor.safety, Safety::Review);
    assert!(!cursor.preselect, "leftovers are never preselected");

    let installers = items(&result, "installers");
    let foo = find(&installers, "Foo-2.1-arm64.dmg").unwrap();
    assert!(foo.preselect && foo.detail.contains("Foo"), "{}", foo.detail);
    assert!(!find(&installers, "Mystery.dmg").unwrap().preselect);
    assert!(find(&installers, "notes.txt").is_none());

    let projects = items(&result, "projects");
    let old = find(&projects, "old-site/node_modules").unwrap();
    assert!(old.preselect, "inactive project is preselected");
    assert_eq!(old.group.as_deref(), Some(group("code/old-site").as_str()));
    assert!(!find(&projects, "new-site/node_modules").unwrap().preselect);
    assert!(!find(&projects, "new-site/.next").unwrap().preselect);

    let everything: Vec<&Item> = result.categories.iter().flat_map(|c| &c.items).collect();
    assert!(
        everything.iter().all(|i| i.paths.iter().all(|p| !p.contains("KeepMe"))),
        "data of unknown apps is never listed"
    );
}

#[test]
fn mac_cleaning_removes_only_selected_items() {
    let f = mac_fixture();
    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});
    let selected = selected(&result);
    assert!(selected.len() >= 4, "selected {}", selected.len());

    let report = clean(&f.env, &selected, &|_| {});
    for outcome in &report.outcomes {
        assert!(outcome.ok, "{}: {:?}", outcome.title, outcome.message);
    }

    let h = &f.home;
    for gone in [
        ".npm/_cacache",
        "Library/Caches/SomeApp",
        "Downloads/Foo-2.1-arm64.dmg",
        "code/old-site/node_modules",
    ] {
        assert!(!at(h, gone).exists(), "{gone} should be deleted");
    }
    for kept in [
        "code/old-site/package.json",
        "code/old-site/src/index.js",
        "code/new-site/node_modules",
        "code/new-site/.next",
        "Downloads/Mystery.dmg",
        "Downloads/notes.txt",
        "Library/Caches/com.apple.Safari",
        "Library/Application Support/KeepMe",
        ".cursor",
    ] {
        assert!(at(h, kept).exists(), "{kept} should still be there");
    }
}

#[cfg(unix)]
#[test]
fn files_that_cannot_be_deleted_are_reported() {
    use std::os::unix::fs::PermissionsExt;

    let f = mac_fixture();
    let locked = at(&f.home, "Library/Caches/SomeApp");
    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});
    let item = find(&items(&result, "caches"), "Library/Caches/SomeApp").unwrap().clone();

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let report = clean(&f.env, &[item], &|_| {});
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    let outcome = &report.outcomes[0];
    assert!(!outcome.ok && outcome.partial);
    assert_eq!(outcome.message.as_deref(), Some("1 file couldn't be deleted."));
    assert!(outcome.hint.as_deref().unwrap().contains("Full Disk Access"));
}

#[test]
fn mac_guard_refuses_important_paths() {
    let f = mac_fixture();
    let h = &f.home;
    for bad in [
        h.clone(),
        at(h, "Library"),
        at(h, "Library/Caches"),
        at(h, "Documents"),
        at(h, "Downloads"),
        at(h, ".ssh/id_ed25519"),
        at(h, "Library/Keychains/login.keychain-db"),
        h.join("code").join("..").join("Documents"),
        PathBuf::from("/etc/hosts"),
        PathBuf::from("relative/path"),
    ] {
        assert!(check_target_on(Os::Mac, h, &bad).is_err(), "{} should be refused", bad.display());
    }
    for ok in [
        ".npm/_cacache",
        "Library/Caches/SomeApp",
        "Downloads/Foo-2.1-arm64.dmg",
        ".cursor",
    ] {
        assert!(check_target_on(Os::Mac, h, &at(h, ok)).is_ok(), "{ok} should be allowed");
    }
}

#[cfg(unix)]
#[test]
fn guard_refuses_folders_that_link_outside_home() {
    let f = mac_fixture();
    let outside = f.dir.path().join("outside");
    fs::create_dir_all(outside.join("victim")).unwrap();
    std::os::unix::fs::symlink(&outside, f.home.join("link")).unwrap();
    assert!(check_target_on(Os::Mac, &f.home, &f.home.join("link/victim")).is_err());
}

#[test]
fn without_full_disk_access_protected_folders_are_skipped() {
    let mut f = mac_fixture();
    write(&at(&f.home, "Documents/app/package.json"), 100);
    write(&at(&f.home, "Documents/app/node_modules/x/index.js"), 10_000);

    f.env.full_disk_access = false;
    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});
    assert!(!result.full_disk_access);
    assert!(items(&result, "installers").is_empty(), "Downloads is skipped");
    let projects = items(&result, "projects");
    assert!(find(&projects, "Documents/app/node_modules").is_none(), "Documents is skipped");
    assert!(find(&projects, "old-site/node_modules").is_some(), "other folders are still scanned");
    for place in ["Documents", "Downloads"] {
        assert!(result.skipped.iter().any(|s| s == place), "{place} listed as skipped: {:?}", result.skipped);
    }

    f.env.full_disk_access = true;
    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});
    assert!(result.skipped.is_empty());
    assert!(find(&items(&result, "projects"), "Documents/app/node_modules").is_some());
    assert!(!items(&result, "installers").is_empty());
}

// ---------------------------------------------------------------- Windows

fn windows_fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let start_menu = dir.path().join("Start Menu");
    write(&at(&start_menu, "Foo/Foo.lnk"), 10);
    fs::create_dir_all(at(&home, "AppData/Local/Programs/Microsoft VS Code")).unwrap();

    for rel in [
        "AppData/Local/npm-cache/_cacache/content/aa",
        "AppData/Roaming/Slack/Cache/Cache_Data/f",
        "AppData/Roaming/Slack/storage/keep",
        "AppData/Local/Google/Chrome/User Data/Default/Cache/Cache_Data/f",
        "AppData/Local/Google/Chrome/User Data/Default/History",
        "AppData/Local/Temp/old.tmp",
        "AppData/Local/Temp/new.tmp",
        "AppData/Roaming/Cursor/data",
        "AppData/Roaming/Code/User/settings.json",
        "AppData/Local/Docker/wsl/disk/docker_data.vhdx",
        "Downloads/Foo-Setup-2.1-x64.exe",
        "Downloads/Mystery.exe",
        "Downloads/notes.txt",
        "code/api/bin/Debug/Api.dll",
        "code/api/obj/project.assets.json",
    ] {
        write(&at(&home, rel), 10_000);
    }
    write(&at(&home, "code/api/Api.csproj"), 100);
    age(&at(&home, "AppData/Local/Temp/old.tmp"), 5);
    for rel in ["code/api/Api.csproj", "code/api/bin/Debug/Api.dll", "code/api/obj/project.assets.json"] {
        age(&at(&home, rel), 100);
    }
    add_projects(&home);

    let programs = at(&home, "AppData/Local/Programs");
    let env = env_for(Os::Windows, &home, vec![start_menu, programs]);
    Fixture { dir, home, env }
}

#[test]
fn windows_scan_finds_the_right_things() {
    let f = windows_fixture();
    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});

    assert!(find(&items(&result, "dev"), "npm-cache/_cacache").unwrap().preselect);

    let caches = items(&result, "caches");
    let slack = caches.iter().find(|i| i.title == "Slack").expect("Slack cache");
    assert!(slack.preselect);
    assert!(slack.paths.iter().all(|p| !p.contains("storage")), "only cache folders: {:?}", slack.paths);
    let chrome = caches.iter().find(|i| i.title == "Google Chrome").expect("Chrome cache");
    assert!(chrome.paths.iter().all(|p| !p.ends_with("History")));
    let temp = find(&caches, "AppData/Local/Temp").expect("Temp files");
    assert_eq!(temp.title, "Temporary files");
    assert!(temp.preselect);

    let leftovers = items(&result, "leftovers");
    let cursor = leftovers.iter().find(|i| i.title == "Cursor").expect("Cursor leftovers");
    assert!(!cursor.preselect);
    assert!(
        leftovers.iter().all(|i| i.title != "Visual Studio Code"),
        "installed apps aren't leftovers"
    );

    let installers = items(&result, "installers");
    let foo = find(&installers, "Foo-Setup-2.1-x64.exe").unwrap();
    assert!(foo.preselect && foo.detail.contains("Foo"), "{}", foo.detail);
    assert!(!find(&installers, "Mystery.exe").unwrap().preselect);
    assert!(find(&installers, "notes.txt").is_none());

    let projects = items(&result, "projects");
    assert!(find(&projects, "code/api/bin").unwrap().preselect, ".NET build output");
    assert!(find(&projects, "code/api/obj").unwrap().preselect);
    assert!(find(&projects, "old-site/node_modules").unwrap().preselect);
    assert!(!find(&projects, "new-site/node_modules").unwrap().preselect);

    let docker = items(&result, "docker");
    assert_eq!(docker.len(), 2);
    let purge = docker.iter().find(|i| i.id == "docker:purge").unwrap();
    assert_eq!(purge.safety, Safety::Info, "Docker's disk is never deleted on Windows");
    assert!(!purge.enabled);
}

#[test]
fn windows_cleaning_removes_only_selected_items() {
    let f = windows_fixture();
    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});
    let report = clean(&f.env, &selected(&result), &|_| {});
    for outcome in &report.outcomes {
        assert!(outcome.ok, "{}: {:?}", outcome.title, outcome.message);
    }

    let h = &f.home;
    for gone in [
        "AppData/Local/npm-cache/_cacache",
        "AppData/Roaming/Slack/Cache",
        "AppData/Local/Google/Chrome/User Data/Default/Cache",
        "AppData/Local/Temp/old.tmp",
        "Downloads/Foo-Setup-2.1-x64.exe",
        "code/api/bin",
        "code/api/obj",
        "code/old-site/node_modules",
    ] {
        assert!(!at(h, gone).exists(), "{gone} should be deleted");
    }
    for kept in [
        "AppData/Roaming/Slack/storage/keep",
        "AppData/Local/Google/Chrome/User Data/Default/History",
        "AppData/Local/Temp/new.tmp",
        "AppData/Roaming/Cursor/data",
        "AppData/Roaming/Code/User/settings.json",
        "AppData/Local/Docker/wsl/disk/docker_data.vhdx",
        "Downloads/Mystery.exe",
        "Downloads/notes.txt",
        "code/api/Api.csproj",
        "code/new-site/node_modules",
    ] {
        assert!(at(h, kept).exists(), "{kept} should still be there");
    }
}

#[test]
fn windows_guard_refuses_important_paths() {
    let f = windows_fixture();
    let h = &f.home;
    for bad in [
        "AppData",
        "AppData/Local",
        "appdata/local",
        "AppData/Roaming",
        "AppData/Local/Temp",
        "AppData/Local/Programs",
        "Documents",
        "Downloads",
        "OneDrive",
        ".ssh/id_ed25519",
        "AppData/Roaming/Microsoft/Credentials/abc",
        "AppData/Local/Packages/Some.App/LocalState",
        "appdata/local/packages/some.app",
    ] {
        assert!(check_target_on(Os::Windows, h, &at(h, bad)).is_err(), "{bad} should be refused");
    }
    assert!(check_target_on(Os::Windows, h, h).is_err());
    for ok in [
        "AppData/Local/npm-cache/_cacache",
        "AppData/Roaming/Slack/Cache",
        "AppData/Local/Temp/old.tmp",
        "AppData/Local/Microsoft/TypeScript",
        "Downloads/Foo-Setup-2.1-x64.exe",
    ] {
        assert!(check_target_on(Os::Windows, h, &at(h, ok)).is_ok(), "{ok} should be allowed");
    }
}

#[cfg(windows)]
#[test]
fn junctions_are_removed_without_touching_their_target() {
    let f = windows_fixture();
    let store = at(f.dir.path(), "store/pkg");
    write(&at(&store, "index.js"), 100);
    let link = at(&f.home, "code/old-site/node_modules/linked");
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&store)
        .status()
        .unwrap();
    assert!(status.success(), "mklink /J failed");

    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});
    let item = find(&items(&result, "projects"), "old-site/node_modules").unwrap().clone();
    let report = clean(&f.env, &[item], &|_| {});
    assert!(report.outcomes[0].ok, "{:?}", report.outcomes[0].message);
    assert!(!at(&f.home, "code/old-site/node_modules").exists());
    assert!(at(&store, "index.js").exists(), "the junction's target must survive");
}

#[cfg(windows)]
#[test]
fn read_only_files_are_deleted() {
    let f = windows_fixture();
    let file = at(&f.home, "AppData/Local/npm-cache/_cacache/content/aa");
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();

    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});
    let item = find(&items(&result, "dev"), "npm-cache/_cacache").unwrap().clone();
    let report = clean(&f.env, &[item], &|_| {});
    assert!(report.outcomes[0].ok, "{:?}", report.outcomes[0].message);
    assert!(!file.exists());
}
