use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use windows_sys::Win32::Foundation::{HWND, LRESULT, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, VK_LBUTTON,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindow, GetWindowRect, KillTimer, SetForegroundWindow, SetTimer, SetWindowPos,
    GW_CHILD, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WM_ACTIVATE, WM_CANCELMODE,
    WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCDESTROY,
    WM_TIMER,
};

// EnableWindow is not exported by windows-sys 0.59 under Win32_UI_WindowsAndMessaging.
// Link directly to user32.
#[link(name = "user32")]
extern "system" {
    fn EnableWindow(hwnd: HWND, benable: i32) -> i32;
}

// --- Constants ---

pub const TAB_LONG_PRESS_TIMER_ID: usize = 1;
pub const LONG_PRESS_MS: u32 = 200;
pub const DEFAULT_HIDDEN_RATIO: f32 = 0.0;
pub const MAX_HIDDEN_RATIO: f32 = 1.0 / 2.0;
pub const TAB_LOGICAL_SIZE: i32 = 48;
pub const TAB_EDGE_MARGIN: i32 = 4;
const TAB_OFFSCREEN_HIDE_COORD: i32 = -32000;

/// Calculate the tab window's physical pixel size from its DPI.
/// Uses integer arithmetic to avoid floating-point drift: (logical * dpi + 48) / 96
/// This is the single source of truth for tab size — used by region, snap, and SetWindowPos.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn tab_physical_size(hwnd: HWND) -> i32 {
    let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) };
    (TAB_LOGICAL_SIZE * dpi as i32 + 48) / 96
}

/// Re-apply circular region directly via Win32.
/// Called from the subclass proc on WM_ACTIVATE to repair region lost by focus changes.
fn reapply_circle_region(hwnd: HWND) {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateEllipticRgn, DeleteObject, RedrawWindow, SetWindowRgn, RDW_ERASE, RDW_FRAME,
        RDW_INVALIDATE,
    };

    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetWindowRect(hwnd, &mut rect);
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        let size = if w > 0 && h > 0 {
            w.min(h)
        } else {
            tab_physical_size(hwnd)
        };

        let region = CreateEllipticRgn(0, 0, size, size);
        if region.is_null() {
            return;
        }
        if SetWindowRgn(hwnd, region, 1) == 0 {
            DeleteObject(region);
        } else {
            RedrawWindow(
                hwnd,
                std::ptr::null(),
                std::ptr::null_mut(),
                RDW_ERASE | RDW_FRAME | RDW_INVALIDATE,
            );
        }
    }
}

static SUBCLASS_INSTALLED: AtomicBool = AtomicBool::new(false);

// --- Types ---

#[derive(Clone, Copy)]
pub struct MainWindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TabState {
    Idle,
    Pressed,
    Dragging,
}

struct TabController {
    state: TabState,
    press_origin_screen: (i32, i32),
    win_origin: (i32, i32),
    app: tauri::AppHandle,
}

// --- Subclass installation ---

