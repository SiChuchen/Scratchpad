pub mod models;
pub mod scratchpad;
pub mod storage;
pub mod system;

use std::sync::Mutex;
use rusqlite::Connection;
use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

pub struct AppState {
    pub db: Mutex<Connection>,
    pub main_geometry: Mutex<Option<system::tab_controller::MainWindowGeometry>>,
    pub current_shortcut: Mutex<Option<Shortcut>>,
}

// --- Shortcut helpers ---

fn parse_modifiers(s: &str) -> Option<Modifiers> {
    let mut mods = Modifiers::empty();
    for part in s.split('+') {
        match part.trim() {
            "Alt" => mods |= Modifiers::ALT,
            "Shift" => mods |= Modifiers::SHIFT,
            "Ctrl" | "Control" => mods |= Modifiers::CONTROL,
            "Meta" | "Win" | "Super" => mods |= Modifiers::META,
            "" => {}
            _ => return None,
        }
    }
    if mods.is_empty() { None } else { Some(mods) }
}

fn parse_key_code(s: &str) -> Option<Code> {
    let upper = s.to_uppercase();
    if let Some(num) = upper.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
        return match num {
            1 => Some(Code::F1), 2 => Some(Code::F2), 3 => Some(Code::F3),
            4 => Some(Code::F4), 5 => Some(Code::F5), 6 => Some(Code::F6),
            7 => Some(Code::F7), 8 => Some(Code::F8), 9 => Some(Code::F9),
            10 => Some(Code::F10), 11 => Some(Code::F11), 12 => Some(Code::F12),
            _ => None,
        };
    }
    if upper.len() == 1 {
        let ch = upper.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            let idx = (ch as u8 - b'A') as usize;
            let codes: [Code; 26] = [
                Code::KeyA, Code::KeyB, Code::KeyC, Code::KeyD, Code::KeyE,
                Code::KeyF, Code::KeyG, Code::KeyH, Code::KeyI, Code::KeyJ,
                Code::KeyK, Code::KeyL, Code::KeyM, Code::KeyN, Code::KeyO,
                Code::KeyP, Code::KeyQ, Code::KeyR, Code::KeyS, Code::KeyT,
                Code::KeyU, Code::KeyV, Code::KeyW, Code::KeyX, Code::KeyY,
                Code::KeyZ,
            ];
            return Some(codes[idx]);
        }
        if ch.is_ascii_digit() {
            let codes: [Code; 10] = [
                Code::Digit0, Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4,
                Code::Digit5, Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9,
            ];
            return Some(codes[(ch as u8 - b'0') as usize]);
        }
    }
    None
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

// --- Shortcut IPC commands ---

#[derive(serde::Serialize)]
struct ShortcutStatus {
    modifiers: String,
    key: String,
    registered: bool,
}

#[tauri::command]
fn ipc_shortcut_status(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<ShortcutStatus, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let prefs = scratchpad::preferences::load_preferences(&conn).map_err(|e| e.to_string())?;
    drop(conn);
    let guard = state.current_shortcut.lock().map_err(|e| e.to_string())?;
    let registered = guard
        .as_ref()
        .map_or(false, |s| app.global_shortcut().is_registered(s.clone()));
    Ok(ShortcutStatus {
        modifiers: prefs.shortcut_modifiers,
        key: prefs.shortcut_key,
        registered,
    })
}

#[tauri::command]
fn ipc_shortcut_update(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    modifiers: String,
    key: String,
) -> Result<ShortcutStatus, String> {
    let mods = parse_modifiers(&modifiers).ok_or_else(|| format!("invalid modifiers: {modifiers}"))?;
    let code = parse_key_code(&key).ok_or_else(|| format!("invalid key: {key}"))?;
    let new_shortcut = Shortcut::new(Some(mods), code);

    // Unregister old shortcut
    let mut guard = state.current_shortcut.lock().map_err(|e| e.to_string())?;
    if let Some(ref old) = *guard {
        let _ = app.global_shortcut().unregister(old.clone());
    }

    // Register new shortcut with same toggle handler
    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(new_shortcut.clone(), move |_app, _sc, event| {
            use tauri_plugin_global_shortcut::ShortcutState;
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
        })
        .map_err(|e| format!("failed to register shortcut: {e}"))?;

    let registered = app.global_shortcut().is_registered(new_shortcut.clone());
    *guard = Some(new_shortcut);

    // Persist to preferences
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let mut prefs = scratchpad::preferences::load_preferences(&conn).map_err(|e| e.to_string())?;
        prefs.shortcut_modifiers = modifiers.clone();
        prefs.shortcut_key = key.clone();
        prefs.shortcut_registered = registered;
        scratchpad::preferences::save_preferences(&mut conn, &prefs).map_err(|e| e.to_string())?;
    }

    Ok(ShortcutStatus {
        modifiers,
        key,
        registered,
    })
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

