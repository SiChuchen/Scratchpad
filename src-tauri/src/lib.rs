pub mod content;
pub mod models;
pub mod scratchpad;
pub mod storage;
pub mod system;
pub mod vault;

use chrono::Utc;
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
    let scale_factor = quick.scale_factor().unwrap_or(1.0);
    let (x, y, w, h) = system::window::fit_and_center_quick_access(work, scale_factor);
    let (min_w, min_h) = system::window::runtime_min_size(&work, scale_factor);
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

fn toggle_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    match system::window::visibility_toggle_action(window.is_visible().unwrap_or(false)) {
        system::window::VisibilityToggleAction::Hide => {
            let _ = window.hide();
        }
        system::window::VisibilityToggleAction::Show => {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

fn toggle_quick_access_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("quick-access") else {
        toggle_main_window(app);
        return;
    };
    match system::window::visibility_toggle_action(window.is_visible().unwrap_or(false)) {
        system::window::VisibilityToggleAction::Hide => {
            let _ = window.hide();
        }
        system::window::VisibilityToggleAction::Show => show_quick_access_centered(app),
    }
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

#[cfg(test)]
pub(crate) fn content_changed_event<T>(
    mutation: &content::models::ContentMutation<T>,
) -> content::models::ContentChangedEvent {
    content::ipc::content_changed_event(mutation)
}

#[allow(dead_code)]
pub(crate) fn dispatch_content_changed<T, E>(
    mutation: &content::models::ContentMutation<T>,
    emit: impl FnOnce(&str, content::models::ContentChangedEvent) -> Result<(), E>,
) {
    content::ipc::dispatch_content_changed(mutation, emit);
}

pub(crate) fn emit_content_changed<T>(
    app: &tauri::AppHandle,
    mutation: &content::models::ContentMutation<T>,
) {
    content::ipc::emit_content_changed(app, mutation);
}

#[cfg(test)]
mod dock_ipc_tests {
    use super::{content_changed_event, dispatch_content_changed};
    use crate::content::models::{ContentChange, ContentMutation, ContentOperation};

    #[test]
    fn ipc_content_changed_event_preserves_committed_revision_ids_and_operations() {
        let mutation = ContentMutation {
            value: (),
            revision: 42,
            changes: vec![
                ContentChange {
                    id: "dock:one".to_string(),
                    operation: ContentOperation::Updated,
                },
                ContentChange {
                    id: "dock:two".to_string(),
                    operation: ContentOperation::Reordered,
                },
            ],
        };

        let event = content_changed_event(&mutation);

        assert_eq!(event.revision, 42);
        assert_eq!(event.changes, mutation.changes);
    }

    #[test]
    fn ipc_dispatches_exact_content_changed_name_and_committed_payload() {
        let mutation = ContentMutation {
            value: (),
            revision: 43,
            changes: vec![ContentChange {
                id: "dock:actual-id".to_string(),
                operation: ContentOperation::Created,
            }],
        };
        let mut captured = None;

        dispatch_content_changed(&mutation, |event_name, payload| {
            captured = Some((event_name.to_string(), payload));
            Ok::<(), &'static str>(())
        });

        let (event_name, payload) = captured.expect("dispatch must invoke the emitter");
        assert_eq!(event_name, "content-changed");
        assert_eq!(payload.revision, 43);
        assert_eq!(payload.changes, mutation.changes);
    }

    #[test]
    fn ipc_dispatch_failure_is_non_fatal_after_commit() {
        let mutation = ContentMutation {
            value: (),
            revision: 44,
            changes: vec![ContentChange {
                id: "dock:committed".to_string(),
                operation: ContentOperation::Deleted,
            }],
        };
        let mut attempted = false;

        dispatch_content_changed(&mutation, |event_name, payload| {
            attempted = true;
            assert_eq!(event_name, "content-changed");
            assert_eq!(payload.revision, 44);
            Err::<(), &'static str>("forced emitter failure")
        });

        assert!(attempted);
    }
}

#[tauri::command]
fn ipc_entries_create_text(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    view: models::entry::EntryView,
    content: String,
    source: String,
) -> Result<models::entry::DockEntry, String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::storage::create_text_entry_with_revision(&mut conn, view, &content, &source)
            .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
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
    app: tauri::AppHandle,
    entry_id: String,
) -> Result<(), String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::storage::add_to_note_with_revision(&mut conn, &entry_id)
            .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(())
}

