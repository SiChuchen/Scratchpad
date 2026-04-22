use serde::{Deserialize, Serialize};

use crate::system::fonts::system_font_defaults;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DockPreferences {
    pub entry_surface_opacity: f64,
    pub dock_bg_opacity: f64,
    pub dock_bg_color: String,
    pub dock_minimized: bool,
    pub dock_position_x: f64,
    pub dock_position_y: f64,
    pub dock_width: f64,
    pub dock_height: f64,
    pub dock_edge_anchor: String,
    pub text_size_px: f64,
    pub text_color: String,
    pub font_family_zh: String,
    pub font_family_en: String,
    pub launch_on_startup: bool,
}

impl Default for DockPreferences {
    fn default() -> Self {
        let (sys_font, _sys_size) = system_font_defaults();
        Self {
            entry_surface_opacity: 0.82,
            dock_bg_opacity: 0.85,
            dock_bg_color: "#2a3548".to_string(),
            dock_minimized: false,
            dock_position_x: 40.0,
            dock_position_y: 40.0,
            dock_width: 360.0,
            dock_height: 640.0,
            dock_edge_anchor: "right".to_string(),
            text_size_px: 15.0,
            text_color: "#e8edf5".to_string(),
            font_family_zh: sys_font.clone(),
            font_family_en: sys_font,
            launch_on_startup: false,
        }
    }
}
