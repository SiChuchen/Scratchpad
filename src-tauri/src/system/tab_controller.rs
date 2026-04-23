use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use windows_sys::Win32::Foundation::{HWND, LRESULT, RECT, POINT};
use windows_sys::Win32::Graphics::Gdi::{MonitorFromWindow, GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetCapture, ReleaseCapture, GetAsyncKeyState, VK_LBUTTON};
use windows_sys::Win32::UI::Shell::{SetWindowSubclass, DefSubclassProc, RemoveWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SetTimer, KillTimer, GetCursorPos, SetWindowPos, ShowWindow,
    SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNORMAL, SW_SHOWNOACTIVATE,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_TIMER,
    WM_CAPTURECHANGED, WM_CANCELMODE, WM_NCDESTROY,
};

// --- Constants ---

pub const TAB_LONG_PRESS_TIMER_ID: usize = 1;
pub const LONG_PRESS_MS: u32 = 200;
pub const DEFAULT_HIDDEN_RATIO: f32 = 1.0 / 3.0;
pub const MAX_HIDDEN_RATIO: f32 = 1.0 / 2.0;

static SUBCLASS_INSTALLED: AtomicBool = AtomicBool::new(false);

// --- Types ---

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

// --- Snap calculation (pure function) ---

pub fn calc_snap_position(
    window_rect: &RECT,
    work_rect: &RECT,
    tab_size: (i32, i32),
    hidden_ratio: f32,
) -> (i32, i32) {
    let center_x = window_rect.left + (window_rect.right - window_rect.left) / 2;
    let center_y = window_rect.top + (window_rect.bottom - window_rect.top) / 2;

    let work_left = work_rect.left;
    let work_top = work_rect.top;
    let work_right = work_rect.right;
    let work_bottom = work_rect.bottom;

    let dist_left = (center_x - work_left).abs();
    let dist_right = (work_right - center_x).abs();
    let dist_top = (center_y - work_top).abs();
    let dist_bottom = (work_bottom - center_y).abs();

    let (tw, th) = tab_size;
    let ratio = hidden_ratio.clamp(0.0, MAX_HIDDEN_RATIO);
    let margin = 2i32;

    let hidden_w = (tw as f32 * ratio) as i32;
    let visible_w = (tw as f32 * (1.0 - ratio)) as i32;
    let hidden_h = (th as f32 * ratio) as i32;
    let visible_h = (th as f32 * (1.0 - ratio)) as i32;

    let snap_x: i32;
    let snap_y: i32;

    if dist_left <= dist_right && dist_left <= dist_top && dist_left <= dist_bottom {
        // Left edge
        snap_x = work_left - hidden_w;
        snap_y = (center_y - th / 2).clamp(work_top + margin, work_bottom - th - margin);
    } else if dist_right <= dist_top && dist_right <= dist_bottom {
        // Right edge
        snap_x = work_right - visible_w;
        snap_y = (center_y - th / 2).clamp(work_top + margin, work_bottom - th - margin);
    } else if dist_top <= dist_bottom {
        // Top edge
        snap_x = (center_x - tw / 2).clamp(work_left + margin, work_right - tw - margin);
        snap_y = work_top - hidden_h;
    } else {
        // Bottom edge
        snap_x = (center_x - tw / 2).clamp(work_left + margin, work_right - tw - margin);
        snap_y = work_bottom - th + hidden_h;
    }

    (snap_x, snap_y)
}

// --- Unit tests ---

#[cfg(test)]
mod tests {
    use super::*;

    fn make_work_rect() -> RECT {
        RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
    }

    fn make_tab_size() -> (i32, i32) {
        (48, 48)
    }

    #[test]
    fn snap_to_right_edge_default_ratio() {
        let win = RECT { left: 1800, top: 500, right: 1848, bottom: 548 };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let expected_visible = 48.0 * (1.0 - DEFAULT_HIDDEN_RATIO);
        assert_eq!(x, work.right - expected_visible as i32);
        assert!(y >= work.top && y <= work.bottom - 48);
    }

    #[test]
    fn snap_to_left_edge() {
        let win = RECT { left: 10, top: 500, right: 58, bottom: 548 };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let hidden_px = (48.0 * DEFAULT_HIDDEN_RATIO) as i32;
        assert_eq!(x, work.left - hidden_px);
        assert!(y >= work.top && y <= work.bottom - 48);
    }

    #[test]
    fn snap_to_top_edge() {
        let win = RECT { left: 900, top: 20, right: 948, bottom: 68 };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let hidden_px = (48.0 * DEFAULT_HIDDEN_RATIO) as i32;
        assert_eq!(y, work.top - hidden_px);
        assert!(x >= work.left && x <= work.right - 48);
    }

    #[test]
    fn snap_to_bottom_edge() {
        let win = RECT { left: 900, top: 1020, right: 948, bottom: 1068 };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let hidden_px = (48.0 * DEFAULT_HIDDEN_RATIO) as i32;
        assert_eq!(y, work.bottom - 48 + hidden_px);
    }

    #[test]
    fn max_hidden_ratio_clamps_to_half() {
        let win = RECT { left: 1800, top: 500, right: 1848, bottom: 548 };
        let work = make_work_rect();
        let (x, _) = calc_snap_position(&win, &work, make_tab_size(), 0.8);
        let expected_visible = 48.0 * (1.0 - MAX_HIDDEN_RATIO);
        assert_eq!(x, work.right - expected_visible as i32);
    }

    #[test]
    fn multi_monitor_offset_work_rect() {
        let win = RECT { left: 3700, top: 500, right: 3748, bottom: 548 };
        let work = RECT { left: 1920, top: 0, right: 3840, bottom: 1080 };
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let expected_visible = (48.0 * (1.0 - DEFAULT_HIDDEN_RATIO)) as i32;
        assert_eq!(x, work.right - expected_visible);
        assert!(y >= work.top && y <= work.bottom - 48);
    }

    #[test]
    fn center_y_clamped_to_work_area() {
        // Window center at x=1824 (near right edge), y=-500 (far above screen).
        // dist_left=1824, dist_right=96, dist_top=500, dist_bottom=1580 -> snaps to right edge.
        let win = RECT { left: 1800, top: -524, right: 1848, bottom: -476 };
        let work = make_work_rect();
        let (_, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        assert!(y >= work.top + 2);
    }
}
