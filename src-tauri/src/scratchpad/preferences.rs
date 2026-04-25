use rusqlite::{params, Connection};
use serde_json;

use crate::models::preferences::DockPreferences;
use crate::storage::error::StorageResult;

pub fn save_preferences(conn: &mut Connection, prefs: &DockPreferences) -> StorageResult<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM preferences", [])?;

    let overrides_json = serde_json::to_string(&prefs.theme_overrides)
        .unwrap_or_else(|_| "{}".to_string());

    for (key, value) in [
        ("theme_mode", prefs.theme_mode.clone()),
        ("theme_preset_id", prefs.theme_preset_id.clone()),
        ("custom_base_preset_id", prefs.custom_base_preset_id.clone()),
        ("theme_overrides", overrides_json),
        ("ui_text_size_px", prefs.ui_text_size_px.to_string()),
        ("content_text_size_px", prefs.content_text_size_px.to_string()),
        ("spacing_preset", prefs.spacing_preset.clone()),
        ("radius_preset", prefs.radius_preset.clone()),
        ("dock_position_x", prefs.dock_position_x.to_string()),
        ("dock_position_y", prefs.dock_position_y.to_string()),
        ("dock_width", prefs.dock_width.to_string()),
        ("dock_height", prefs.dock_height.to_string()),
        ("dock_edge_anchor", prefs.dock_edge_anchor.clone()),
        ("dock_minimized", prefs.dock_minimized.to_string()),
        ("font_family_zh", prefs.font_family_zh.clone()),
        ("font_family_en", prefs.font_family_en.clone()),
        ("launch_on_startup", prefs.launch_on_startup.to_string()),
        ("update_proxy", prefs.update_proxy.clone()),
        ("language", prefs.language.clone()),
        ("auto_cleanup_days", prefs.auto_cleanup_days.to_string()),
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

    if let Some(v) = map.get("theme_mode") {
        prefs.theme_mode = v.clone();
    }
    if let Some(v) = map.get("theme_preset_id") {
        prefs.theme_preset_id = v.clone();
    }
    if let Some(v) = map.get("custom_base_preset_id") {
        prefs.custom_base_preset_id = v.clone();
    }
    if let Some(v) = map.get("theme_overrides") {
        prefs.theme_overrides = serde_json::from_str(v)
            .unwrap_or_default();
    }
    if let Some(v) = map.get("ui_text_size_px") {
        prefs.ui_text_size_px = v.parse().unwrap_or(prefs.ui_text_size_px);
    }
    if let Some(v) = map.get("content_text_size_px") {
        prefs.content_text_size_px = v.parse().unwrap_or(prefs.content_text_size_px);
    }
    if let Some(v) = map.get("spacing_preset") {
        prefs.spacing_preset = v.clone();
    }
    if let Some(v) = map.get("radius_preset") {
        prefs.radius_preset = v.clone();
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
    if let Some(v) = map.get("dock_minimized") {
        prefs.dock_minimized = v.parse().unwrap_or(false);
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
    if let Some(v) = map.get("update_proxy") {
        prefs.update_proxy = v.clone();
    }
    if let Some(v) = map.get("language") {
        prefs.language = v.clone();
    }
    if let Some(v) = map.get("auto_cleanup_days") {
        prefs.auto_cleanup_days = v.parse().unwrap_or(0);
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
    fn preferences_roundtrip_persists_theme_fields() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();

        let mut prefs = DockPreferences::default();
        prefs.theme_mode = "custom".to_string();
        prefs.custom_base_preset_id = "dark-glass".to_string();
        prefs.theme_overrides.insert("--color-primary".to_string(), "#ff0000".to_string());
        prefs.ui_text_size_px = 14.0;
        prefs.spacing_preset = "compact".to_string();

        save_preferences(&mut conn, &prefs).unwrap();
        let loaded = load_preferences(&conn).unwrap();

        assert_eq!(loaded.theme_mode, "custom");
        assert_eq!(loaded.custom_base_preset_id, "dark-glass");
        assert_eq!(loaded.theme_overrides.get("--color-primary"), Some(&"#ff0000".to_string()));
        assert_eq!(loaded.ui_text_size_px, 14.0);
        assert_eq!(loaded.spacing_preset, "compact");
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
        ensure_dock_schema(&mut conn, 0).unwrap();

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
