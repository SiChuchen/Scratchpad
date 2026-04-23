# Native Tab Controller Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move all minimized-tab window interaction (click restore, long-press drag, edge snap) from frontend to native Win32 subclass, and shrink the frontend to display-only.

**Architecture:** A Win32 `SetWindowSubclass` on the minimized-tab HWND intercepts `WM_LBUTTONDOWN/UP/MOVE/TIMER` to implement a state machine (`Idle → Pressed → Dragging`). Click restore reads `MainWindowGeometry` from Rust `AppState` directly. Minimize logic collapses to a single IPC `ipc_dock_minimize_to_tab` that saves geometry, computes snap, and shows the tab. Frontend keeps only icon display and hover opacity.

**Tech Stack:** Rust, windows-sys 0.59 (Win32 UI/GDI/Shell), Tauri 2, Svelte 5

**Spec:** `docs/superpowers/specs/2026-04-24-native-tab-controller-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| `src-tauri/src/system/tab_controller.rs` | NEW — state machine, subclass proc, snap calc |
| `src-tauri/src/system/window.rs` | MODIFY — add `restore_main_window()`, simplify `restore_from_tab` |
| `src-tauri/src/system/mod.rs` | MODIFY — add `pub mod tab_controller` |
| `src-tauri/src/lib.rs` | MODIFY — extend `AppState`, add IPC commands |
| `src-tauri/Cargo.toml` | MODIFY — add windows-sys features |
| `src/MinimizedApp.svelte` | MODIFY — strip to display-only |
| `src/App.svelte` | MODIFY — shrink `minimize()` to one invoke |

---

### Task 1: Add windows-sys features to Cargo.toml

**Files:**
- Modify: `src-tauri/Cargo.toml:19-22`

- [ ] **Step 1: Edit Cargo.toml windows-sys features**

Change the `windows-sys` dependency features from:

```toml
windows-sys = { version = "0.59", features = [
    "Win32_Graphics_Gdi",
    "Win32_Foundation",
] }
```

To:

```toml
windows-sys = { version = "0.59", features = [
    "Win32_Foundation",
    "Win32_Graphics_Gdi",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Shell",
    "Win32_UI_Input_KeyboardAndMouse",
] }
```

- [ ] **Step 2: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1`
Expected: compiles with no errors (features only add bindings, no code changes yet)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "add windows-sys UI/Shell/Input features for native tab controller"
```

---

### Task 2: Create `tab_controller.rs` with snap calculation and unit tests

**Files:**
- Create: `src-tauri/src/system/tab_controller.rs`
- Modify: `src-tauri/src/system/mod.rs:1-2`

This task creates the module with the pure `calc_snap_position` function and its tests. The subclass proc comes in Task 3.

- [ ] **Step 1: Register the new module in `mod.rs`**

Change `src-tauri/src/system/mod.rs` from:

```rust
pub mod fonts;
pub mod window;
```

To:

```rust
pub mod fonts;
pub mod tab_controller;
pub mod window;
```

- [ ] **Step 2: Create `tab_controller.rs` with constants and pure snap function**

Create `src-tauri/src/system/tab_controller.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::RECT;

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

// --- Snap calculation (pure function, no Tauri types) ---