/// WebView2 creates its own HWND tree inside the top-level Tauri window.
/// The deepest child windows reject external SetWindowSubclass, and they
/// intercept all mouse input before it reaches the parent.
///
/// Solution: disable child windows so the system routes mouse messages
/// to the parent (host) window where our subclass is installed.
/// A disabled window still renders normally — it just doesn't receive input.
fn disable_child_input(host_hwnd: HWND) {
    let mut current = unsafe { GetWindow(host_hwnd, GW_CHILD) };
    let mut depth = 0u32;
    while !current.is_null() {
        depth += 1;
        eprintln!(
            "[tab_controller] disabling child level {depth}: hwnd={:#x}",
            current as usize
        );
        unsafe { EnableWindow(current, 0) };
        current = unsafe { GetWindow(current, GW_CHILD) };
    }
    if depth == 0 {
        eprintln!(
            "[tab_controller] WARNING: no child windows found on host={:#x}",
            host_hwnd as usize
        );
    } else {
        eprintln!(
            "[tab_controller] disabled {depth} levels of child input on host={:#x}",
            host_hwnd as usize
        );
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn install(app: &tauri::AppHandle, host_hwnd: HWND) {
    if SUBCLASS_INSTALLED.load(Ordering::SeqCst) {
        return;
    }
    // Subclass goes on the host (top-level) HWND — it owns positioning and region.
    eprintln!(
        "[tab_controller] installing subclass on host hwnd={:#x}",
        host_hwnd as usize
    );

    let controller = Box::new(TabController {
        state: TabState::Idle,
        press_origin_screen: (0, 0),
        win_origin: (0, 0),
        app: app.clone(),
    });
    let ptr = Box::into_raw(controller);
    let ok = unsafe { SetWindowSubclass(host_hwnd, Some(subclass_proc), 0, ptr as usize) };
    if ok != 0 {
        SUBCLASS_INSTALLED.store(true, Ordering::SeqCst);
        // Now disable child windows so mouse input falls through to our subclass
        disable_child_input(host_hwnd);
    } else {
        unsafe { drop(Box::from_raw(ptr)) };
        eprintln!(
            "tab_controller: SetWindowSubclass failed on host hwnd={:#x}",
            host_hwnd as usize
        );
    }
}

// --- Subclass callback ---

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
    _uid: usize,
    data: usize,
) -> LRESULT {
    let controller = &mut *(data as *mut TabController);

    // Diagnostic: log key messages to verify subclass receives input
    match msg {
        WM_LBUTTONDOWN => eprintln!("[tab_subclass] WM_LBUTTONDOWN hwnd={:#x}", hwnd as usize),
        WM_LBUTTONUP => eprintln!(
            "[tab_subclass] WM_LBUTTONUP hwnd={:#x} state={:?}",
            hwnd as usize, controller.state
        ),
        WM_TIMER if wparam == TAB_LONG_PRESS_TIMER_ID => {
            eprintln!(
                "[tab_subclass] WM_TIMER (long-press) hwnd={:#x}",
                hwnd as usize
            )
        }
        WM_CAPTURECHANGED => {
            eprintln!("[tab_subclass] WM_CAPTURECHANGED hwnd={:#x}", hwnd as usize)
        }
        WM_NCDESTROY => eprintln!("[tab_subclass] WM_NCDESTROY hwnd={:#x}", hwnd as usize),
        _ => {}
    }

    let handled = match msg {
        WM_LBUTTONDOWN => handle_lbuttondown(hwnd, controller, wparam, lparam),
        WM_TIMER => handle_timer(hwnd, controller, wparam),
        WM_MOUSEMOVE => handle_mousemove(hwnd, controller, lparam),
        WM_LBUTTONUP => handle_lbuttonup(hwnd, controller),
        WM_CAPTURECHANGED => handle_capture_changed(hwnd, controller),
        WM_CANCELMODE => handle_cancel_mode(hwnd, controller),
        WM_NCDESTROY => {
            cleanup(hwnd, controller);
            RemoveWindowSubclass(hwnd, Some(subclass_proc), 0);
            let _ = Box::from_raw(data as *mut TabController);
            SUBCLASS_INSTALLED.store(false, Ordering::SeqCst);
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        }
        _ => false,
    };

    let result = if handled {
        0
    } else {
        DefSubclassProc(hwnd, msg, wparam, lparam)
    };

    // Re-apply circle region AFTER DefSubclassProc for activation messages.
    // WebView2 / DWM may clear the region during default processing;
    // applying after ensures the region is correct for the next repaint.
    if msg == WM_ACTIVATE || msg == WM_MOUSEACTIVATE {
        reapply_circle_region(hwnd);
    }

    result
}

// --- Message handlers ---

fn handle_lbuttondown(
    hwnd: HWND,
    ctrl: &mut TabController,
    _wparam: usize,
    _lparam: isize,
) -> bool {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut pt) };
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(hwnd, &mut rect) };

    ctrl.press_origin_screen = (pt.x, pt.y);
    ctrl.win_origin = (rect.left, rect.top);
    ctrl.state = TabState::Pressed;

    unsafe {
        SetCapture(hwnd);
        SetTimer(hwnd, TAB_LONG_PRESS_TIMER_ID, LONG_PRESS_MS, None);
    }
    true
}

fn handle_timer(hwnd: HWND, ctrl: &mut TabController, wparam: usize) -> bool {
    if wparam != TAB_LONG_PRESS_TIMER_ID {
        return false;
    }
    if ctrl.state == TabState::Pressed {
        unsafe { KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID) };
        ctrl.state = TabState::Dragging;
    }
    true
}

