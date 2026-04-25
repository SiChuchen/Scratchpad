use tauri::Manager;

pub fn disable_dwm_transitions(
    app: &tauri::AppHandle,
    label: &str,
) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("window not found: {label}"))?;

    #[cfg(target_os = "windows")]
    {
        let hwnd = window
            .hwnd()
            .map_err(|e| e.to_string())?
            .0 as windows_sys::Win32::Foundation::HWND;

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

pub fn apply_circle_region(
    app: &tauri::AppHandle,
    label: &str,
) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("window not found: {label}"))?;

    #[cfg(target_os = "windows")]
    {
        let hwnd = window
            .hwnd()
            .map_err(|e| e.to_string())?
            .0 as windows_sys::Win32::Foundation::HWND;

        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Gdi::{
            CreateEllipticRgn, DeleteObject, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
            RedrawWindow, SetWindowRgn,
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

pub fn clear_region(
    app: &tauri::AppHandle,
    label: &str,
) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("window not found: {label}"))?;

    #[cfg(target_os = "windows")]
    {
        let hwnd = window
            .hwnd()
            .map_err(|e| e.to_string())?
            .0 as windows_sys::Win32::Foundation::HWND;

        unsafe {
            windows_sys::Win32::Graphics::Gdi::SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
        }
    }

    Ok(())
}

pub fn restore_from_tab(
    app: &tauri::AppHandle,
) -> Result<(), String> {
    crate::system::tab_controller::restore_main_window(app);
    Ok(())
}