/// Calculate the snap position for a tab window at the nearest work-area edge.
///
/// - `window_rect`: current window position (screen coordinates)
/// - `work_rect`: monitor work area (screen coordinates, from GetMonitorInfoW rcWork)
/// - `tab_size`: physical pixel size of the tab (width, height)
/// - `hidden_ratio`: fraction of the tab to hide behind the screen edge (0.0–0.5)
///
/// Returns the top-left corner (x, y) in physical screen coordinates.
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

    // Distance from window center to each edge of the work area
    let dist_left = (center_x - work_left).abs();
    let dist_right = (work_right - center_x).abs();
    let dist_top = (center_y - work_top).abs();
    let dist_bottom = (work_bottom - center_y).abs();

    let (tw, th) = tab_size;
    let ratio = hidden_ratio.clamp(0.0, MAX_HIDDEN_RATIO);
    let margin = 2i32; // small gap so the icon isn't flush against the very pixel edge

    let snap_x: i32;
    let snap_y: i32;

    if dist_left <= dist_right && dist_left <= dist_top && dist_left <= dist_bottom {
        // Left edge
        snap_x = work_left - ((tw as f32 * ratio) as i32);
        snap_y = (center_y - th / 2).clamp(work_top + margin, work_bottom - th - margin);
    } else if dist_right <= dist_top && dist_right <= dist_bottom {
        // Right edge
        snap_x = work_right - tw + ((tw as f32 * ratio) as i32);
        snap_y = (center_y - th / 2).clamp(work_top + margin, work_bottom - th - margin);
    } else if dist_top <= dist_bottom {
        // Top edge
        snap_x = (center_x - tw / 2).clamp(work_left + margin, work_right - tw - margin);
        snap_y = work_top - ((th as f32 * ratio) as i32);
    } else {
        // Bottom edge
        snap_x = (center_x - tw / 2).clamp(work_left + margin, work_right - tw - margin);
        snap_y = work_bottom - th + ((th as f32 * ratio) as i32);
    }

    (snap_x, snap_y)
}

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
        // Window center is near the right edge
        let win = RECT { left: 1800, top: 500, right: 1848, bottom: 548 };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        // hidden_ratio = 1/3, so visible part = 2/3 of 48 = 32px from work_right
        let expected_visible = 48.0 * (1.0 - DEFAULT_HIDDEN_RATIO); // 32
        assert_eq!(x, work.right - expected_visible as i32);
        assert!(y >= work.top && y <= work.bottom - 48);
    }

    #[test]
    fn snap_to_left_edge() {
        let win = RECT { left: 10, top: 500, right: 58, bottom: 548 };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let hidden_px = (48.0 * DEFAULT_HIDDEN_RATIO) as i32; // 16
        assert_eq!(x, work.left - hidden_px);
        assert!(y >= work.top && y <= work.bottom - 48);
    }

    #[test]
    fn snap_to_top_edge() {
        let win = RECT { left: 900, top: 20, right: 948, bottom: 68 };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let hidden_px = (48.0 * DEFAULT_HIDDEN_RATIO) as i32; // 16
        assert_eq!(y, work.top - hidden_px);
        assert!(x >= work.left && x <= work.right - 48);
    }

    #[test]
    fn snap_to_bottom_edge() {
        let win = RECT { left: 900, top: 1020, right: 948, bottom: 1068 };
        let work = make_work_rect();
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let hidden_px = (48.0 * DEFAULT_HIDDEN_RATIO) as i32; // 16
        assert_eq!(y, work.bottom - 48 + hidden_px);
    }

    #[test]
    fn max_hidden_ratio_clamps_to_half() {
        let win = RECT { left: 1800, top: 500, right: 1848, bottom: 548 };
        let work = make_work_rect();
        let (x, _) = calc_snap_position(&win, &work, make_tab_size(), 0.8);
        // Should clamp to MAX_HIDDEN_RATIO = 0.5
        let expected_visible = 48.0 * (1.0 - MAX_HIDDEN_RATIO); // 24
        assert_eq!(x, work.right - expected_visible as i32);
    }

    #[test]
    fn multi_monitor_offset_work_rect() {
        // Second monitor at x=1920
        let win = RECT { left: 3700, top: 500, right: 3748, bottom: 548 };
        let work = RECT { left: 1920, top: 0, right: 3840, bottom: 1080 };
        let (x, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        let expected_visible = 48.0 * (1.0 - DEFAULT_HIDDEN_RATIO) as i32;
        assert_eq!(x, work.right - expected_visible as i32);
        assert!(y >= work.top && y <= work.bottom - 48);
    }

    #[test]
    fn center_y_clamped_to_work_area() {
        // Window center very high — should clamp to work_top + margin
        let win = RECT { left: 1800, top: -100, right: 1848, bottom: -52 };
        let work = make_work_rect();
        let (_, y) = calc_snap_position(&win, &work, make_tab_size(), DEFAULT_HIDDEN_RATIO);
        assert!(y >= work.top + 2);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test tab_controller 2>&1`
Expected: 7 tests pass

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/system/tab_controller.rs src-tauri/src/system/mod.rs
git commit -m "add tab_controller module with snap calculation and tests"
```

---

### Task 3: Add Win32 subclass proc to `tab_controller.rs`

**Files:**
- Modify: `src-tauri/src/system/tab_controller.rs`

This task adds the `install()` function and the `subclass_proc` callback with the full state machine.

- [ ] **Step 1: Add imports and subclass proc to `tab_controller.rs`**

Append the following code to `src-tauri/src/system/tab_controller.rs` (after the `#[cfg(test)]` block is fine — imports go at the top of the file). Add these imports at the top of the file:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use windows_sys::Win32::Foundation::{HWND, LRESULT, RECT, POINT};
use windows_sys::Win32::Graphics::Gdi::{MonitorFromWindow, GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetCapture, ReleaseCapture, GetAsyncKeyState, VK_LBUTTON};
use windows_sys::Win32::UI::Shell::{SetWindowSubclass, DefSubclassProc, RemoveWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SetTimer, KillTimer, GetCursorPos, SetWindowPos, ShowWindow,
    SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNORMAL, SW_SHOWNOACTIVATE,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_TIMER,
    WM_CAPTURECHANGED, WM_CANCELMODE, WM_NCDESTROY,
};
```

Remove the duplicate `use std::sync::atomic::{AtomicBool, Ordering};` that was already there, and remove the earlier `use windows_sys::Win32::Foundation::RECT;` — the full import list replaces it.

- [ ] **Step 2: Add `install()` and `subclass_proc` to the same file**

Add after the `TabController` struct definition, before `calc_snap_position`:

```rust
// --- Subclass installation ---

/// Install the native gesture controller on the minimized-tab window.
/// Safe to call multiple times — uses an AtomicBool to prevent double-install.
pub fn install(app: &tauri::AppHandle, hwnd: HWND) {
    if SUBCLASS_INSTALLED.load(Ordering::SeqCst) {
        return;
    }
    let controller = Box::new(TabController {
        state: TabState::Idle,
        press_origin_screen: (0, 0),
        win_origin: (0, 0),
        app: app.clone(),
    });
    let ok = unsafe {
        SetWindowSubclass(
            hwnd,
            Some(subclass_proc),
            0,
            Box::into_raw(controller) as usize,
        )
    };
    if ok != 0 {
        SUBCLASS_INSTALLED.store(true, Ordering::SeqCst);
    } else {
        // Installation failed — free the box, don't set the flag, allow retry
        unsafe { drop(Box::from_raw(Box::into_raw(controller))); }
        eprintln!("tab_controller: SetWindowSubclass failed");
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

    match msg {
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
        _ => return DefSubclassProc(hwnd, msg, wparam, lparam),
    }

    LRESULT(0)
}

// --- Message handlers ---

fn handle_lbuttondown(hwnd: HWND, ctrl: &mut TabController, _wparam: usize, _lparam: isize) {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut pt) };
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { windows_sys::Win32::Graphics::Gdi::GetWindowRect(hwnd, &mut rect) };

    ctrl.press_origin_screen = (pt.x, pt.y);
    ctrl.win_origin = (rect.left, rect.top);
    ctrl.state = TabState::Pressed;

    unsafe {
        SetCapture(hwnd);
        SetTimer(hwnd, TAB_LONG_PRESS_TIMER_ID, LONG_PRESS_MS, None);
    }
}

fn handle_timer(hwnd: HWND, ctrl: &mut TabController, wparam: usize) {
    if wparam != TAB_LONG_PRESS_TIMER_ID { return; }
    match ctrl.state {
        TabState::Pressed => {
            unsafe { KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID) };
            ctrl.state = TabState::Dragging;
        }
        _ => {}
    }
}