fn handle_mousemove(hwnd: HWND, ctrl: &mut TabController, _lparam: isize) -> bool {
    match ctrl.state {
        TabState::Dragging => {
            let key_state = unsafe { GetAsyncKeyState(VK_LBUTTON as i32) };
            if key_state & (0x8000u16 as i16) == 0 {
                // Left button released but never got WM_LBUTTONUP — snap and cleanup
                unsafe { ReleaseCapture() };
                ctrl.state = TabState::Idle;
                snap_to_edge(hwnd);
                return true;
            }
            let mut pt = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut pt) };
            let dx = pt.x - ctrl.press_origin_screen.0;
            let dy = pt.y - ctrl.press_origin_screen.1;
            let new_x = ctrl.win_origin.0 + dx;
            let new_y = ctrl.win_origin.1 + dy;
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    new_x,
                    new_y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER,
                );
            }
            true
        }
        _ => false,
    }
}

fn handle_lbuttonup(hwnd: HWND, ctrl: &mut TabController) -> bool {
    match ctrl.state {
        TabState::Pressed => {
            unsafe { KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID) };
            unsafe { ReleaseCapture() };
            ctrl.state = TabState::Idle;
            // Ensure circle region is correct before tab.hide() — DWM captures the last frame
            reapply_circle_region(hwnd);
            restore_main_window(&ctrl.app);
            true
        }
        TabState::Dragging => {
            unsafe { ReleaseCapture() };
            ctrl.state = TabState::Idle;
            snap_to_edge(hwnd);
            true
        }
        TabState::Idle => false,
    }
}

fn handle_capture_changed(hwnd: HWND, ctrl: &mut TabController) -> bool {
    unsafe { KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID) };
    ctrl.state = TabState::Idle;
    true
}

fn handle_cancel_mode(hwnd: HWND, ctrl: &mut TabController) -> bool {
    unsafe {
        KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID);
        ReleaseCapture();
    }
    ctrl.state = TabState::Idle;
    true
}

fn cleanup(hwnd: HWND, ctrl: &mut TabController) {
    unsafe {
        KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID);
        ReleaseCapture();
    }
    ctrl.state = TabState::Idle;
}

// --- Helpers ---

fn snap_to_edge(hwnd: HWND) {
    let mut win_rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(hwnd, &mut win_rect) };

    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { GetMonitorInfoW(monitor, &mut mi) };

    let tab_w = win_rect.right - win_rect.left;
    let tab_h = win_rect.bottom - win_rect.top;
    let (snap_x, snap_y) = calc_snap_position(&win_rect, &mi.rcWork, (tab_w, tab_h), 0.0);
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            snap_x,
            snap_y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER,
        );
    }
    reapply_circle_region(hwnd);
}

fn hide_tab_offscreen(hwnd: HWND) {
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            TAB_OFFSCREEN_HIDE_COORD,
            TAB_OFFSCREEN_HIDE_COORD,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_HIDEWINDOW,
        );
    }
}

#[allow(dead_code)]
fn calc_current_hidden_ratio(win_rect: &RECT, work_rect: &RECT) -> f32 {
    let tw = (win_rect.right - win_rect.left) as f32;
    let th = (win_rect.bottom - win_rect.top) as f32;
    let cx = win_rect.left as f32 + tw / 2.0;
    let cy = win_rect.top as f32 + th / 2.0;

    let dist_left = (cx - work_rect.left as f32).abs();
    let dist_right = (work_rect.right as f32 - cx).abs();
    let dist_top = (cy - work_rect.top as f32).abs();
    let dist_bottom = (work_rect.bottom as f32 - cy).abs();

    if dist_left <= dist_right && dist_left <= dist_top && dist_left <= dist_bottom {
        let visible = (win_rect.right - work_rect.left).max(0) as f32;
        (tw - visible) / tw
    } else if dist_right <= dist_top && dist_right <= dist_bottom {
        let visible = (work_rect.right - win_rect.left).max(0) as f32;
        (tw - visible) / tw
    } else if dist_top <= dist_bottom {
        let visible = (win_rect.bottom - work_rect.top).max(0) as f32;
        (th - visible) / th
    } else {
        let visible = (work_rect.bottom - win_rect.top).max(0) as f32;
        (th - visible) / th
    }
}

