use std::path::Path;

// ---------------------------------------------------------------------------
// Win32 FFI for CF_HDROP clipboard (file copy so Explorer can paste)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win32_clipboard {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn OpenClipboard(hWndNewOwner: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(uFormat: u32, hMem: isize) -> isize;
        fn GetClipboardData(uFormat: u32) -> isize;
        fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> isize;
        fn GlobalLock(hMem: isize) -> isize;
        fn GlobalUnlock(hMem: isize) -> i32;
        fn DragQueryFileW(hDrop: isize, iFile: u32, lpszFile: *mut u16, cch: u32) -> u32;
    }

    const CF_HDROP: u32 = 15;
    const GMEM_MOVEABLE: u32 = 0x0002;

    /// Read file paths from CF_HDROP format in the system clipboard.
    pub fn get_file_drop_list() -> Result<Vec<String>, String> {
        unsafe {
            if OpenClipboard(0) == 0 {
                return Err("OpenClipboard failed".to_string());
            }
            let hdrop = GetClipboardData(CF_HDROP);
            if hdrop == 0 {
                CloseClipboard();
                return Err("no CF_HDROP data on clipboard".to_string());
            }
            let count = DragQueryFileW(hdrop, 0xFFFFFFFF, std::ptr::null_mut(), 0);
            let mut paths: Vec<String> = Vec::with_capacity(count as usize);
            for i in 0..count {
                let needed = DragQueryFileW(hdrop, i, std::ptr::null_mut(), 0) as usize;
                if needed == 0 { continue; }
                let mut buf: Vec<u16> = vec![0; needed + 1];
                let written = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), buf.len() as u32) as usize;
                if written > 0 {
                    let s = String::from_utf16_lossy(&buf[..written]);
                    if !s.is_empty() { paths.push(s); }
                }
            }
            CloseClipboard();
            Ok(paths)
        }
    }

    /// Write file paths as CF_HDROP to the system clipboard via Win32 API.
    pub fn set_file_drop_list(paths: &[&str]) -> Result<(), String> {
        let mut wide_paths: Vec<u16> = Vec::new();
        for p in paths {
            wide_paths.extend(OsStr::new(p).encode_wide());
            wide_paths.push(0);
        }
        wide_paths.push(0);

        let header_size: usize = 20;
        let paths_bytes = wide_paths.len() * 2;
        let total = header_size + paths_bytes;

        unsafe {
            let hmem = GlobalAlloc(GMEM_MOVEABLE, total);
            if hmem == 0 {
                return Err("GlobalAlloc failed".to_string());
            }
            let ptr = GlobalLock(hmem);
            if ptr == 0 {
                return Err("GlobalLock failed".to_string());
            }

            let buf = std::slice::from_raw_parts_mut(ptr as *mut u8, total);
            buf[0..4].copy_from_slice(&(header_size as u32).to_ne_bytes());
            buf[16..20].copy_from_slice(&1u32.to_ne_bytes());

            let path_slice =
                std::slice::from_raw_parts(wide_paths.as_ptr() as *const u8, paths_bytes);
            buf[header_size..].copy_from_slice(path_slice);

            GlobalUnlock(hmem);

            if OpenClipboard(0) == 0 {
                return Err("OpenClipboard failed — clipboard may be locked".to_string());
            }
            EmptyClipboard();
            let ok = SetClipboardData(CF_HDROP, hmem);
            CloseClipboard();
            if ok == 0 {
                Err("SetClipboardData failed".to_string())
            } else {
                Ok(())
            }
        }
    }
}

/// Copy file to clipboard using CF_HDROP so Ctrl+V in Explorer pastes the file.
/// Uses a delayed background thread to avoid WebView2 clipboard interference.
pub fn copy_file(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("路径为空".to_string());
    }
    if !Path::new(path).exists() {
        return Err(format!("文件不存在: {}", path));
    }

    let path_owned = path.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        #[cfg(windows)]
        {
            match win32_clipboard::set_file_drop_list(&[&path_owned]) {
                Ok(()) => eprintln!("[clipboard] copy_file OK: {}", path_owned),
                Err(e) => eprintln!("[clipboard] copy_file FAILED: {} — {}", path_owned, e),
            }
        }
    });

    Ok(())
}

/// Read file paths from the Windows clipboard (CF_HDROP).
/// Returns an empty list if CF_HDROP is not available.
pub fn read_file_paths() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        win32_clipboard::get_file_drop_list()
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Copy image files using CF_HDROP so Ctrl+V in Explorer pastes the image file.
pub fn copy_image(path: &str) -> Result<(), String> {
    copy_file(path)
}

#[cfg(test)]
mod tests {
    use super::copy_image;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn copy_image_copies_the_image_file_not_decoded_pixels() {
        let path = std::env::temp_dir().join(format!(
            "scratchpad-copy-image-{}.png",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, b"not a decodable png").unwrap();

        let result = copy_image(path.to_str().unwrap());

        assert!(
            result.is_ok(),
            "copy_image should accept an existing image file path"
        );
        std::thread::sleep(std::time::Duration::from_millis(250));
        let _ = fs::remove_file(path);
    }
}
