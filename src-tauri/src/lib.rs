//! Sweep: finds files your Mac doesn't need and deletes the ones you pick.

pub mod cleaner;
pub mod disk;
pub mod registry;
pub mod scanner;
pub mod sizes;

use scanner::{Env, Item, ScanOptions, ScanResult};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

/// Items from the latest scan. Cleaning only accepts IDs found here, so the
/// window can never ask Sweep to delete an arbitrary path.
#[derive(Default)]
struct AppState {
    items: Mutex<HashMap<String, Item>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Overview {
    home: String,
    disk: disk::DiskInfo,
    full_disk_access: bool,
}

#[tauri::command]
fn overview() -> Overview {
    let env = Env::system();
    Overview {
        home: env.home.display().to_string(),
        disk: disk::disk_info(),
        full_disk_access: env.full_disk_access,
    }
}

#[tauri::command]
async fn scan(app: AppHandle, state: State<'_, AppState>, options: ScanOptions) -> Result<ScanResult, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        let env = Env::system();
        scanner::scan(&env, &options, &|step: &str| {
            let _ = app.emit("scan-progress", step);
        })
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut items = state.items.lock().unwrap();
    items.clear();
    for item in result.categories.iter().flat_map(|c| &c.items) {
        items.insert(item.id.clone(), item.clone());
    }
    Ok(result)
}

#[tauri::command]
async fn clean(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>) -> Result<cleaner::CleanReport, String> {
    let items: Vec<Item> = {
        let known = state.items.lock().unwrap();
        ids.iter()
            .map(|id| {
                known
                    .get(id)
                    .cloned()
                    .ok_or_else(|| "These results are out of date. Scan again, then clean.".to_string())
            })
            .collect::<Result<_, _>>()?
    };
    let report = tauri::async_runtime::spawn_blocking(move || {
        let env = Env::system();
        cleaner::clean(&env, &items, &|progress| {
            let _ = app.emit("clean-progress", progress);
        })
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut known = state.items.lock().unwrap();
    for outcome in report.outcomes.iter().filter(|o| o.ok) {
        known.remove(&outcome.id);
    }
    Ok(report)
}

/// Takes back ownership of root-owned files in an item (macOS asks for the
/// admin password), then cleans it again.
#[tauri::command]
async fn fix_and_clean(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<cleaner::CleanReport, String> {
    let item = state.items.lock().unwrap().get(&id).cloned();
    let item = item.ok_or("Scan again, then try this.")?;
    let report = tauri::async_runtime::spawn_blocking(move || {
        let env = Env::system();
        cleaner::take_ownership(&env, &item)?;
        Ok::<_, String>(cleaner::clean(&env, &[item], &|progress| {
            let _ = app.emit("clean-progress", progress);
        }))
    })
    .await
    .map_err(|e| e.to_string())??;
    if report.outcomes.iter().all(|o| o.ok) {
        state.items.lock().unwrap().remove(&id);
    }
    Ok(report)
}

/// Shows an item in Finder.
#[tauri::command]
fn reveal(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let path = {
        let known = state.items.lock().unwrap();
        known
            .get(&id)
            .and_then(|item| item.paths.iter().find(|p| Path::new(p).exists()).cloned())
    }
    .ok_or("This is no longer on disk.")?;
    Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens System Settings at Privacy & Security → Full Disk Access.
#[tauri::command]
fn open_full_disk_access() -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            overview,
            scan,
            clean,
            fix_and_clean,
            reveal,
            open_full_disk_access
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sweep");
}
