use tauri::Manager;

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

        // Use the window's DPI scale factor to convert 48 logical px → physical px
        let scale = window.scale_factor().map_err(|e| e.to_string())?;
        let physical = (48.0 * scale).round() as i32;

        unsafe {
            let region = windows_sys::Win32::Graphics::Gdi::CreateEllipticRgn(
                0,
                0,
                physical,
                physical,
            );
            if region.is_null() {
                return Err("CreateEllipticRgn failed".into());
            }
            windows_sys::Win32::Graphics::Gdi::SetWindowRgn(hwnd, region, 1);
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
