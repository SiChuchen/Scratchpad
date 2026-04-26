use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::system::fonts::system_font_defaults;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DockPreferences {
    // Theme
    pub theme_mode: String,                          // "system" | "preset" | "custom"
    pub theme_preset_id: String,                     // "dark-glass" | "light-matte" | "light-frosted"
    pub custom_base_preset_id: String,
    pub theme_overrides: HashMap<String, String>,

    // Layout
    pub ui_text_size_px: f64,                        // default 12
    pub content_text_size_px: f64,                   // default 14
    pub spacing_preset: String,                      // "compact" | "normal" | "spacious"
    pub radius_preset: String,                       // "sharp" | "normal" | "round"

    // Window geometry
    pub dock_position_x: f64,
    pub dock_position_y: f64,
    pub dock_width: f64,
    pub dock_height: f64,
    pub dock_edge_anchor: String,
    pub dock_minimized: bool,

    // Fonts
    pub font_family_zh: String,
    pub font_family_en: String,

    // System
    pub launch_on_startup: bool,
    pub update_proxy: String,

    // Language
    pub language: String,                          // "zh-CN" | "en", default "" (auto-detect)

    // Shortcut
    pub shortcut_modifiers: String,
    pub shortcut_key: String,
    pub shortcut_registered: bool,

    // Cleanup
    pub auto_cleanup_days: i64,                    // 0 = clean all unstarred on startup, N = keep N days
}

impl Default for DockPreferences {
    fn default() -> Self {
        let (sys_font, _sys_size) = system_font_defaults();
        Self {
            theme_mode: "system".to_string(),
            theme_preset_id: "dark-glass".to_string(),
            custom_base_preset_id: String::new(),
            theme_overrides: HashMap::new(),
            ui_text_size_px: 12.0,
            content_text_size_px: 14.0,
            spacing_preset: "normal".to_string(),
            radius_preset: "normal".to_string(),
            dock_position_x: 40.0,
            dock_position_y: 40.0,
            dock_width: 360.0,
            dock_height: 640.0,
            dock_edge_anchor: "right".to_string(),
            dock_minimized: false,
            font_family_zh: sys_font.clone(),
            font_family_en: sys_font,
            launch_on_startup: false,
            update_proxy: String::new(),
            language: String::new(),
            shortcut_modifiers: "Alt+Shift".to_string(),
            shortcut_key: "V".to_string(),
            shortcut_registered: false,
            auto_cleanup_days: 0,
        }
    }
}
