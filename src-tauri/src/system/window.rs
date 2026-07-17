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

const QUICK_ACCESS_WIDTH_LOGICAL: f64 = 680.0;
const QUICK_ACCESS_HEIGHT_LOGICAL: f64 = 480.0;
const QUICK_ACCESS_MIN_WIDTH_LOGICAL: f64 = 480.0;
const QUICK_ACCESS_MIN_HEIGHT_LOGICAL: f64 = 340.0;

fn logical_pixels(value: f64, scale_factor: f64) -> i32 {
    (value * scale_factor).round() as i32
}

/// Compute the centered, clamped Quick Access geometry in physical pixels.
/// Target dimensions are logical pixels converted through `scale_factor`.
pub fn fit_and_center_quick_access(
    work_area: WorkRect,
    scale_factor: f64,
) -> (i32, i32, i32, i32) {
    let work_width = work_area.right - work_area.left;
    let work_height = work_area.bottom - work_area.top;
    let width = logical_pixels(QUICK_ACCESS_WIDTH_LOGICAL, scale_factor)
        .min(work_width * 9 / 10);
    let height = logical_pixels(QUICK_ACCESS_HEIGHT_LOGICAL, scale_factor)
        .min(work_height * 9 / 10);
    let x = work_area.left + (work_width - width) / 2;
    let y = work_area.top + (work_height - height) / 2;
    (x, y, width, height)
}

/// Compute the scale-aware runtime minimum in physical pixels, clamped to 90%
/// of the current monitor work area.
pub fn runtime_min_size(work_area: &WorkRect, scale_factor: f64) -> (i32, i32) {
    let work_width = work_area.right - work_area.left;
    let work_height = work_area.bottom - work_area.top;
    let min_width = logical_pixels(QUICK_ACCESS_MIN_WIDTH_LOGICAL, scale_factor)
        .min(work_width * 9 / 10);
    let min_height = logical_pixels(QUICK_ACCESS_MIN_HEIGHT_LOGICAL, scale_factor)
        .min(work_height * 9 / 10);
    (min_width, min_height)
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

    #[test]
    fn fit_and_center_quick_access_uses_logical_target_at_100_percent() {
        let work = WorkRect::new(0, 0, 1920, 1080);
        let (x, y, w, h) = fit_and_center_quick_access(work, 1.0);
        assert_eq!((w, h), (680, 480));
        assert_eq!((x, y), ((1920 - 680) / 2, (1080 - 480) / 2));
    }

    #[test]
    fn fit_and_center_quick_access_scales_logical_target() {
        let work = WorkRect::new(0, 0, 2560, 1440);
        assert_eq!(fit_and_center_quick_access(work, 1.25).2, 850);
        assert_eq!(fit_and_center_quick_access(work, 1.25).3, 600);
        assert_eq!(fit_and_center_quick_access(work, 1.5).2, 1020);
        assert_eq!(fit_and_center_quick_access(work, 1.5).3, 720);
    }

    #[test]
    fn fit_and_center_quick_access_clamps_small_work_area() {
        let work = WorkRect::new(0, 0, 800, 500);
        let (x, y, w, h) = fit_and_center_quick_access(work, 1.0);
        assert_eq!((w, h), (680, 450));
        assert_eq!((x, y), ((800 - 680) / 2, (500 - 450) / 2));
    }

    #[test]
    fn fit_and_center_quick_access_handles_negative_monitor_coordinates() {
        let work = WorkRect::new(-1920, 0, 0, 1080);
        let (x, y, w, h) = fit_and_center_quick_access(work, 1.0);
        assert_eq!((w, h), (680, 480));
        assert_eq!((x, y), (-1920 + (1920 - 680) / 2, (1080 - 480) / 2));
    }

    #[test]
    fn runtime_min_size_is_scale_aware_and_clamped() {
        let large = WorkRect::new(0, 0, 2560, 1440);
        assert_eq!(runtime_min_size(&large, 1.0), (480, 340));
        assert_eq!(runtime_min_size(&large, 1.5), (720, 510));

        let small = WorkRect::new(0, 0, 400, 300);
        assert_eq!(runtime_min_size(&small, 1.5), (360, 270));
    }
}