fn handle_mousemove(hwnd: HWND, ctrl: &mut TabController, _lparam: isize) {
    match ctrl.state {
        TabState::Dragging => {
            // Safety check: is left button still held?
            let key_state = unsafe { GetAsyncKeyState(VK_LBUTTON) };
            if key_state & 0x8000 == 0 {
                // Left button released but we never got WM_LBUTTONUP — snap and cleanup
                unsafe { ReleaseCapture() };
                ctrl.state = TabState::Idle;
                snap_to_edge(hwnd);
                return;
            }
            let mut pt = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut pt) };
            let dx = pt.x - ctrl.press_origin_screen.0;
            let dy = pt.y - ctrl.press_origin_screen.1;
            let new_x = ctrl.win_origin.0 + dx;
            let new_y = ctrl.win_origin.1 + dy;
            unsafe {
                SetWindowPos(hwnd, HWND(0), new_x, new_y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
            }
        }
        _ => {} // let other messages fall through to DefSubclassProc
    }
}

fn handle_lbuttonup(hwnd: HWND, ctrl: &mut TabController) {
    match ctrl.state {
        TabState::Pressed => {
            // Short click → restore main window
            unsafe { KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID) };
            unsafe { ReleaseCapture() };
            ctrl.state = TabState::Idle;
            restore_main_window(&ctrl.app);
        }
        TabState::Dragging => {
            // Drag finished → snap to edge
            unsafe { ReleaseCapture() };
            ctrl.state = TabState::Idle;
            snap_to_edge(hwnd);
        }
        TabState::Idle => {}
    }
}