#[tauri::command]
fn ipc_entries_remove_from_view(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    view: models::entry::EntryView,
    entry_id: String,
) -> Result<(), String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::storage::remove_from_view_with_revision(&mut conn, view, &entry_id)
            .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(())
}

#[tauri::command]
fn ipc_entries_update_text(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
    content: String,
) -> Result<(), String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::storage::update_entry_text_with_revision(&mut conn, &id, &content)
            .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(())
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
    app: tauri::AppHandle,
    id: String,
    title: Option<String>,
) -> Result<(), String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::storage::rename_entry_with_revision(&mut conn, &id, title.as_deref())
            .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(())
}

#[tauri::command]
fn ipc_entries_reorder(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    view: models::entry::EntryView,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::storage::reorder_entries_with_revision(&mut conn, view, &ordered_ids)
            .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(())
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

/// 从主窗口 UI 打开全局 quick-access 面板。
///
/// UI 入口始终居中、显示并聚焦；全局快捷键保留独立的显示/隐藏切换行为。
#[tauri::command]
fn ipc_open_quick_access(app: tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("quick-access").is_none() {
        return Err("quick-access window not found".to_string());
    }
    show_quick_access_centered(&app);
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
                        toggle_main_window(&app_handle);
                    }
                })
                .map_err(|e| format!("failed to register shortcut: {e}"))?;
        }
        ShortcutTarget::QuickAccess => {
            app.global_shortcut()
                .on_shortcut(new_shortcut, move |app, _sc, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state == ShortcutState::Pressed {
                        toggle_quick_access_window(app);
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
    app: tauri::AppHandle,
    source_path: String,
    view: models::entry::EntryView,
) -> Result<models::entry::DockEntry, String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::assets::import_file_with_revision(&mut conn, &source_path, view)
            .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn ipc_entries_import_image_bytes(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    bytes: Vec<u8>,
    file_name: String,
    mime_type: String,
    width: Option<i64>,
    height: Option<i64>,
    view: models::entry::EntryView,
) -> Result<models::entry::DockEntry, String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::assets::import_image_bytes_with_revision(
            &mut conn, &bytes, &file_name, &mime_type, width, height, view,
        )
        .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
}

#[tauri::command]
fn ipc_entries_import_file_bytes(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    bytes: Vec<u8>,
    file_name: String,
    mime_type: Option<String>,
    view: models::entry::EntryView,
) -> Result<models::entry::DockEntry, String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        scratchpad::assets::import_file_bytes_with_revision(
            &mut conn,
            &bytes,
            &file_name,
            mime_type.as_deref(),
            view,
        )
        .map_err(|e| e.to_string())?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
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

/// 在系统文件管理器中显示该文件（Windows: explorer /select）。
/// 仅用于展示已入库资源的位置，路径来自后端详情数据而非用户任意输入。
#[tauri::command]
fn ipc_reveal_in_folder(path: String) -> Result<(), String> {
    let target = std::path::PathBuf::from(&path);
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", target.display()))
            .spawn()
            .map_err(|e| format!("failed to reveal in folder: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&target)
            .spawn()
            .map_err(|e| format!("failed to reveal in folder: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = target.parent().map(|d| d.to_path_buf()).unwrap_or(target);
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("failed to reveal in folder: {e}"))?;
    }
    Ok(())
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

fn initialize_schemas(
    conn: &mut Connection,
    cleanup_days: i64,
) -> storage::error::StorageResult<()> {
    content::migrations::validate_cleanup_days(cleanup_days)?;
    scratchpad::storage::ensure_dock_schema(conn)?;
    vault::storage::ensure_vault_schema(conn)?;
    content::migrations::ensure_content_schema(conn, cleanup_days)?;
    content::service::cleanup_expired(conn, Utc::now())?;
    Ok(())
}

#[cfg(test)]
mod init_db_tests {
    use rusqlite::{params, Connection};

    use super::initialize_schemas;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::storage::error::StorageError;
    use crate::storage::migration::get_schema_version;

    #[test]
    fn invalid_cleanup_days_stop_before_legacy_cleanup_or_schema_changes() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO entries(
                id, kind, content, source, created_at, updated_at
             ) VALUES (?1, 'text', 'keep me', 'fixture', ?2, ?2)",
            params!["home-only", "2026-07-18T08:00:00+00:00"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO home_entries(entry_id, created_at, sort_order)
             VALUES (?1, ?2, 0.0)",
            params!["home-only", "2026-07-18T08:00:00+00:00"],
        )
        .unwrap();

        let error = initialize_schemas(&mut conn, -1).unwrap_err();

        assert!(matches!(error, StorageError::Validation(_)));
        assert_eq!(get_schema_version(&conn).unwrap(), 2);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM entries WHERE id = 'home-only'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM home_entries WHERE entry_id = 'home-only'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        for table in ["vault_entries", "content_catalog"] {
            assert_eq!(
                conn.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
                     )",
                    params![table],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                0,
                "{table} should not be created"
            );
        }
    }

    #[test]
    fn zero_day_startup_backfills_before_unified_cleanup_and_bumps_revision() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO entries(
                id, kind, content, source, created_at, updated_at
             ) VALUES ('startup-due', 'text', 'remove me', 'fixture',
                       '2026-07-18T08:00:00Z', '2026-07-18T08:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO home_entries(entry_id, created_at, sort_order)
             VALUES ('startup-due', '2026-07-18T08:00:00Z', 0.0)",
            [],
        )
        .unwrap();

        initialize_schemas(&mut conn, 0).unwrap();

        assert_eq!(
            conn.query_row(
                "SELECT revision FROM content_state WHERE singleton=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        for sql in [
            "SELECT COUNT(*) FROM entries WHERE id='startup-due'",
            "SELECT COUNT(*) FROM home_entries WHERE entry_id='startup-due'",
            "SELECT COUNT(*) FROM content_catalog WHERE unified_id='dock:startup-due'",
            "SELECT COUNT(*) FROM content_fts WHERE unified_id='dock:startup-due'",
        ] {
            assert_eq!(
                conn.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap(),
                0,
                "{sql}"
            );
        }
    }

    #[test]
    fn startup_unified_cleanup_keeps_saved_and_future_temporary_content() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        conn.execute_batch(
            "INSERT INTO entries(id, kind, content, source, created_at, updated_at) VALUES
                 ('startup-future', 'text', 'future', 'fixture',
                  '2999-07-18T08:00:00Z', '2999-07-18T08:00:00Z'),
                 ('startup-saved', 'text', 'saved', 'fixture',
                  '2020-07-18T08:00:00Z', '2020-07-18T08:00:00Z');
             INSERT INTO home_entries(entry_id, created_at, sort_order) VALUES
                 ('startup-future', '2999-07-18T08:00:00Z', 0.0),
                 ('startup-saved', '2020-07-18T08:00:00Z', 1.0);
             INSERT INTO note_entries(entry_id, created_at, sort_order)
                 VALUES ('startup-saved', '2020-07-18T08:00:00Z', 0.0);",
        )
        .unwrap();

        initialize_schemas(&mut conn, 7).unwrap();

        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_catalog", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT revision FROM content_state WHERE singleton=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }
}

fn init_db() -> Connection {
    let mut conn = storage::connection::open_db().expect("Failed to open scratchpad DB");
    let cleanup_days = scratchpad::preferences::load_preferences(&conn)
        .map(|p| p.auto_cleanup_days)
        .unwrap_or(0);
    initialize_schemas(&mut conn, cleanup_days).expect("Failed to initialize database schemas");
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
        .manage(content::ipc::DeleteSchedulerState::default())
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
            ipc_reveal_in_folder,
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
            content::ipc::ipc_content_revision,
            content::ipc::ipc_content_list,
            content::ipc::ipc_content_detail,
            content::ipc::ipc_content_search_local,
            content::ipc::ipc_content_plan_search,
            content::ipc::ipc_content_cancel_search,
            content::ipc::ipc_open_main_content,
            content::ipc::ipc_content_update_text,
            content::ipc::ipc_content_rename,
            content::ipc::ipc_content_update_structured,
            content::ipc::ipc_content_save,
            content::ipc::ipc_content_unsave,
            content::ipc::ipc_content_reorder,
            content::ipc::ipc_content_delete,
            content::ipc::ipc_content_restore,
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
            vault::ipc::entries::ipc_vault_convert_ai_tag_to_manual,
            vault::ipc::search::ipc_vault_search_local,
            vault::ipc::search::ipc_vault_search_hybrid,
            vault::ipc::search::ipc_vault_cancel_search,
            vault::ipc::search::ipc_vault_create_capture_draft,
            vault::ipc::search::ipc_vault_get_outbound_audit,
            vault::ipc::settings::ipc_vault_get_ai_settings,
            vault::ipc::settings::ipc_vault_save_ai_settings,
            vault::ipc::settings::ipc_vault_get_llm_config_meta,
            vault::ipc::settings::ipc_vault_save_llm_config,
            vault::ipc::settings::ipc_vault_delete_llm_config,
            vault::ipc::settings::ipc_vault_test_llm_connection,
        ])
        .setup(|app| {
            // 主窗口默认几何：贴合屏幕右侧、垂直居中，物理像素通过 hwnd 精确控制。
            // 配置文件中的 dock_* 字段是逻辑像素，这里不使用；从第二屏启动时
            // 防止逻辑→物理转换误差导致窗口偏移到另一屏。
            // 如果 AppState.main_geometry 已保存（从 minimized-tab 还原），优先使用它。
            if let Some(window) = app.get_webview_window("main") {
                let state = app.state::<AppState>();
                let saved_geo = *state.main_geometry.lock().unwrap();
                system::window::set_main_window_default_geometry(&window, saved_geo);
            }

            // 注册两个全局快捷键。它们独立：一个失败不影响另一个。
            let app_handle = app.handle().clone();
            let app_handle2 = app.handle().clone();
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            let prefs = scratchpad::preferences::load_preferences(&conn).unwrap_or_default();
            drop(conn);

            let mut shortcuts = state.shortcuts.lock().unwrap();
            // Main: 显示/隐藏主窗口
            if let (Some(mods), Some(code)) = (
                parse_modifiers(&prefs.shortcut_modifiers),
                parse_key_code(&prefs.shortcut_key),
            ) {
                let sc = Shortcut::new(Some(mods), code);
                let ah = app_handle.clone();
                match app_handle.global_shortcut().on_shortcut(sc, move |_app, _sc, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state == ShortcutState::Pressed {
                        toggle_main_window(&ah);
                    }
                }) {
                    Ok(()) => shortcuts.main = Some(sc),
                    Err(e) => eprintln!("Failed to register main shortcut: {e}"),
                }
            }
            // Quick access: 显示/隐藏快速访问窗口
            if let (Some(mods), Some(code)) = (
                parse_modifiers(&prefs.quick_access_shortcut_modifiers),
                parse_key_code(&prefs.quick_access_shortcut_key),
            ) {
                let sc = Shortcut::new(Some(mods), code);
                let ah2 = app_handle2.clone();
                match app_handle2
                    .global_shortcut()
                    .on_shortcut(sc, move |app, _sc, event| {
                        use tauri_plugin_global_shortcut::ShortcutState;
                        if event.state == ShortcutState::Pressed {
                            toggle_quick_access_window(app);
                        }
                    })
                {
                    Ok(()) => shortcuts.quick_access = Some(sc),
                    Err(e) => eprintln!("Failed to register quick access shortcut: {e}"),
                }
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}
