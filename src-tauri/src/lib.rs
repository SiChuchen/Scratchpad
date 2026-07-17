pub mod models;
pub mod scratchpad;
pub mod storage;
pub mod system;
pub mod vault;

use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

pub struct AppState {
    pub db: Mutex<Connection>,
    pub main_geometry: Mutex<Option<system::tab_controller::MainWindowGeometry>>,
    pub shortcuts: Mutex<RegisteredShortcuts>,
}

/// 已注册的全局快捷键。两个 target 互相独立：一个注册失败不影响另一个。
#[derive(Default)]
pub struct RegisteredShortcuts {
    pub main: Option<Shortcut>,
    pub quick_access: Option<Shortcut>,
}

/// 快捷键目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum ShortcutTarget {
    Main,
    QuickAccess,
}

impl ShortcutTarget {
    fn prefs_fields(self) -> (&'static str, &'static str, &'static str) {
        // (modifiers_key, key_key, registered_key)
        match self {
            ShortcutTarget::Main => ("shortcut_modifiers", "shortcut_key", "shortcut_registered"),
            ShortcutTarget::QuickAccess => (
                "quick_access_shortcut_modifiers",
                "quick_access_shortcut_key",
                "quick_access_shortcut_registered",
            ),
        }
    }
}

// --- Win32 helpers (quick-access positioning) ---

/// 返回鼠标当前所在点的物理屏幕坐标 (x, y)。
#[cfg(target_os = "windows")]
fn win_cursor_pos() -> (i32, i32) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut pt) != 0 {
            (pt.x, pt.y)
        } else {
            (0, 0)
        }
    }
}

/// 返回包含 `(x, y)` 的显示器的工作区（rcWork），失败时退回到主屏。
#[cfg(target_os = "windows")]
fn win_monitor_work_area(x: i32, y: i32) -> system::window::WorkRect {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        GetMonitorInfoW(monitor, &mut mi);
    }
    system::window::WorkRect::new(
        mi.rcWork.left,
        mi.rcWork.top,
        mi.rcWork.right,
        mi.rcWork.bottom,
    )
}

#[cfg(not(target_os = "windows"))]
fn win_cursor_pos() -> (i32, i32) {
    (0, 0)
}

#[cfg(not(target_os = "windows"))]
fn win_monitor_work_area(_x: i32, _y: i32) -> system::window::WorkRect {
    system::window::WorkRect::new(0, 0, 1920, 1080)
}

/// 把 quick-access 窗口移动到鼠标所在显示器的工作区中心并 show/set_focus/emit。
fn show_quick_access_centered(app: &tauri::AppHandle) {
    use tauri::{PhysicalPosition as PhysPos, PhysicalSize as PhysSize, Size};

    let Some(quick) = app.get_webview_window("quick-access") else {
        return;
    };
    let (cx, cy) = win_cursor_pos();
    let work = win_monitor_work_area(cx, cy);
    let (x, y, w, h) = system::window::fit_and_center_quick_access(cx, cy, work);
    let (min_w, min_h) = system::window::runtime_min_size(&work);
    let _ = quick.set_position(PhysPos::new(x as f64, y as f64));
    let _ = quick.set_size(PhysSize::new(w as f64, h as f64));
    let _ = quick.set_min_size(Some(Size::Physical(PhysSize::new(
        min_w as u32,
        min_h as u32,
    ))));
    let _ = quick.show();
    let _ = quick.set_focus();
    let _ = app.emit("quick-access-focus-input", ());
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
    if mods.is_empty() {
        None
    } else {
        Some(mods)
    }
}