fn handle_capture_changed(hwnd: HWND, ctrl: &mut TabController) {
    unsafe { KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID) };
    ctrl.state = TabState::Idle;
}

fn handle_cancel_mode(hwnd: HWND, ctrl: &mut TabController) {
    unsafe {
        KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID);
        ReleaseCapture();
    }
    ctrl.state = TabState::Idle;
}

fn cleanup(hwnd: HWND, ctrl: &mut TabController) {
    unsafe {
        KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID);
        ReleaseCapture();
    }
    ctrl.state = TabState::Idle;
}

// --- Helpers ---

/// Snap the tab to the nearest edge of its current monitor's work area.
fn snap_to_edge(hwnd: HWND) {
    let mut win_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { windows_sys::Win32::Graphics::Gdi::GetWindowRect(hwnd, &mut win_rect) };

    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { GetMonitorInfoW(monitor, &mut mi) };

    let tab_w = win_rect.right - win_rect.left;
    let tab_h = win_rect.bottom - win_rect.top;

    // Calculate hidden ratio based on how far the tab currently extends beyond the work area
    let hidden_ratio = calc_current_hidden_ratio(&win_rect, &mi.rcWork);

    let (snap_x, snap_y) = calc_snap_position(&win_rect, &mi.rcWork, (tab_w, tab_h), hidden_ratio);
    unsafe {
        SetWindowPos(hwnd, HWND(0), snap_x, snap_y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
    }
}

/// Determine how much of the tab is hidden based on its current position vs work area.
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
        // Left edge: how much of tab is left of work_rect.left?
        let visible = (win_rect.right - work_rect.left).max(0) as f32;
        (tw - visible) / tw
    } else if dist_right <= dist_top && dist_right <= dist_bottom {
        // Right edge: how much is right of work_rect.right?
        let visible = (work_rect.right - win_rect.left).max(0) as f32;
        (tw - visible) / tw
    } else if dist_top <= dist_bottom {
        // Top edge
        let visible = (win_rect.bottom - work_rect.top).max(0) as f32;
        (th - visible) / th
    } else {
        // Bottom edge
        let visible = (work_rect.bottom - win_rect.top).max(0) as f32;
        (th - visible) / th
    }
}

