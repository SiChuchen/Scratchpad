pub mod models;
pub mod scratchpad;
pub mod storage;
pub mod system;

use std::sync::Mutex;
use rusqlite::Connection;
use tauri::Manager;

pub struct AppState {
    pub db: Mutex<Connection>,
}

// --- Dock entry IPC commands ---

#[tauri::command]
fn ipc_entries_create_text(
    state: tauri::State<AppState>,
    view: models::entry::EntryView,
    content: String,
    source: String,
) -> Result<models::entry::DockEntry, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::storage::create_text_entry(&mut conn, view, &content, &source)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_entries_list(
    state: tauri::State<AppState>,
    view: models::entry::EntryView,
    kind: Option<models::entry::EntryKind>,
) -> Result<Vec<models::entry::DockEntry>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::storage::list_entries(&conn, view, kind).map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_entries_add_to_note(
    state: tauri::State<AppState>,
    entry_id: String,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::storage::add_to_note(&mut conn, &entry_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_entries_remove_from_view(
    state: tauri::State<AppState>,
    view: models::entry::EntryView,
    entry_id: String,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::storage::remove_from_view(&mut conn, view, &entry_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_entries_update_text(
    state: tauri::State<AppState>,
    id: String,
    content: String,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::storage::update_entry_text(&mut conn, &id, &content).map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_entries_toggle_collapse(
    state: tauri::State<AppState>,
    id: String,
    collapsed: bool,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::storage::toggle_collapse(&mut conn, &id, collapsed).map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_entries_reorder(
    state: tauri::State<AppState>,
    view: models::entry::EntryView,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::storage::reorder_entries(&mut conn, view, &ordered_ids).map_err(|e| e.to_string())
}

// --- Preferences IPC commands ---

#[tauri::command]
fn ipc_preferences_get(
    state: tauri::State<AppState>,
) -> Result<models::preferences::DockPreferences, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::preferences::load_preferences(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_preferences_set(
    state: tauri::State<AppState>,
    prefs: models::preferences::DockPreferences,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::preferences::save_preferences(&mut conn, &prefs).map_err(|e| e.to_string())
}

// --- Asset import IPC commands ---

#[tauri::command]
fn ipc_entries_import_file(
    state: tauri::State<AppState>,
    source_path: String,
    view: models::entry::EntryView,
) -> Result<models::entry::DockEntry, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::assets::import_file(&mut conn, &source_path, view).map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_entries_import_image_bytes(
    state: tauri::State<AppState>,
    bytes: Vec<u8>,
    file_name: String,
    mime_type: String,
    width: Option<i64>,
    height: Option<i64>,
    view: models::entry::EntryView,
) -> Result<models::entry::DockEntry, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::assets::import_image_bytes(
        &mut conn,
        &bytes,
        &file_name,
        &mime_type,
        width,
        height,
        view,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_entries_import_file_bytes(
    state: tauri::State<AppState>,
    bytes: Vec<u8>,
    file_name: String,
    mime_type: Option<String>,
    view: models::entry::EntryView,
) -> Result<models::entry::DockEntry, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::assets::import_file_bytes(
        &mut conn,
        &bytes,
        &file_name,
        mime_type.as_deref(),
        view,
    )
    .map_err(|e| e.to_string())
}

// --- System IPC commands ---

#[tauri::command]
fn ipc_preferences_list_fonts() -> Result<Vec<String>, String> {
    Ok(system::fonts::list_installed_fonts())
}

// --- Window control ---

#[tauri::command]
async fn ipc_toggle_always_on_top(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let window = app
        .get_webview_window("main")
        .ok_or("No main window")?;
    let current = window.is_always_on_top().map_err(|e| e.to_string())?;
    window
        .set_always_on_top(!current)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"always_on_top": !current}))
}

// --- DB initialization ---

fn init_db() -> Connection {
    let mut conn = storage::connection::open_db().expect("Failed to open scratchpad DB");
    scratchpad::storage::ensure_dock_schema(&mut conn)
        .expect("Failed to init scratch dock schema");
    conn
}

// --- App entry ---

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(AppState {
            db: Mutex::new(init_db()),
        })
        .invoke_handler(tauri::generate_handler![
            ipc_entries_create_text,
            ipc_entries_list,
            ipc_entries_add_to_note,
            ipc_entries_remove_from_view,
            ipc_entries_update_text,
            ipc_entries_toggle_collapse,
            ipc_entries_reorder,
            ipc_entries_import_file,
            ipc_entries_import_image_bytes,
            ipc_entries_import_file_bytes,
            ipc_preferences_get,
            ipc_preferences_set,
            ipc_preferences_list_fonts,
            ipc_toggle_always_on_top,
        ])
        .setup(|app| {
            // System tray menu
            let show_item = tauri::menu::MenuItem::with_id(
                app,
                "show",
                "显示主窗口",
                true,
                None::<&str>,
            )?;
            let quit_item = tauri::menu::MenuItem::with_id(
                app,
                "quit",
                "退出",
                true,
                None::<&str>,
            )?;
            let menu = tauri::menu::Menu::with_items(app, &[&show_item, &quit_item])?;

            let tray = app.tray_by_id("main").expect("tray icon exists");
            tray.set_menu(Some(menu))?;
            tray.on_menu_event(move |app, event| match event.id().as_ref() {
                "show" => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            });

            // Global shortcut: Alt+Shift+V (toggle show/hide)
            use tauri_plugin_global_shortcut::{
                Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
            };
            let shortcut =
                Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyV);
            let app_handle = app.handle().clone();
            let _ = app.global_shortcut().on_shortcut(
                shortcut,
                move |_app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        if let Some(w) = app_handle.get_webview_window("main") {
                            if w.is_visible().unwrap_or(false) {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    }
                },
            );

            // Ensure window is focused on startup so keyboard/paste events work
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
                // Set window icon for taskbar
                if let Ok(icon) = tauri::image::Image::from_path("icons/icon.ico") {
                    let _ = w.set_icon(icon);
                } else {
                    // Dev mode fallback: resolve relative to source dir
                    if let Ok(exe) = std::env::current_exe() {
                        if let Some(exe_dir) = exe.parent() {
                            let candidates = [
                                exe_dir.join("icons").join("icon.ico"),
                                exe_dir.join("..").join("..").join("icons").join("icon.ico"),
                            ];
                            for path in &candidates {
                                if let Ok(icon) = tauri::image::Image::from_path(path) {
                                    let _ = w.set_icon(icon);
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
