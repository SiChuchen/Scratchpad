pub mod models;
pub mod scratchpad;
pub mod storage;
pub mod system;

use std::sync::Mutex;
use rusqlite::Connection;
use tauri::Manager;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub main_geometry: Mutex<Option<system::tab_controller::MainWindowGeometry>>,
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
fn ipc_entries_rename(
    state: tauri::State<AppState>,
    id: String,
    title: Option<String>,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    scratchpad::storage::rename_entry(&mut conn, &id, title.as_deref()).map_err(|e| e.to_string())
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

// --- Native window region ---

#[tauri::command]
fn ipc_window_apply_circle_region(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    system::window::apply_circle_region(&app, &label)
}

#[tauri::command]
fn ipc_window_clear_region(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    system::window::clear_region(&app, &label)
}

#[tauri::command]
fn ipc_dock_restore_from_tab(
    app: tauri::AppHandle,
) -> Result<(), String> {
    system::window::restore_from_tab(&app)
}

#[tauri::command]
fn ipc_dock_minimize_to_tab(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, ShowWindow, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
        SW_SHOWNOACTIVATE,
    };

    let main_w = app.get_webview_window("main").ok_or("main window not found")?;
    let tab_w = app.get_webview_window("minimized-tab").ok_or("minimized-tab window not found")?;

    let main_hwnd = main_w.hwnd().map_err(|e| e.to_string())?.0 as HWND;
    let tab_hwnd = tab_w.hwnd().map_err(|e| e.to_string())?.0 as HWND;

    // 1. Save main window geometry (physical coordinates)
    let mut main_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(main_hwnd, &mut main_rect) };
    let geo = system::tab_controller::MainWindowGeometry {
        x: main_rect.left,
        y: main_rect.top,
        width: main_rect.right - main_rect.left,
        height: main_rect.bottom - main_rect.top,
    };
    *state.main_geometry.lock().unwrap() = Some(geo);

    // 2. Sync to DockPreferences (physical → logical, single db guard)
    {
        let dpi = unsafe { GetDpiForWindow(main_hwnd) };
        let scale = dpi as f64 / 96.0;
        let mut db = state.db.lock().unwrap();
        let mut prefs = scratchpad::preferences::load_preferences(&db)
            .map_err(|e| e.to_string())?;
        prefs.dock_position_x = geo.x as f64 / scale;
        prefs.dock_position_y = geo.y as f64 / scale;
        prefs.dock_width = geo.width as f64 / scale;
        prefs.dock_height = geo.height as f64 / scale;
        scratchpad::preferences::save_preferences(&mut db, &prefs)
            .map_err(|e| e.to_string())?;
        drop(db);
    }

    // 3. Get monitor work rect
    let monitor = unsafe { MonitorFromWindow(main_hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { GetMonitorInfoW(monitor, &mut mi) };

    // 4. Get tab physical size
    let mut tab_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(tab_hwnd, &mut tab_rect) };
    let tab_size = (tab_rect.right - tab_rect.left, tab_rect.bottom - tab_rect.top);

    // 5. Calculate snap position with default hidden ratio
    let (snap_x, snap_y) = system::tab_controller::calc_snap_position(
        &main_rect,
        &mi.rcWork,
        tab_size,
        system::tab_controller::DEFAULT_HIDDEN_RATIO,
    );

    // 6. Install subclass (idempotent)
    system::tab_controller::install(&app, tab_hwnd);

    // 7. Apply circle region (idempotent)
    system::window::apply_circle_region(&app, "minimized-tab")?;

    // 8. Position tab and show it
    unsafe {
        SetWindowPos(tab_hwnd, std::ptr::null_mut(), snap_x, snap_y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
        ShowWindow(tab_hwnd, SW_SHOWNOACTIVATE);
    }

    // 9. Hide main
    unsafe { ShowWindow(main_hwnd, SW_HIDE) };

    Ok(())
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            db: Mutex::new(init_db()),
            main_geometry: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            ipc_entries_create_text,
            ipc_entries_list,
            ipc_entries_add_to_note,
            ipc_entries_remove_from_view,
            ipc_entries_update_text,
            ipc_entries_toggle_collapse,
            ipc_entries_rename,
            ipc_entries_reorder,
            ipc_entries_import_file,
            ipc_entries_import_image_bytes,
            ipc_entries_import_file_bytes,
            ipc_preferences_get,
            ipc_preferences_set,
            ipc_preferences_list_fonts,
            ipc_toggle_always_on_top,
            ipc_window_apply_circle_region,
            ipc_window_clear_region,
            ipc_dock_restore_from_tab,
            ipc_dock_minimize_to_tab,
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
            }

            // Set window icon for all windows (taskbar, alt-tab, etc.)
            let icon_result = (|| -> Option<tauri::image::Image> {
                if let Ok(icon) = tauri::image::Image::from_path("icons/icon.ico") {
                    return Some(icon);
                }
                // Dev mode fallback: resolve relative to exe directory
                let exe = std::env::current_exe().ok()?;
                let exe_dir = exe.parent()?;
                let candidates = [
                    exe_dir.join("icons").join("icon.ico"),
                    exe_dir.join("..").join("..").join("icons").join("icon.ico"),
                ];
                for path in &candidates {
                    if let Ok(icon) = tauri::image::Image::from_path(path) {
                        return Some(icon);
                    }
                }
                None
            })();

            if let Some(icon) = icon_result {
                for label in ["main", "minimized-tab"] {
                    if let Some(w) = app.get_webview_window(label) {
                        let _ = w.set_icon(icon.clone());
                    }
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
