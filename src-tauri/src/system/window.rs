use tauri::Manager;

pub fn disable_dwm_transitions(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("window not found: {label}"))?;

    #[cfg(target_os = "windows")]
    {
        let hwnd =
            window.hwnd().map_err(|e| e.to_string())?.0 as windows_sys::Win32::Foundation::HWND;

        use windows_sys::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED,
        };

        let disabled: i32 = 1;
        let hr = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED as u32,
                &disabled as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&disabled) as u32,
            )
        };
        if hr < 0 {
            return Err(format!(
                "DwmSetWindowAttribute(DWMWA_TRANSITIONS_FORCEDISABLED) failed: 0x{:08x}",
                hr as u32
            ));
        }
    }

    Ok(())
}

pub fn apply_circle_region(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("window not found: {label}"))?;

    #[cfg(target_os = "windows")]
    {
        let hwnd =
            window.hwnd().map_err(|e| e.to_string())?.0 as windows_sys::Win32::Foundation::HWND;

        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Gdi::{
            CreateEllipticRgn, DeleteObject, RedrawWindow, SetWindowRgn, RDW_ERASE, RDW_FRAME,
            RDW_INVALIDATE,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

        unsafe {
            // Read actual window size, fall back to DPI-based calculation for hidden windows
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
                crate::system::tab_controller::tab_physical_size(hwnd)
            };

            let region = CreateEllipticRgn(0, 0, size, size);
            if region.is_null() {
                return Err("CreateEllipticRgn failed".into());
            }

            let ok = SetWindowRgn(hwnd, region, 1);
            if ok == 0 {
                // SetWindowRgn failed — we must free the region ourselves
                DeleteObject(region);
                return Err("SetWindowRgn failed".into());
            }
            // Success: system owns the region, do NOT DeleteObject

            // Force a full redraw to eliminate stale artifacts
            RedrawWindow(
                hwnd,
                std::ptr::null(),
                std::ptr::null_mut(),
                RDW_ERASE | RDW_FRAME | RDW_INVALIDATE,
            );
        }
    }

    Ok(())
}

pub fn clear_region(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("window not found: {label}"))?;

    #[cfg(target_os = "windows")]
    {
        let hwnd =
            window.hwnd().map_err(|e| e.to_string())?.0 as windows_sys::Win32::Foundation::HWND;

        unsafe {
            windows_sys::Win32::Graphics::Gdi::SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
        }
    }

    Ok(())
}

pub fn restore_from_tab(app: &tauri::AppHandle) -> Result<(), String> {
    crate::system::tab_controller::restore_main_window(app);
    Ok(())
}

/// Compute the centered position and clamped size for the quick-access window
/// given the cursor position and the work area of the monitor the cursor is on.
///
/// Pure function — caller is responsible for obtaining the Win32 work area so
/// this can be unit-tested without FFI.
///
/// Returns `(x, y, width, height)` in physical pixels.
pub fn fit_and_center_quick_access(
    _cursor_x: i32,
    _cursor_y: i32,
    work_area: WorkRect,
) -> (i32, i32, i32, i32) {
    let work_width = work_area.right - work_area.left;
    let work_height = work_area.bottom - work_area.top;
    let width = 760.min(work_width * 9 / 10);
    let height = 520.min(work_height * 9 / 10);
    let x = work_area.left + (work_width - width) / 2;
    let y = work_area.top + (work_height - height) / 2;
    (x, y, width, height)
}

/// Work-area rectangle in physical pixels. Mirrors `windows_sys::Win32::Foundation::RECT`
/// but kept as a plain struct so the pure helper is testable without depending on Win32.
#[derive(Debug, Clone, Copy)]
pub struct WorkRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WorkRect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 760×520 on a large monitor stays at 760×520 and is centered.
    #[test]
    fn fit_and_center_quick_access_large_monitor() {
        // 1920×1080 work area at origin
        let work = WorkRect::new(0, 0, 1920, 1080);
        let (x, y, w, h) = fit_and_center_quick_access(960, 540, work);
        assert_eq!((w, h), (760, 520));
        assert_eq!(x, (1920 - 760) / 2);
        assert_eq!(y, (1080 - 520) / 2);
    }

    /// Small work area clamps the window to 90% and centers it.
    #[test]
    fn fit_and_center_quick_access_small_work_area() {
        // 800×500 work area
        let work = WorkRect::new(0, 0, 800, 500);
        let (x, y, w, h) = fit_and_center_quick_access(400, 250, work);
        assert_eq!(w, 800 * 9 / 10); // 720
        assert_eq!(h, 500 * 9 / 10); // 450
        assert_eq!(x, (800 - w) / 2);
        assert_eq!(y, (500 - h) / 2);
    }

    /// Negative-coordinate secondary monitor is handled correctly.
    #[test]
    fn fit_and_center_quick_access_negative_coords_secondary() {
        // Work area on a secondary monitor placed to the left of the primary:
        // (-1920, 0) .. (0, 1080)
        let work = WorkRect::new(-1920, 0, 0, 1080);
        let (x, y, w, h) = fit_and_center_quick_access(-960, 540, work);
        assert_eq!((w, h), (760, 520));
        let work_width = 0 - (-1920); // 1920
        let work_height = 1080 - 0; // 1080
        assert_eq!(x, -1920 + (work_width - w) / 2);
        assert_eq!(y, 0 + (work_height - h) / 2);
    }
}
