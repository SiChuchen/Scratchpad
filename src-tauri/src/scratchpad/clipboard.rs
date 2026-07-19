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
        fn GlobalSize(hMem: isize) -> usize;
        fn DragQueryFileW(hDrop: isize, iFile: u32, lpszFile: *mut u16, cch: u32) -> u32;
        fn GetClipboardSequenceNumber() -> u32;
    }

    const CF_HDROP: u32 = 15;
    const CF_UNICODETEXT: u32 = 13;
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
                if needed == 0 {
                    continue;
                }
                let mut buf: Vec<u16> = vec![0; needed + 1];
                let written = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), buf.len() as u32) as usize;
                if written > 0 {
                    let s = String::from_utf16_lossy(&buf[..written]);
                    if !s.is_empty() {
                        paths.push(s);
                    }
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

    /// Write UTF-16 text to the clipboard as CF_UNICODETEXT.
    /// Returns the clipboard sequence number immediately after the write
    /// (callers use this to detect whether the user copied something else
    /// before the auto-clear timer fires).
    pub fn set_unicode_text(text: &str) -> Result<u32, String> {
        // Encode as UTF-16 + NUL terminator.
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        let byte_count = wide.len() * 2;

        unsafe {
            let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_count);
            if hmem == 0 {
                return Err("GlobalAlloc failed".to_string());
            }
            let ptr = GlobalLock(hmem);
            if ptr == 0 {
                return Err("GlobalLock failed".to_string());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, byte_count);
            GlobalUnlock(hmem);

            if OpenClipboard(0) == 0 {
                return Err("OpenClipboard failed — clipboard may be locked".to_string());
            }
            EmptyClipboard();
            let ok = SetClipboardData(CF_UNICODETEXT, hmem);
            // Per Win32 docs: on success, ownership of `hmem` is transferred
            // to the system. We must NOT free it in that case.
            // On failure the caller still owns the handle; we leak it rather
            // than call GlobalFree (not declared here) since the only known
            // failure path is an already-locked clipboard.
            let seq = GetClipboardSequenceNumber();
            CloseClipboard();
            if ok == 0 {
                return Err("SetClipboardData failed".to_string());
            }
            Ok(seq)
        }
    }

    /// Read the current CF_UNICODETEXT contents, if any. Returns None when
    /// the clipboard does not currently hold text (different format or empty).
    pub fn get_unicode_text() -> Result<Option<String>, String> {
        unsafe {
            if OpenClipboard(0) == 0 {
                return Err("OpenClipboard failed".to_string());
            }
            let handle = GetClipboardData(CF_UNICODETEXT);
            if handle == 0 {
                CloseClipboard();
                return Ok(None);
            }
            let size = GlobalSize(handle);
            if size == 0 {
                CloseClipboard();
                return Ok(None);
            }
            let ptr = GlobalLock(handle);
            if ptr == 0 {
                CloseClipboard();
                return Err("GlobalLock failed".to_string());
            }
            // size is in bytes; u16 count = size / 2 (may include trailing NUL).
            let u16_count = size / 2;
            let slice = std::slice::from_raw_parts(ptr as *const u16, u16_count);
            // Strip trailing NULs (String::from_utf16_lossy tolerates embedded NULs
            // but we want the user-visible string).
            let mut end = u16_count;
            while end > 0 && slice[end - 1] == 0 {
                end -= 1;
            }
            let text = if end == 0 {
                String::new()
            } else {
                String::from_utf16_lossy(&slice[..end])
            };
            GlobalUnlock(handle);
            CloseClipboard();
            Ok(Some(text))
        }
    }

    /// 当前剪贴板序列号。每次剪贴板内容变化时该值递增。
    pub fn current_sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    /// 调用 Win32 EmptyClipboard() 清空剪贴板。用于敏感值超时清除。
    pub fn empty_clipboard() -> Result<(), String> {
        unsafe {
            if OpenClipboard(0) == 0 {
                return Err("OpenClipboard failed".to_string());
            }
            let ok = EmptyClipboard();
            CloseClipboard();
            if ok == 0 {
                Err("EmptyClipboard failed".to_string())
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

/// 复制文本到系统剪贴板（CF_UNICODETEXT）。
///
/// `clear_after_seconds`：
///   * `Some(n)` — 复制成功 n 秒后，如果剪贴板内容仍然是本次写入的文本
///     且序列号未变，则清空剪贴板。这是敏感值（密码等）的自动清除路径。
///   * `None` — 普通文本复制，不做自动清除。
///
/// 非 Windows 平台返回 `Err("clipboard not supported on this platform")`，
/// 不伪装成功。
pub fn copy_text(text: &str, clear_after_seconds: Option<u64>) -> Result<(), String> {
    #[cfg(windows)]
    {
        let saved_seq = win32_clipboard::set_unicode_text(text)?;
        if let Some(secs) = clear_after_seconds {
            let expected = text.to_string();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(secs));
                let current_seq = win32_clipboard::current_sequence();
                let current_text = win32_clipboard::get_unicode_text().ok().flatten();
                if should_clear_sensitive_clipboard(
                    saved_seq,
                    current_seq,
                    &expected,
                    current_text.as_deref(),
                ) {
                    if let Err(e) = win32_clipboard::empty_clipboard() {
                        eprintln!("[clipboard] auto-clear EmptyClipboard failed: {}", e);
                    }
                }
            });
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        let _ = clear_after_seconds;
        Err("clipboard not supported on this platform".to_string())
    }
}

/// 纯函数：决定敏感剪贴板是否应当被清除。
///
/// 只有当当前剪贴板序列号 == 复制时记录的序列号，且当前文本内容仍为
/// 预期值时才返回 true。任何一项不满足都说明用户已经复制了新内容，
/// 不能清空剪贴板（避免误删用户的最新复制）。
pub fn should_clear_sensitive_clipboard(
    copied_sequence: u32,
    current_sequence: u32,
    expected: &str,
    current: Option<&str>,
) -> bool {
    copied_sequence == current_sequence && current == Some(expected)
}

#[cfg(test)]
mod tests {
    use super::{copy_image, should_clear_sensitive_clipboard};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn clear_decision_returns_true_when_sequence_and_value_match() {
        // 复制后 30s 内用户没有再复制任何东西；剪贴板仍是我们写入的值。
        assert!(should_clear_sensitive_clipboard(
            42,
            42,
            "super-secret",
            Some("super-secret"),
        ));
    }

    #[test]
    fn clear_decision_returns_false_when_user_copied_new_content_after() {
        // 序列号变化 → 用户复制了新内容 → 不能清空。
        assert!(!should_clear_sensitive_clipboard(
            42,
            43,
            "super-secret",
            Some("other"),
        ));
    }

    #[test]
    fn clear_decision_returns_false_when_sequence_same_but_value_differs() {
        // 防御性：序列号一致但内容不同（理论上不应发生）— 仍然不清空。
        assert!(!should_clear_sensitive_clipboard(
            42,
            42,
            "super-secret",
            Some("different"),
        ));
    }

    #[test]
    fn clear_decision_returns_false_when_clipboard_is_not_text() {
        // 剪贴板已被清空或被替换为非文本格式（图片/文件等）→ 不清空。
        assert!(!should_clear_sensitive_clipboard(
            42,
            42,
            "super-secret",
            None
        ));
    }

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