pub(crate) fn restore_main_window(app: &tauri::AppHandle) {
    let state = app.state::<crate::AppState>();

    let main_w = app.get_webview_window("main");
    let tab_w = app.get_webview_window("minimized-tab");
    let (Some(main), Some(tab)) = (main_w, tab_w) else {
        eprintln!("restore_main_window: missing main or tab window");
        return;
    };

    // Priority 1: exact geometry saved at minimize time (physical coordinates)
    let geo = state.main_geometry.lock().unwrap().take();

    if let Some(geo) = geo {
        if let Ok(hwnd) = main.hwnd() {
            let hwnd = hwnd.0 as HWND;
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    geo.x,
                    geo.y,
                    geo.width,
                    geo.height,
                    SWP_NOZORDER,
                );
            }
        }
    } else {
        // Fallback: reconstruct from DockPreferences (logical → physical)
        let db = state.db.lock().unwrap();
        match crate::scratchpad::preferences::load_preferences(&db) {
            Ok(prefs) => {
                drop(db);
                if let Ok(hwnd) = main.hwnd() {
                    let hwnd = hwnd.0 as HWND;
                    let dpi = unsafe { GetDpiForWindow(hwnd) };
                    let scale = dpi as f64 / 96.0;
                    let px = (prefs.dock_position_x * scale) as i32;
                    let py = (prefs.dock_position_y * scale) as i32;
                    let pw = (prefs.dock_width * scale) as i32;
                    let ph = (prefs.dock_height * scale) as i32;
                    unsafe {
                        SetWindowPos(hwnd, std::ptr::null_mut(), px, py, pw, ph, SWP_NOZORDER);
                    }
                }
            }
            Err(e) => {
                eprintln!("restore_main_window: failed to load preferences: {e}");
                return;
            }
        }
    }

    if let Ok(tab_hwnd) = tab.hwnd() {
        hide_tab_offscreen(tab_hwnd.0 as HWND);
    }
    let _ = tab.hide();
    let _ = main.show();
    let _ = main.set_focus();
    if let Ok(main_hwnd) = main.hwnd() {
        unsafe {
            SetForegroundWindow(main_hwnd.0 as HWND);
        }
    }
}

// --- Snap calculation (pure function) ---

pub fn calc_snap_position(
    window_rect: &RECT,
    work_rect: &RECT,
    tab_size: (i32, i32),
    hidden_ratio: f32,
) -> (i32, i32) {
    let _ = hidden_ratio; // Full-visibility mode: tab stays entirely within work area

    let center_x = window_rect.left + (window_rect.right - window_rect.left) / 2;
    let center_y = window_rect.top + (window_rect.bottom - window_rect.top) / 2;

    let work_left = work_rect.left;
    let work_top = work_rect.top;
    let work_right = work_rect.right;
    let work_bottom = work_rect.bottom;

    let (tw, th) = tab_size;
    let margin = TAB_EDGE_MARGIN;

    let dist_left = (center_x - work_left).abs();
    let dist_right = (work_right - center_x).abs();
    let dist_top = (center_y - work_top).abs();
    let dist_bottom = (work_bottom - center_y).abs();

    let snap_x: i32;
    let snap_y: i32;

    if dist_left <= dist_right && dist_left <= dist_top && dist_left <= dist_bottom {
        // Left edge
        snap_x = work_left + margin;
        snap_y = (center_y - th / 2).clamp(work_top + margin, work_bottom - th - margin);
    } else if dist_right <= dist_top && dist_right <= dist_bottom {
        // Right edge
        snap_x = work_right - tw - margin;
        snap_y = (center_y - th / 2).clamp(work_top + margin, work_bottom - th - margin);
    } else if dist_top <= dist_bottom {
        // Top edge
        snap_x = (center_x - tw / 2).clamp(work_left + margin, work_right - tw - margin);
        snap_y = work_top + margin;
    } else {
        // Bottom edge
        snap_x = (center_x - tw / 2).clamp(work_left + margin, work_right - tw - margin);
        snap_y = work_bottom - th - margin;
    }

    (snap_x, snap_y)
}

