//! Scans and cleans a fake home folder, and checks that only the right
//! things disappear.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use sweep_lib::cleaner::{check_target, clean};
use sweep_lib::scanner::{now_secs, scan, Env, Item, Safety, ScanOptions, ScanResult};

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    File::create(path).unwrap().write_all(&vec![7u8; bytes]).unwrap();
}

fn age(path: &Path, days: u64) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
        .unwrap();
}

fn env_for(home: &Path, apps: &Path) -> Env {
    Env {
        home: home.to_path_buf(),
        app_dirs: vec![apps.to_path_buf()],
        bin_dirs: Vec::new(),
        min_bytes: 1,
        now: now_secs(),
        check_processes: false,
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

fn find<'a>(items: &'a [Item], path_end: &str) -> Option<&'a Item> {
    items.iter().find(|i| i.paths.iter().any(|p| p.ends_with(path_end)))
}

struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    env: Env,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let apps = dir.path().join("Applications");
    fs::create_dir_all(apps.join("Foo.app")).unwrap();

    write(&home.join(".npm/_cacache/content/aa"), 10_000);
    write(&home.join("Library/Caches/SomeApp/blob"), 10_000);
    write(&home.join("Library/Caches/com.apple.Safari/blob"), 10_000);
    write(&home.join("Library/Application Support/KeepMe/data"), 10_000);
    write(&home.join(".cursor/extensions/x"), 10_000);
    write(&home.join("Downloads/Foo-2.1-arm64.dmg"), 10_000);
    write(&home.join("Downloads/Mystery.dmg"), 10_000);
    write(&home.join("Downloads/notes.txt"), 10_000);

    // A project nobody has touched for 100 days...
    let old = home.join("code/old-site");
    write(&old.join("package.json"), 100);
    write(&old.join("src/index.js"), 100);
    write(&old.join("node_modules/react/index.js"), 10_000);
    fs::create_dir_all(old.join(".git")).unwrap();
    for f in ["package.json", "src/index.js", "node_modules/react/index.js"] {
        age(&old.join(f), 100);
    }
    // ...and one being worked on today.
    let new = home.join("code/new-site");
    write(&new.join("package.json"), 100);
    write(&new.join("node_modules/x/index.js"), 10_000);
    write(&new.join(".next/cache/a"), 10_000);

    let env = env_for(&home, &apps);
    Fixture { _dir: dir, home, env }
}

#[test]
fn scan_finds_the_right_things() {
    let f = fixture();
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
    assert_eq!(old.group.as_deref(), Some("~/code/old-site"));
    assert!(!find(&projects, "new-site/node_modules").unwrap().preselect);
    assert!(!find(&projects, "new-site/.next").unwrap().preselect);

    let everything: Vec<&Item> = result.categories.iter().flat_map(|c| &c.items).collect();
    assert!(
        everything.iter().all(|i| i.paths.iter().all(|p| !p.contains("KeepMe"))),
        "data of unknown apps is never listed"
    );
}

#[test]
fn cleaning_removes_only_selected_items() {
    let f = fixture();
    let result = scan(&f.env, &ScanOptions::default(), &|_: &str| {});
    let selected: Vec<Item> = result
        .categories
        .iter()
        .flat_map(|c| c.items.iter())
        .filter(|i| i.preselect && i.enabled)
        .cloned()
        .collect();
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
        assert!(!h.join(gone).exists(), "{gone} should be deleted");
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
        assert!(h.join(kept).exists(), "{kept} should still be there");
    }
}

#[test]
fn files_that_cannot_be_deleted_are_reported() {
    let f = fixture();
    let locked = f.home.join("Library/Caches/SomeApp");
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
fn guard_refuses_important_paths() {
    let f = fixture();
    let h = &f.home;
    for bad in [
        h.clone(),
        h.join("Library"),
        h.join("Library/Caches"),
        h.join("Documents"),
        h.join("Downloads"),
        h.join(".ssh/id_ed25519"),
        h.join("Library/Keychains/login.keychain-db"),
        h.join("code/../Documents"),
        PathBuf::from("/etc/hosts"),
        PathBuf::from("relative/path"),
    ] {
        assert!(check_target(h, &bad).is_err(), "{} should be refused", bad.display());
    }
    for ok in [
        h.join(".npm/_cacache"),
        h.join("Library/Caches/SomeApp"),
        h.join("Downloads/Foo-2.1-arm64.dmg"),
        h.join(".cursor"),
    ] {
        assert!(check_target(h, &ok).is_ok(), "{} should be allowed", ok.display());
    }
}

#[test]
fn guard_refuses_folders_that_link_outside_home() {
    let f = fixture();
    let outside = f._dir.path().join("outside");
    fs::create_dir_all(outside.join("victim")).unwrap();
    std::os::unix::fs::symlink(&outside, f.home.join("link")).unwrap();
    assert!(check_target(&f.home, &f.home.join("link/victim")).is_err());
}