// --- Clipboard IPC commands ---

#[tauri::command]
fn ipc_clipboard_copy_file(path: String) -> Result<(), String> {
    scratchpad::clipboard::copy_file(&path)
}

#[tauri::command]
fn ipc_clipboard_copy_image(path: String) -> Result<(), String> {
    scratchpad::clipboard::copy_image(&path)
}

#[tauri::command]
fn ipc_clipboard_read_file_paths() -> Result<Vec<String>, String> {
    scratchpad::clipboard::read_file_paths()
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
        GetWindowRect, SetWindowPos, SWP_NOZORDER,
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

    // 3. Get monitor work rect and calculate tab physical size + snap position
    let monitor = unsafe { MonitorFromWindow(main_hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { GetMonitorInfoW(monitor, &mut mi) };

    let tab_px = system::tab_controller::tab_physical_size(tab_hwnd);
    let tab_size = (tab_px, tab_px);

    let (snap_x, snap_y) = system::tab_controller::calc_snap_position(
        &main_rect,
        &mi.rcWork,
        tab_size,
        0.0, // Full-visibility mode: tab stays entirely within work area
    );

    // 4. Install subclass (idempotent)
    system::tab_controller::install(&app, tab_hwnd);

    // 5. SetWindowPos FIRST — position and size tab at final location
    unsafe {
        SetWindowPos(tab_hwnd, std::ptr::null_mut(), snap_x, snap_y, tab_px, tab_px, SWP_NOZORDER);
    }

    // 6. Apply circle region AFTER SetWindowPos (region based on actual window size)
    system::window::apply_circle_region(&app, "minimized-tab")?;

    // 7. Show minimized-tab
    tab_w.show().map_err(|e| e.to_string())?;

    // 8. Re-apply circle region after show (window now visible, GetWindowRect reliable)
    system::window::apply_circle_region(&app, "minimized-tab")?;

    // 9. Hide main window
    main_w.hide().map_err(|e| e.to_string())?;

    Ok(())
}

// --- DB initialization ---

fn init_db() -> Connection {
    let mut conn = storage::connection::open_db().expect("Failed to open scratchpad DB");
    let cleanup_days = scratchpad::preferences::load_preferences(&conn)
        .map(|p| p.auto_cleanup_days)
        .unwrap_or(0);
    scratchpad::storage::ensure_dock_schema(&mut conn, cleanup_days)
        .expect("Failed to init scratch dock schema");
    conn
}

// --- App entry ---

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            db: Mutex::new(init_db()),
            main_geometry: Mutex::new(None),
            current_shortcut: Mutex::new(None),
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
            ipc_clipboard_copy_file,
            ipc_clipboard_copy_image,
            ipc_clipboard_read_file_paths,
            ipc_preferences_get,
            ipc_preferences_set,
            ipc_preferences_list_fonts,
            ipc_shortcut_status,
            ipc_shortcut_update,
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

            // Global shortcut: load from preferences, register, report status
            {
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                let prefs = scratchpad::preferences::load_preferences(&conn).unwrap_or_default();
                drop(conn);

                let mods = parse_modifiers(&prefs.shortcut_modifiers)
                    .unwrap_or(Modifiers::ALT | Modifiers::SHIFT);
                let code = parse_key_code(&prefs.shortcut_key)
                    .unwrap_or(Code::KeyV);
                let shortcut = Shortcut::new(Some(mods), code);

                let app_handle = app.handle().clone();
                let reg_result = app.global_shortcut().on_shortcut(
                    shortcut.clone(),
                    move |_app, _sc, event| {
                        use tauri_plugin_global_shortcut::ShortcutState;
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

                let registered = reg_result.is_ok()
                    && app.global_shortcut().is_registered(shortcut.clone());

                // Persist registration status
                {
                    let mut conn = state.db.lock().unwrap();
                    let mut prefs = scratchpad::preferences::load_preferences(&conn).unwrap_or_default();
                    prefs.shortcut_registered = registered;
                    let _ = scratchpad::preferences::save_preferences(&mut conn, &prefs);
                }

                let mut guard = state.current_shortcut.lock().unwrap();
                *guard = Some(shortcut);
            }

            // Ensure window is focused on startup so keyboard/paste events work
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }

            // The minimized tab is a transparent shaped HWND. Disable DWM show/hide
            // transitions so Windows does not animate a cached rectangular frame.
            let _ = system::window::disable_dwm_transitions(app.handle(), "minimized-tab");

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
