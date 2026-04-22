use rusqlite::{params, Connection};

use crate::models::preferences::DockPreferences;
use crate::storage::error::StorageResult;

pub fn save_preferences(conn: &mut Connection, prefs: &DockPreferences) -> StorageResult<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM preferences", [])?;
    for (key, value) in [
        ("entry_surface_opacity", prefs.entry_surface_opacity.to_string()),
        ("dock_bg_opacity", prefs.dock_bg_opacity.to_string()),
        ("dock_bg_color", prefs.dock_bg_color.clone()),
        ("dock_minimized", prefs.dock_minimized.to_string()),
        ("dock_position_x", prefs.dock_position_x.to_string()),
        ("dock_position_y", prefs.dock_position_y.to_string()),
        ("dock_width", prefs.dock_width.to_string()),
        ("dock_height", prefs.dock_height.to_string()),
        ("dock_edge_anchor", prefs.dock_edge_anchor.clone()),
        ("text_size_px", prefs.text_size_px.to_string()),
        ("text_color", prefs.text_color.clone()),
        ("font_family_zh", prefs.font_family_zh.clone()),
        ("font_family_en", prefs.font_family_en.clone()),
        ("launch_on_startup", prefs.launch_on_startup.to_string()),
    ] {
        tx.execute(
            "INSERT INTO preferences(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn load_preferences(conn: &Connection) -> StorageResult<DockPreferences> {
    let mut stmt = conn.prepare("SELECT key, value FROM preferences")?;
    let raw: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let map: std::collections::HashMap<String, String> = raw.into_iter().collect();

    let mut prefs = DockPreferences::default();
    if let Some(v) = map.get("entry_surface_opacity") {
        prefs.entry_surface_opacity = v.parse().unwrap_or(prefs.entry_surface_opacity);
    }
    if let Some(v) = map.get("dock_bg_opacity") {
        prefs.dock_bg_opacity = v.parse().unwrap_or(prefs.dock_bg_opacity);
    }
    if let Some(v) = map.get("dock_bg_color") {
        prefs.dock_bg_color = v.clone();
    }
    if let Some(v) = map.get("dock_minimized") {
        prefs.dock_minimized = v.parse().unwrap_or(false);
    }
    if let Some(v) = map.get("dock_position_x") {
        prefs.dock_position_x = v.parse().unwrap_or(prefs.dock_position_x);
    }
    if let Some(v) = map.get("dock_position_y") {
        prefs.dock_position_y = v.parse().unwrap_or(prefs.dock_position_y);
    }
    if let Some(v) = map.get("dock_width") {
        prefs.dock_width = v.parse().unwrap_or(prefs.dock_width);
    }
    if let Some(v) = map.get("dock_height") {
        prefs.dock_height = v.parse().unwrap_or(prefs.dock_height);
    }
    if let Some(v) = map.get("dock_edge_anchor") {
        prefs.dock_edge_anchor = v.clone();
    }
    if let Some(v) = map.get("text_size_px") {
        prefs.text_size_px = v.parse().unwrap_or(prefs.text_size_px);
    }
    if let Some(v) = map.get("text_color") {
        prefs.text_color = v.clone();
    }
    if let Some(v) = map.get("font_family_zh") {
        prefs.font_family_zh = v.clone();
    }
    if let Some(v) = map.get("font_family_en") {
        prefs.font_family_en = v.clone();
    }
    if let Some(v) = map.get("launch_on_startup") {
        prefs.launch_on_startup = v.parse().unwrap_or(false);
    }

    Ok(prefs)
}

pub fn resolve_font(preferred: Option<&str>, installed: &[String], fallback: &str) -> String {
    if let Some(name) = preferred {
        if installed.iter().any(|f| f == name) {
            return name.to_string();
        }
    }
    fallback.to_string()
}

pub fn load_preferences_with_fonts(
    conn: &Connection,
    installed_fonts: &[String],
) -> StorageResult<DockPreferences> {
    let prefs = load_preferences(conn)?;
    Ok(DockPreferences {
        font_family_zh: resolve_font(
            Some(&prefs.font_family_zh),
            installed_fonts,
            "Microsoft YaHei",
        ),
        font_family_en: resolve_font(
            Some(&prefs.font_family_en),
            installed_fonts,
            "Segoe UI",
        ),
        ..prefs
    })
}

#[cfg(test)]
mod preference_tests {
    use rusqlite::Connection;

    use crate::models::preferences::DockPreferences;
    use crate::scratchpad::preferences::{load_preferences, load_preferences_with_fonts, resolve_font, save_preferences};
    use crate::scratchpad::storage::ensure_dock_schema;

    #[test]
    fn preferences_roundtrip_persists_opacity_and_geometry() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let mut prefs = DockPreferences::default();
        prefs.entry_surface_opacity = 0.66;
        prefs.dock_width = 420.0;
        prefs.dock_height = 700.0;

        save_preferences(&mut conn, &prefs).unwrap();
        let loaded = load_preferences(&conn).unwrap();

        assert_eq!(loaded.entry_surface_opacity, 0.66);
        assert_eq!(loaded.dock_width, 420.0);
        assert_eq!(loaded.dock_height, 700.0);
    }

    #[test]
    fn resolve_font_falls_back_when_saved_font_is_missing() {
        let installed = vec!["Segoe UI".to_string(), "Microsoft YaHei".to_string()];
        let chosen = resolve_font(Some("Missing Font"), &installed, "Segoe UI");
        assert_eq!(chosen, "Segoe UI");
    }

    #[test]
    fn load_preferences_uses_installed_font_fallbacks() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let mut prefs = DockPreferences::default();
        prefs.font_family_zh = "Removed Font".to_string();
        save_preferences(&mut conn, &prefs).unwrap();

        let loaded = load_preferences_with_fonts(
            &conn,
            &["Microsoft YaHei".to_string(), "Segoe UI".to_string()],
        )
        .unwrap();

        assert_eq!(loaded.font_family_zh, "Microsoft YaHei");
    }
}