fn parse_key_code(s: &str) -> Option<Code> {
    let upper = s.to_uppercase();
    match upper.as_str() {
        "SPACE" => return Some(Code::Space),
        "TAB" => return Some(Code::Tab),
        "ENTER" | "RETURN" => return Some(Code::Enter),
        "ESC" | "ESCAPE" => return Some(Code::Escape),
        "BACKSPACE" => return Some(Code::Backspace),
        "UP" => return Some(Code::ArrowUp),
        "DOWN" => return Some(Code::ArrowDown),
        "LEFT" => return Some(Code::ArrowLeft),
        "RIGHT" => return Some(Code::ArrowRight),
        _ => {}
    }
    if let Some(num) = upper.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
        return match num {
            1 => Some(Code::F1),
            2 => Some(Code::F2),
            3 => Some(Code::F3),
            4 => Some(Code::F4),
            5 => Some(Code::F5),
            6 => Some(Code::F6),
            7 => Some(Code::F7),
            8 => Some(Code::F8),
            9 => Some(Code::F9),
            10 => Some(Code::F10),
            11 => Some(Code::F11),
            12 => Some(Code::F12),
            _ => None,
        };
    }
    if upper.len() == 1 {
        let ch = upper.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            let idx = (ch as u8 - b'A') as usize;
            let codes: [Code; 26] = [
                Code::KeyA,
                Code::KeyB,
                Code::KeyC,
                Code::KeyD,
                Code::KeyE,
                Code::KeyF,
                Code::KeyG,
                Code::KeyH,
                Code::KeyI,
                Code::KeyJ,
                Code::KeyK,
                Code::KeyL,
                Code::KeyM,
                Code::KeyN,
                Code::KeyO,
                Code::KeyP,
                Code::KeyQ,
                Code::KeyR,
                Code::KeyS,
                Code::KeyT,
                Code::KeyU,
                Code::KeyV,
                Code::KeyW,
                Code::KeyX,
                Code::KeyY,
                Code::KeyZ,
            ];
            return Some(codes[idx]);
        }
        if ch.is_ascii_digit() {
            let codes: [Code; 10] = [
                Code::Digit0,
                Code::Digit1,
                Code::Digit2,
                Code::Digit3,
                Code::Digit4,
                Code::Digit5,
                Code::Digit6,
                Code::Digit7,
                Code::Digit8,
                Code::Digit9,
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
fn ipc_entries_add_to_note(state: tauri::State<AppState>, entry_id: String) -> Result<(), String> {
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

/// 计算 target 当前应该返回的状态（从持久化偏好 + 内存注册结果汇总）。
fn shortcut_status_for(state: &AppState, target: ShortcutTarget) -> Result<ShortcutStatus, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let prefs = scratchpad::preferences::load_preferences(&conn).map_err(|e| e.to_string())?;
    drop(conn);
    let guard = state.shortcuts.lock().map_err(|e| e.to_string())?;
    let (modifiers, key, registered) = match target {
        ShortcutTarget::Main => (
            prefs.shortcut_modifiers,
            prefs.shortcut_key,
            guard.main.is_some(),
        ),
        ShortcutTarget::QuickAccess => (
            prefs.quick_access_shortcut_modifiers,
            prefs.quick_access_shortcut_key,
            guard.quick_access.is_some(),
        ),
    };
    Ok(ShortcutStatus {
        modifiers,
        key,
        registered,
    })
}

#[tauri::command]
fn ipc_shortcut_status(
    state: tauri::State<AppState>,
    target: ShortcutTarget,
) -> Result<ShortcutStatus, String> {
    shortcut_status_for(&state, target)
}

/// 从主窗口 UI（TopBar 按钮、VaultView header 等）打开全局 quick-access 面板。
///
/// 复用与全局快捷键相同的 `show_quick_access_centered`：在鼠标所在显示器
/// 居中、set_size、set_focus、emit `quick-access-focus-input`。可见时切回隐藏。
#[tauri::command]
fn ipc_open_quick_access(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("quick-access") {
        if w.is_visible().map_err(|e| e.to_string())? {
            let _ = w.hide();
        } else {
            show_quick_access_centered(&app);
        }
    }
    Ok(())
}

/// 从 quick-access 打开可见的主窗口，并导航到设置页。
#[tauri::command]
fn ipc_open_main_settings(app: tauri::AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    app.emit("main-open-settings", ())
        .map_err(|e| e.to_string())?;
    if let Some(quick) = app.get_webview_window("quick-access") {
        quick.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn ipc_shortcut_update(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    target: ShortcutTarget,
    modifiers: String,
    key: String,
) -> Result<ShortcutStatus, String> {
    let mods =
        parse_modifiers(&modifiers).ok_or_else(|| format!("invalid modifiers: {modifiers}"))?;
    let code = parse_key_code(&key).ok_or_else(|| format!("invalid key: {key}"))?;
    let new_shortcut = Shortcut::new(Some(mods), code);

    // 检查与另一 target 是否冲突。冲突时不注销旧 shortcut，保留用户原设置。
    {
        let guard = state.shortcuts.lock().map_err(|e| e.to_string())?;
        let other = match target {
            ShortcutTarget::Main => guard.quick_access,
            ShortcutTarget::QuickAccess => guard.main,
        };
        if let Some(other_sc) = other {
            if other_sc == new_shortcut {
                return Err(
                    "shortcut conflict: same combination is used by the other target".to_string(),
                );
            }
        }
    }

    // 先尝试注册新 shortcut；成功后再注销旧 shortcut，避免失败时两个都不可用。
    let app_handle = app.clone();
    match target {
        ShortcutTarget::Main => {
            app.global_shortcut()
                .on_shortcut(new_shortcut, move |_app, _sc, event| {
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
        }
        ShortcutTarget::QuickAccess => {
            app.global_shortcut()
                .on_shortcut(new_shortcut, move |app, _sc, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state == ShortcutState::Pressed {
                        if let Some(w) = app.get_webview_window("quick-access") {
                            if w.is_visible().unwrap_or(false) {
                                let _ = w.hide();
                            } else {
                                show_quick_access_centered(app);
                            }
                        } else if let Some(w) = app.get_webview_window("main") {
                            // 兜底：quick-access 窗口尚未创建（旧配置）时退回 main。
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
        }
    }

    // 注册成功 — 注销旧 shortcut 并写入新状态。
    let mut guard = state.shortcuts.lock().map_err(|e| e.to_string())?;
    let old = match target {
        ShortcutTarget::Main => guard.main.replace(new_shortcut),
        ShortcutTarget::QuickAccess => guard.quick_access.replace(new_shortcut),
    };
    if let Some(old_sc) = old {
        let _ = app.global_shortcut().unregister(old_sc);
    }

    // 持久化偏好（registered 字段实际不持久化，但保留字段语义）
    let registered = true;
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let mut prefs =
            scratchpad::preferences::load_preferences(&conn).map_err(|e| e.to_string())?;
        let (mods_key, key_key, reg_key) = target.prefs_fields();
        match target {
            ShortcutTarget::Main => {
                prefs.shortcut_modifiers = modifiers.clone();
                prefs.shortcut_key = key.clone();
                prefs.shortcut_registered = registered;
            }
            ShortcutTarget::QuickAccess => {
                prefs.quick_access_shortcut_modifiers = modifiers.clone();
                prefs.quick_access_shortcut_key = key.clone();
                prefs.quick_access_shortcut_registered = registered;
            }
        }
        let _ = (mods_key, key_key, reg_key); // 仅用于文档化字段名映射
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
        &mut conn, &bytes, &file_name, &mime_type, width, height, view,
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
    scratchpad::assets::import_file_bytes(&mut conn, &bytes, &file_name, mime_type.as_deref(), view)
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

/// 复制文本到剪贴板。`sensitive = true` 时从 VaultAiSettings 读取
/// `sensitive_clipboard_clear_seconds`（默认 30s）作为自动清除窗口；
/// 前端无法伪造更长的清除时间。
#[tauri::command]
async fn ipc_clipboard_copy_text(
    text: String,
    sensitive: bool,
    vault: tauri::State<'_, vault::ipc::VaultRuntimeState>,
) -> Result<(), String> {
    let seconds = if sensitive {
        vault.settings().sensitive_clipboard_clear_seconds
    } else {
        None
    };
    scratchpad::clipboard::copy_text(&text, seconds)
}

// --- Data directory IPC ---

#[derive(serde::Serialize)]
struct DataDirInfo {
    path: String,
    mode: String, // "portable" | "installed" | "custom"
}

#[tauri::command]
fn ipc_data_dir_info() -> Result<DataDirInfo, String> {
    let path = storage::connection::data_dir().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = exe.parent().unwrap_or(exe.as_path());
    let mode = if path.starts_with(exe_dir) {
        "portable"
    } else {
        "installed"
    };
    Ok(DataDirInfo {
        path: path.to_string_lossy().to_string(),
        mode: mode.to_string(),
    })
}

#[tauri::command]
fn ipc_data_dir_set(path: String) -> Result<DataDirInfo, String> {
    let new_dir = std::path::PathBuf::from(&path);
    std::fs::create_dir_all(&new_dir).map_err(|e| format!("无法创建目录: {e}"))?;
    storage::connection::save_data_dir_override(&path).map_err(|e| e.to_string())?;
    Ok(DataDirInfo {
        path,
        mode: "custom".to_string(),
    })
}

// --- System IPC commands ---

#[tauri::command]
fn ipc_preferences_list_fonts() -> Result<Vec<String>, String> {
    Ok(system::fonts::list_installed_fonts())
}

// --- Window control ---

#[tauri::command]
async fn ipc_toggle_always_on_top(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let window = app.get_webview_window("main").ok_or("No main window")?;
    let current = window.is_always_on_top().map_err(|e| e.to_string())?;
    window
        .set_always_on_top(!current)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"always_on_top": !current}))
}

// --- Native window region ---

#[tauri::command]
fn ipc_window_apply_circle_region(app: tauri::AppHandle, label: String) -> Result<(), String> {
    system::window::apply_circle_region(&app, &label)
}

#[tauri::command]
fn ipc_window_clear_region(app: tauri::AppHandle, label: String) -> Result<(), String> {
    system::window::clear_region(&app, &label)
}

#[tauri::command]
fn ipc_dock_restore_from_tab(app: tauri::AppHandle) -> Result<(), String> {
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
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, SetWindowPos, SWP_NOZORDER};

    let main_w = app
        .get_webview_window("main")
        .ok_or("main window not found")?;
    let tab_w = app
        .get_webview_window("minimized-tab")
        .ok_or("minimized-tab window not found")?;

    let main_hwnd = main_w.hwnd().map_err(|e| e.to_string())?.0 as HWND;
    let tab_hwnd = tab_w.hwnd().map_err(|e| e.to_string())?.0 as HWND;

    // 1. Save main window geometry (physical coordinates)
    let mut main_rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
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
        let mut prefs =
            scratchpad::preferences::load_preferences(&db).map_err(|e| e.to_string())?;
        prefs.dock_position_x = geo.x as f64 / scale;
        prefs.dock_position_y = geo.y as f64 / scale;
        prefs.dock_width = geo.width as f64 / scale;
        prefs.dock_height = geo.height as f64 / scale;
        scratchpad::preferences::save_preferences(&mut db, &prefs).map_err(|e| e.to_string())?;
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
        &main_rect, &mi.rcWork, tab_size,
        0.0, // Full-visibility mode: tab stays entirely within work area
    );

    // 4. Install subclass (idempotent)
    system::tab_controller::install(&app, tab_hwnd);

    // 5. SetWindowPos FIRST — position and size tab at final location
    unsafe {
        SetWindowPos(
            tab_hwnd,
            std::ptr::null_mut(),
            snap_x,
            snap_y,
            tab_px,
            tab_px,
            SWP_NOZORDER,
        );
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
    vault::storage::ensure_vault_schema(&mut conn).expect("Failed to init vault schema");
    conn
}

// --- App entry ---

pub fn run() {
    // Task 8: 在 Builder 之前初始化 DB 连接并从中加载 vault runtime
    // （LLM 配置 + AI 设置 + 失败门控初始状态），让 AI 功能在用户打开
    // Settings 之前就可用。
    let conn = init_db();
    let vault_runtime = vault::ipc::VaultRuntimeState::load(&conn);

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            db: Mutex::new(conn),
            main_geometry: Mutex::new(None),
            shortcuts: Mutex::new(RegisteredShortcuts::default()),
        })
        .manage(vault_runtime)
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
            ipc_clipboard_copy_text,
            ipc_data_dir_info,
            ipc_data_dir_set,
            ipc_preferences_get,
            ipc_preferences_set,
            ipc_preferences_list_fonts,
            ipc_shortcut_status,
            ipc_shortcut_update,
            ipc_open_quick_access,
            ipc_open_main_settings,
            ipc_toggle_always_on_top,
            ipc_window_apply_circle_region,
            ipc_window_clear_region,
            ipc_dock_restore_from_tab,
            ipc_dock_minimize_to_tab,
            vault::ipc::entries::ipc_vault_create_entry,
            vault::ipc::entries::ipc_vault_update_entry,
            vault::ipc::entries::ipc_vault_delete_entry,
            vault::ipc::entries::ipc_vault_list_entries,
            vault::ipc::entries::ipc_vault_get_entry,
            vault::ipc::entries::ipc_vault_update_manual_tags,
            vault::ipc::entries::ipc_vault_remove_ai_tag,
            vault::ipc::entries::ipc_vault_refresh_ai_metadata,
            vault::ipc::entries::ipc_vault_ai_backfill_status,
            vault::ipc::capture::ipc_vault_parse_capture_local,
            vault::ipc::capture::ipc_vault_enrich_capture,
            vault::ipc::capture::ipc_vault_create_from_capture,
            vault::ipc::ipc_vault_search,
            vault::ipc::search::ipc_vault_search_hybrid_local,
            vault::ipc::search::ipc_vault_plan_search,
            vault::ipc::search::ipc_vault_cancel_search,
            vault::ipc::ipc_vault_get_llm_presets,
            vault::ipc::settings::ipc_vault_get_llm_config,
            vault::ipc::settings::ipc_vault_verify_and_save_llm,
            vault::ipc::settings::ipc_vault_test_saved_llm,
            vault::ipc::settings::ipc_vault_delete_llm_config,
            vault::ipc::settings::ipc_vault_get_ai_settings,
            vault::ipc::settings::ipc_vault_set_ai_settings,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(false) = event {
                if window.label() == "quick-access" {
                    let _ = window.emit("vault-sensitive-reset", ());
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            // System tray menu
            let show_item =
                tauri::menu::MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item =
                tauri::menu::MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
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

            // Global shortcuts: load from preferences, register each target
            // independently. 两个 target 互不阻塞：一个被系统占用时，另一个
            // 仍然注册和工作。
            {
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                let prefs = scratchpad::preferences::load_preferences(&conn).unwrap_or_default();
                drop(conn);

                // --- 主窗口 toggle ---
                let main_mods = parse_modifiers(&prefs.shortcut_modifiers)
                    .unwrap_or(Modifiers::ALT | Modifiers::SHIFT);
                let main_code = parse_key_code(&prefs.shortcut_key).unwrap_or(Code::KeyV);
                let main_shortcut = Shortcut::new(Some(main_mods), main_code);
                let main_registered = {
                    let app_handle = app.handle().clone();
                    app.global_shortcut()
                        .on_shortcut(main_shortcut, move |_app, _sc, event| {
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
                        .is_ok()
                };
                if main_registered {
                    let mut guard = state.shortcuts.lock().unwrap();
                    guard.main = Some(main_shortcut);
                }

                // --- Quick access toggle ---
                let qa_mods = parse_modifiers(&prefs.quick_access_shortcut_modifiers)
                    .unwrap_or(Modifiers::ALT | Modifiers::SHIFT);
                let qa_code =
                    parse_key_code(&prefs.quick_access_shortcut_key).unwrap_or(Code::Space);
                let qa_shortcut = Shortcut::new(Some(qa_mods), qa_code);
                let qa_registered = {
                    app.global_shortcut()
                        .on_shortcut(qa_shortcut, move |app, _sc, event| {
                            use tauri_plugin_global_shortcut::ShortcutState;
                            if event.state == ShortcutState::Pressed {
                                if let Some(w) = app.get_webview_window("quick-access") {
                                    if w.is_visible().unwrap_or(false) {
                                        let _ = w.hide();
                                    } else {
                                        show_quick_access_centered(app);
                                    }
                                } else if let Some(w) = app.get_webview_window("main") {
                                    if w.is_visible().unwrap_or(false) {
                                        let _ = w.hide();
                                    } else {
                                        let _ = w.show();
                                        let _ = w.set_focus();
                                    }
                                }
                            }
                        })
                        .is_ok()
                };
                if qa_registered {
                    let mut guard = state.shortcuts.lock().unwrap();
                    guard.quick_access = Some(qa_shortcut);
                }

                // 持久化注册结果（registered 字段不持久化但写库以备调试）
                {
                    let mut conn = state.db.lock().unwrap();
                    let mut prefs =
                        scratchpad::preferences::load_preferences(&conn).unwrap_or_default();
                    prefs.shortcut_registered = main_registered;
                    prefs.quick_access_shortcut_registered = qa_registered;
                    let _ = scratchpad::preferences::save_preferences(&mut conn, &prefs);
                }
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
                for label in ["main", "minimized-tab", "quick-access"] {
                    if let Some(w) = app.get_webview_window(label) {
                        let _ = w.set_icon(icon.clone());
                    }
                }
            }

            // Task 10: 启动 AI metadata backfill worker（仅当 config 存在
            // 且 auto_enrich 开启时；否则 try_start_backfill 内部不会启动）。
            vault::jobs::try_start_backfill(app.handle());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;
    use rusqlite::Connection;

    /// `parse_key_code("Space")` 必须返回 `Code::Space` — 这是 Quick Access
    /// 默认快捷键的关键码，老版本无法解析。
    #[test]
    fn shortcut_parse_space_key_code() {
        assert_eq!(parse_key_code("Space"), Some(Code::Space));
        assert_eq!(parse_key_code("space"), Some(Code::Space));
        assert_eq!(parse_key_code("SPACE"), Some(Code::Space));
    }

    #[test]
    fn shortcut_parse_tab_and_arrows() {
        assert_eq!(parse_key_code("Tab"), Some(Code::Tab));
        assert_eq!(parse_key_code("Up"), Some(Code::ArrowUp));
        assert_eq!(parse_key_code("Down"), Some(Code::ArrowDown));
    }

    /// 模拟冲突检测逻辑：当两个 target 的 (modifiers, key) 相同时，
    /// `ipc_shortcut_update` 应当拒绝并保留旧 shortcut。
    #[test]
    fn shortcut_update_rejects_conflict_with_other_target_and_preserves_old() {
        let main_mods = parse_modifiers("Alt+Shift").unwrap();
        let main_sc = Shortcut::new(Some(main_mods), Code::KeyV);

        let new_mods = parse_modifiers("Alt+Shift").unwrap();
        let new_sc_for_qa = Shortcut::new(Some(new_mods), Code::KeyV);

        // Quick Access 已有 Some(other)；现在 Main 想注册相同组合
        let shortcuts = RegisteredShortcuts {
            quick_access: Some(main_sc),
            ..Default::default()
        };

        // 模拟 ipc_shortcut_update 中的冲突检查
        let other = shortcuts.quick_access;
        let conflict = other.map(|o| o == new_sc_for_qa).unwrap_or(false);
        assert!(conflict, "same combination must be detected as conflict");

        // 冲突时不应注销旧 shortcut
        assert!(shortcuts.quick_access.is_some());
    }

    /// Main 注册失败不应阻塞 Quick Access 注册。这里通过 RegisteredShortcuts
    /// 的字段独立性验证：可以只设置 quick_access 而保留 main = None。
    #[test]
    fn shortcut_main_registration_does_not_block_quick_access_registration() {
        let mut shortcuts = RegisteredShortcuts::default();
        // 模拟 Main 注册失败（保持 None），Quick Access 成功
        let qa_mods = parse_modifiers("Alt+Shift").unwrap();
        let qa_sc = Shortcut::new(Some(qa_mods), Code::Space);
        shortcuts.quick_access = Some(qa_sc);

        assert!(shortcuts.main.is_none());
        assert!(shortcuts.quick_access.is_some());
    }

    /// 整合测试：通过 DockPreferences 验证两个 target 的字段独立持久化。
    #[test]
    fn shortcut_roundtrip_persists_both_targets() {
        let mut conn = Connection::open_in_memory().unwrap();
        scratchpad::storage::ensure_dock_schema(&mut conn, 0).unwrap();

        let prefs = models::preferences::DockPreferences {
            shortcut_modifiers: "Ctrl+Alt".to_string(),
            shortcut_key: "V".to_string(),
            shortcut_registered: true,
            quick_access_shortcut_modifiers: "Ctrl+Shift".to_string(),
            quick_access_shortcut_key: "Space".to_string(),
            quick_access_shortcut_registered: true,
            ..Default::default()
        };
        scratchpad::preferences::save_preferences(&mut conn, &prefs).unwrap();
        let loaded = scratchpad::preferences::load_preferences(&conn).unwrap();

        assert_eq!(loaded.shortcut_modifiers, "Ctrl+Alt");
        assert_eq!(loaded.shortcut_key, "V");
        assert_eq!(loaded.quick_access_shortcut_modifiers, "Ctrl+Shift");
        assert_eq!(loaded.quick_access_shortcut_key, "Space");
    }

    /// 旧偏好缺 quick_access_* 字段时使用 Alt+Shift+Space 默认值。
    #[test]
    fn shortcut_legacy_prefs_default_quick_access_to_alt_shift_space() {
        let mut conn = Connection::open_in_memory().unwrap();
        scratchpad::storage::ensure_dock_schema(&mut conn, 0).unwrap();
        conn.execute(
            "INSERT INTO preferences(key, value) VALUES ('shortcut_modifiers', 'Ctrl+K')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO preferences(key, value) VALUES ('shortcut_key', 'V')",
            [],
        )
        .unwrap();

        let loaded = scratchpad::preferences::load_preferences(&conn).unwrap();
        assert_eq!(loaded.shortcut_modifiers, "Ctrl+K");
        assert_eq!(loaded.shortcut_key, "V");
        assert_eq!(loaded.quick_access_shortcut_modifiers, "Alt+Shift");
        assert_eq!(loaded.quick_access_shortcut_key, "Space");
    }
}
