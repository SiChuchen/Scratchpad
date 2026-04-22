use std::collections::BTreeSet;

#[cfg(target_os = "windows")]
pub fn list_installed_fonts() -> Vec<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    fn clean_name(raw: &str) -> String {
        // Strip trailing parenthetical suffixes like "(TrueType)", "(OpenType)", "(All res)", "(120)"
        let name = raw.trim();
        if let Some(pos) = name.rfind(" (") {
            if name.ends_with(')') {
                return name[..pos].to_string();
            }
        }
        name.to_string()
    }

    let mut names = BTreeSet::new();
    let key = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";

    // System-wide fonts
    if let Ok(hklm) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(key) {
        for item in hklm.enum_values().flatten() {
            names.insert(clean_name(&item.0));
        }
    }

    // Per-user fonts (manually installed for current user only)
    if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(key) {
        for item in hkcu.enum_values().flatten() {
            names.insert(clean_name(&item.0));
        }
    }

    names.into_iter().collect()
}

#[cfg(not(target_os = "windows"))]
pub fn list_installed_fonts() -> Vec<String> {
    Vec::new()
}

/// Read Windows system default UI font and size from registry.
/// Returns (font_name, size_px). Falls back to ("Segoe UI", 14) on failure.
#[cfg(target_os = "windows")]
pub fn system_font_defaults() -> (String, f64) {
    use winreg::enums::{HKEY_CURRENT_USER, REG_BINARY};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let metrics = match hkcu.open_subkey("Control Panel\\Desktop\\WindowMetrics") {
        Ok(k) => k,
        Err(_) => return ("Segoe UI".into(), 14.0),
    };

    let raw = match metrics.get_raw_value("MessageFont") {
        Ok(v) => {
            if v.vtype != REG_BINARY || v.bytes.len() < 92 {
                return ("Segoe UI".into(), 14.0);
            }
            v.bytes
        }
        Err(_) => return ("Segoe UI".into(), 14.0),
    };

    // LOGFONT: lfHeight (i32, offset 0), lfFaceName (WCHAR[32], offset 28)
    let lf_height = i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let face: String = raw[28..92]
        .chunks(2)
        .take_while(|c| c != &[0u8, 0u8])
        .filter_map(|c| {
            if c.len() == 2 {
                let val = u16::from_le_bytes([c[0], c[1]]);
                Some(char::from_u32(val as u32)?)
            } else {
                None
            }
        })
        .collect();

    // lfHeight < 0: character height in pixels; > 0: cell height in points
    let size_px = if lf_height < 0 {
        (-lf_height) as f64
    } else if lf_height > 0 {
        (lf_height as f64) * 96.0 / 72.0
    } else {
        14.0
    };

    let font_name = if face.is_empty() {
        "Segoe UI".to_string()
    } else {
        face
    };

    (font_name, size_px)
}

#[cfg(not(target_os = "windows"))]
pub fn system_font_defaults() -> (String, f64) {
    ("sans-serif".into(), 14.0)
}