/// Restore the main window from geometry stored in AppState.
fn restore_main_window(app: &tauri::AppHandle) {
    let state = app.state::<crate::AppState>();
    let geo = state.main_geometry.lock().unwrap().clone();

    let main_w = app.get_webview_window("main");
    let tab_w = app.get_webview_window("minimized-tab");
    let (Some(main), Some(tab)) = (main_w, tab_w) else {
        eprintln!("restore_main_window: missing main or tab window");
        return;
    };

    match geo {
        Some(g) => {
            // Primary path: use saved physical coordinates directly
            if let Ok(hwnd) = main.hwnd() {
                let hwnd = hwnd.0 as HWND;
                unsafe {
                    SetWindowPos(hwnd, HWND(0), g.x, g.y, g.width, g.height, SWP_NOZORDER);
                }
            }
        }
        None => {
            // Fallback: read DockPreferences (logical coords) and convert
            let db = state.db.lock().unwrap();
            match crate::scratchpad::preferences::load_preferences(&db) {
                Ok(prefs) => {
                    if let Ok(hwnd) = main.hwnd() {
                        let hwnd = hwnd.0 as HWND;
                        let dpi = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetDpiForWindow(hwnd) };
                        let scale = dpi as f32 / 96.0;
                        let px = (prefs.dock_position_x * scale as f64) as i32;
                        let py = (prefs.dock_position_y * scale as f64) as i32;
                        let pw = (prefs.dock_width * scale as f64) as i32;
                        let ph = (prefs.dock_height * scale as f64) as i32;
                        unsafe {
                            SetWindowPos(hwnd, HWND(0), px, py, pw, ph, SWP_NOZORDER);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("restore_main_window: failed to load preferences fallback: {e}");
                    return;
                }
            }
        }
    }

    // Hide tab, show and focus main
    if let Ok(tab_hwnd) = tab.hwnd() {
        unsafe { ShowWindow(tab_hwnd.0 as HWND, SW_HIDE) };
    }
    if let Ok(main_hwnd) = main.hwnd() {
        unsafe {
            ShowWindow(main_hwnd.0 as HWND, SW_SHOWNORMAL);
            windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(main_hwnd.0 as HWND);
        }
    }
}
```

Note: the `LRESULT(0)` and `HWND(0)` wrappers — verify the exact constructor syntax against the `windows-sys` 0.59 types. Some versions use raw integers; if the compiler rejects these, use `0 as LRESULT` and `0 as HWND` respectively. The `POINT`, `RECT`, and `MONITORINFO` structs should match the windows-sys 0.59 definitions.

Also note: `GetWindowRect` is currently imported via `Win32::Graphics::Gdi`. It may also be available from `Win32::Foundation` or `Win32::UI::WindowsAndMessaging` depending on the windows-sys version. If the compiler complains about ambiguity, fully qualify the call as shown above.

- [ ] **Step 2: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1`
Expected: compiles with no errors. There may be warnings about unused code (the functions aren't called yet) — that's expected.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/system/tab_controller.rs
git commit -m "add Win32 subclass proc with full state machine for tab controller"
```

---

### Task 4: Add `MainWindowGeometry` to `AppState` and add `restore_main_window` to `window.rs`

**Files:**
- Modify: `src-tauri/src/lib.rs:1-12` (AppState)
- Modify: `src-tauri/src/system/window.rs` (simplify restore)

- [ ] **Step 1: Extend `AppState` in `lib.rs`**

Add the import and extend the struct. Change:

```rust
use std::sync::Mutex;
use rusqlite::Connection;
use tauri::Manager;

pub struct AppState {
    pub db: Mutex<Connection>,
}
```

To:

```rust
use std::sync::Mutex;
use rusqlite::Connection;
use tauri::Manager;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub main_geometry: Mutex<Option<system::tab_controller::MainWindowGeometry>>,
}
```

Update the `manage()` call in `run()` (around line 236) from:

```rust
.manage(AppState {
    db: Mutex::new(init_db()),
})
```

To:

```rust
.manage(AppState {
    db: Mutex::new(init_db()),
    main_geometry: Mutex::new(None),
})
```

- [ ] **Step 2: Simplify `restore_from_tab` in `window.rs`**

The old `restore_from_tab` took `pos_x, pos_y, width, height` parameters. It's now unused by the subclass (which calls `tab_controller::restore_main_window` directly), but we keep it as a thin IPC wrapper for backward compatibility (tray menu, etc). Simplify it to call through the shared `MainWindowGeometry`:

Replace the body of `restore_from_tab` in `window.rs` with a simple delegation. The actual restore logic now lives in `tab_controller::restore_main_window`. Change the function to:

```rust
pub fn restore_from_tab(app: &tauri::AppHandle) -> Result<(), String> {
    crate::system::tab_controller::restore_main_window_pub(app)
}
```

We need to make `restore_main_window` in `tab_controller.rs` `pub` and rename it to avoid confusion. Actually, let's just add a public wrapper. In `tab_controller.rs`, change `fn restore_main_window` to `pub fn restore_main_window_pub` (or keep the internal one and add a public alias). The simplest approach: make the existing `fn restore_main_window` `pub(crate)` and call it from `window.rs`:

In `tab_controller.rs`, change:
```rust
fn restore_main_window(app: &tauri::AppHandle) {
```
To:
```rust
pub(crate) fn restore_main_window(app: &tauri::AppHandle) {
```

Then in `window.rs`, replace the old `restore_from_tab` function with:

```rust
pub fn restore_from_tab(app: &tauri::AppHandle) -> Result<(), String> {
    crate::system::tab_controller::restore_main_window(app);
    Ok(())
}
```

Also update the IPC command in `lib.rs`. Change `ipc_dock_restore_from_tab` from:

```rust
#[tauri::command]
fn ipc_dock_restore_from_tab(
    app: tauri::AppHandle,
    posX: i32,
    posY: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    system::window::restore_from_tab(&app, posX, posY, width, height)
}
```

To:

```rust
#[tauri::command]
fn ipc_dock_restore_from_tab(
    app: tauri::AppHandle,
) -> Result<(), String> {
    system::window::restore_from_tab(&app)
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1`
Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/system/window.rs src-tauri/src/system/tab_controller.rs
git commit -m "extend AppState with MainWindowGeometry, simplify restore_from_tab"
```

---

### Task 5: Implement `ipc_dock_minimize_to_tab` IPC command

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the minimize command in `lib.rs`**

Add a new IPC command after `ipc_dock_restore_from_tab`:

```rust
#[tauri::command]
fn ipc_dock_minimize_to_tab(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    use std::os::raw::c_int;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetWindowRect, MonitorFromWindow, GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, ShowWindow, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE};

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

    // 2. Sync to DockPreferences (convert physical → logical)
    {
        let dpi = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetDpiForWindow(main_hwnd) };
        let scale = dpi as f64 / 96.0;
        let db = state.db.lock().unwrap();
        let mut prefs = scratchpad::preferences::load_preferences(&db)
            .map_err(|e| e.to_string())?;
        prefs.dock_position_x = geo.x as f64 / scale;
        prefs.dock_position_y = geo.y as f64 / scale;
        prefs.dock_width = geo.width as f64 / scale;
        prefs.dock_height = geo.height as f64 / scale;
        drop(geo); // borrow ends before mutable db access
        let mut db2 = state.db.lock().unwrap();
        scratchpad::preferences::save_preferences(&mut db2, &prefs)
            .map_err(|e| e.to_string())?;
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

    // 7. Apply circle region (idempotent, safe to re-apply)
    system::window::apply_circle_region(&app, "minimized-tab")?;

    // 8. Position tab
    unsafe {
        SetWindowPos(tab_hwnd, HWND(0), snap_x, snap_y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
        ShowWindow(tab_hwnd, SW_SHOWNOACTIVATE);
    }

    // 9. Hide main
    unsafe { ShowWindow(main_hwnd, SW_HIDE) };

    Ok(())
}
```

Also add it to the `invoke_handler` macro. Find the existing handler list and add `ipc_dock_minimize_to_tab` after `ipc_dock_restore_from_tab`:

```rust
ipc_dock_restore_from_tab,
ipc_dock_minimize_to_tab,
```

Also make `calc_snap_position` and `DEFAULT_HIDDEN_RATIO` and `install` public in `tab_controller.rs` if they aren't already. Change:

```rust
fn calc_snap_position(
```
To:
```rust
pub fn calc_snap_position(
```

Change:
```rust
const DEFAULT_HIDDEN_RATIO: f32 = 1.0 / 3.0;
```
To:
```rust
pub const DEFAULT_HIDDEN_RATIO: f32 = 1.0 / 3.0;
```

- [ ] **Step 2: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/system/tab_controller.rs
git commit -m "add ipc_dock_minimize_to_tab with geometry save, monitor snap, and subclass install"
```

---

### Task 6: Slim down `MinimizedApp.svelte` to display-only

**Files:**
- Modify: `src/MinimizedApp.svelte`

- [ ] **Step 1: Replace entire `MinimizedApp.svelte` with display-only version**

```svelte
<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'

  const win = getCurrentWindow()
  let hideTimer: ReturnType<typeof setTimeout> | null = null

  onMount(() => {
    const appEl = document.getElementById('app')!
    appEl.style.cssText = 'width:48px;height:48px;background:transparent!important;backdrop-filter:none!important;border:none!important;box-shadow:none!important;border-radius:0!important;overflow:hidden;min-width:0;margin:0;padding:0;'
    document.body.style.cssText = 'width:48px;height:48px;min-width:0;background:transparent!important;margin:0;padding:0;overflow:hidden;'
    document.documentElement.style.cssText = 'width:48px;height:48px;background:transparent!important;overflow:hidden;'
    scheduleAutoHide()
  })

  function handleMouseEnter() {
    if (hideTimer) clearTimeout(hideTimer)
    ;(win as any).setOpacity(1).catch((e: unknown) => console.error('setOpacity failed:', e))
  }

  function handleMouseLeave() {
    scheduleAutoHide()
  }

  function scheduleAutoHide() {
    if (hideTimer) clearTimeout(hideTimer)
    hideTimer = setTimeout(() => {
      ;(win as any).setOpacity(0.35).catch((e: unknown) => console.error('setOpacity failed:', e))
    }, 2500)
  }
</script>

<div
  class="minimized-tab"
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
>
  <img src="/app-icon-circle.png" alt="" class="tab-icon" draggable="false" />
</div>

<style>
  .minimized-tab {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    overflow: hidden;
    background: transparent;
    border: none;
    margin: 0;
    padding: 0;
    cursor: default;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .tab-icon {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
    pointer-events: none;
    transition: transform 0.15s ease, filter 0.15s ease;
  }

  .minimized-tab:hover .tab-icon {
    transform: scale(1.1);
    filter: brightness(1.1);
  }
</style>
```

Key changes from current version:
- Removed: `invoke` import, `LONG_PRESS_THRESHOLD_MS`, all state variables (`isPressed`, `isDragging`, `pressTimer`), `handleMouseDown`, `handleMouseUp`, `restoreMain`, `onmousedown`/`onmouseup` bindings
- Removed: `onMount` invoke of `ipc_window_apply_circle_region` (Rust handles this in `ipc_dock_minimize_to_tab`)
- Removed: a11y svelte-ignore comments (no more mouse event listeners on static element)
- Removed: `role="button"` and `tabindex="0"` (no longer interactive via DOM events)
- Kept: transparent style init, hover opacity control, icon display + CSS

- [ ] **Step 2: Verify frontend compiles**

Run: `pnpm check 2>&1`
Expected: 0 errors (warnings about a11y in other files are pre-existing)

- [ ] **Step 3: Commit**

```bash
git add src/MinimizedApp.svelte
git commit -m "strip MinimizedApp to display-only — all interaction now native"
```

---

### Task 7: Simplify `minimize()` in `App.svelte`

**Files:**
- Modify: `src/App.svelte:312-368`

- [ ] **Step 1: Replace `minimize()` function**

The current `minimize()` function at lines 312-368 in `src/App.svelte` should be replaced with:

```typescript
  async function minimize() {
    try {
      await invoke('ipc_dock_minimize_to_tab')
    } catch (e) {
      showToast(`最小化失败: ${formatError(e)}`, 'error')
    }
  }
```

Verify that `invoke` is already imported at the top of the file. The current file has:

```typescript
import { invoke } from '@tauri-apps/api/core'
```

at line 3. If it's not there, add it. But from the current code it is present.

Also check whether `anchorToNearestEdge` import at line 11 can be removed. If it's only used in `minimize()`, remove it:

Remove the import line if it exists:
```typescript
import { anchorToNearestEdge } from '$lib/state/window'
```

Check if `dockApi` import is still needed for other calls — it is (used in `createHomeText`, `updatePreferences`, etc.), so keep it.

- [ ] **Step 2: Verify frontend compiles**

Run: `pnpm check 2>&1`
Expected: 0 errors

- [ ] **Step 3: Commit**

```bash
git add src/App.svelte
git commit -m "simplify minimize() to single IPC call"
```

---

### Task 8: Build and verify

**Files:** None — verification only.

- [ ] **Step 1: Run Rust tests**

Run: `cd src-tauri && cargo test 2>&1`
Expected: all tests pass, including the 7 new tab_controller tests

- [ ] **Step 2: Run frontend type check**

Run: `pnpm check 2>&1`
Expected: 0 errors

- [ ] **Step 3: Run `pnpm tauri dev` for manual testing**

Launch the app and verify all 11 acceptance criteria:

1. Click minimized icon → main window restores
2. Long-press minimized icon → drag works
3. After drag release → no accidental restore
4. Short click → no drag initiated
5. After drag release → snaps to nearest edge
6. Default hidden 1/3 is visible
7. Pushing tab further in hides up to 1/2 max
8. On multi-monitor setup, tab positions are correct
9. Circle window outline matches hit area
10. No console errors
11. `pnpm check` passes (already verified)

- [ ] **Step 4: Commit if any follow-up fixes were needed**

---

### Task 9: Final cleanup

**Files:**
- Any files with diagnostic logs added during Task 8

- [ ] **Step 1: Remove any temporary diagnostic logs**

If any `eprintln!` or `console.log` calls were added for debugging during Task 8, remove them. Keep only the `eprintln!` calls in error paths (install failure, missing windows, preferences fallback failure) as these are part of the permanent error handling.

- [ ] **Step 2: Verify clean build**

Run: `cd src-tauri && cargo check 2>&1 && cd .. && pnpm check 2>&1`
Expected: 0 errors

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "clean up diagnostic logs after native tab controller refactor"
```

---

## Self-Review

**1. Spec coverage check:**

| Spec requirement | Task |
|-----------------|------|
| Constants (timer, ratio) | Task 2 |
| MainWindowGeometry struct | Task 2 |
| TabController + TabState | Task 3 |
| SUBCLASS_INSTALLED AtomicBool | Task 3 |
| install() with retry-on-failure | Task 3 |
| WM_LBUTTONDOWN handler | Task 3 |
| WM_TIMER handler | Task 3 |
| WM_MOUSEMOVE with async key check + snap | Task 3 |
| WM_LBUTTONUP strict match | Task 3 |
| WM_CAPTURECHANGED / WM_CANCELMODE | Task 3 |
| WM_NCDESTROY full cleanup | Task 3 |
| calc_snap_position pure function | Task 2 |
| Physical coordinate system | Task 5 |
| AppState extension | Task 4 |
| ipc_dock_minimize_to_tab (11 steps) | Task 5 |
| Geometry sync to DockPreferences | Task 5 |
| Multi-monitor via rcWork | Task 5 |
| restore_main_window with fallback | Task 3 |
| MinimizedApp display-only | Task 6 |
| App.svelte minimize() simplified | Task 7 |
| Cargo.toml features | Task 1 |
| 11 acceptance criteria | Task 8 |

**2. Placeholder scan:** No TBD, TODO, or vague steps found. All steps have complete code.

**3. Type consistency:** `MainWindowGeometry` fields are consistently `i32` across all uses. `calc_snap_position` signature matches between definition and call sites. `hwnd()` returns `Result<HWND_0, ...>` where `HWND_0` has `.0` as raw integer — cast `as HWND` is consistent. `LRESULT(0)` and `HWND(0)` constructor style may need adjustment per windows-sys 0.59 actual types — flagged in Task 3.