// --- Unit tests ---

#[cfg(test)]
mod tests {
    use super::*;

    fn make_work_rect() -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        }
    }

    fn make_tab_size() -> (i32, i32) {
        (48, 48)
    }

    #[test]
    fn snap_to_right_edge_full_visibility() {
        let win = RECT {
            left: 1800,
            top: 500,
            right: 1848,
            bottom: 548,
        };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        assert_eq!(x, work.right - 48 - TAB_EDGE_MARGIN);
        assert!(y >= work.top + TAB_EDGE_MARGIN && y + 48 <= work.bottom - TAB_EDGE_MARGIN);
    }

    #[test]
    fn snap_to_left_edge_full_visibility() {
        let win = RECT {
            left: 10,
            top: 500,
            right: 58,
            bottom: 548,
        };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        assert_eq!(x, work.left + TAB_EDGE_MARGIN);
        assert!(y >= work.top + TAB_EDGE_MARGIN && y + 48 <= work.bottom - TAB_EDGE_MARGIN);
    }

    #[test]
    fn snap_to_top_edge_full_visibility() {
        let win = RECT {
            left: 900,
            top: 20,
            right: 948,
            bottom: 68,
        };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        assert_eq!(y, work.top + TAB_EDGE_MARGIN);
        assert!(x >= work.left + TAB_EDGE_MARGIN && x + 48 <= work.right - TAB_EDGE_MARGIN);
    }

    #[test]
    fn snap_to_bottom_edge_full_visibility() {
        let win = RECT {
            left: 900,
            top: 1020,
            right: 948,
            bottom: 1068,
        };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        assert_eq!(y, work.bottom - 48 - TAB_EDGE_MARGIN);
        assert!(x >= work.left + TAB_EDGE_MARGIN && x + 48 <= work.right - TAB_EDGE_MARGIN);
    }

    #[test]
    fn hidden_ratio_ignored_in_full_visibility_mode() {
        let win = RECT {
            left: 1800,
            top: 500,
            right: 1848,
            bottom: 548,
        };
        let work = make_work_rect();
        let (x1, y1) = calc_snap_position(&win, &work, make_tab_size(), 0.0);
        let (x2, y2) = calc_snap_position(&win, &work, make_tab_size(), 0.5);
        // hidden_ratio is ignored — both should produce identical results
        assert_eq!((x1, y1), (x2, y2));
    }

    #[test]
    fn multi_monitor_offset_work_rect() {
        let win = RECT {
            left: 3700,
            top: 500,
            right: 3748,
            bottom: 548,
        };
        let work = RECT {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1080,
        };
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        assert_eq!(x, work.right - 48 - TAB_EDGE_MARGIN);
        assert!(y >= work.top + TAB_EDGE_MARGIN && y + 48 <= work.bottom - TAB_EDGE_MARGIN);
    }

    #[test]
    fn center_y_clamped_to_work_area() {
        let win = RECT {
            left: 1800,
            top: -524,
            right: 1848,
            bottom: -476,
        };
        let work = make_work_rect();
        let (_, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        assert!(y >= work.top + TAB_EDGE_MARGIN);
        assert!(y + 48 <= work.bottom - TAB_EDGE_MARGIN);
    }

    #[test]
    fn window_fully_within_work_area() {
        let work = make_work_rect();
        let positions = [
            RECT {
                left: 10,
                top: 500,
                right: 58,
                bottom: 548,
            },
            RECT {
                left: 1800,
                top: 500,
                right: 1848,
                bottom: 548,
            },
            RECT {
                left: 900,
                top: 20,
                right: 948,
                bottom: 68,
            },
            RECT {
                left: 900,
                top: 1020,
                right: 948,
                bottom: 1068,
            },
        ];
        for win in &positions {
            let (x, y) = calc_snap_position(win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
            assert!(x >= work.left, "x {} >= work.left {}", x, work.left);
            assert!(y >= work.top, "y {} >= work.top {}", y, work.top);
            assert!(
                x + 48 <= work.right,
                "x+48 {} <= work.right {}",
                x + 48,
                work.right
            );
            assert!(
                y + 48 <= work.bottom,
                "y+48 {} <= work.bottom {}",
                y + 48,
                work.bottom
            );
        }
    }
}
