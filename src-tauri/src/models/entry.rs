use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Text,
    Image,
    File,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Text => "text",
            EntryKind::Image => "image",
            EntryKind::File => "file",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryView {
    Home,
    Note,
}

impl EntryView {
    pub fn membership_table(self) -> &'static str {
        match self {
            EntryView::Home => "home_entries",
            EntryView::Note => "note_entries",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockEntry {
    pub id: String,
    pub kind: EntryKind,
    pub content: Option<String>,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub size_bytes: Option<i64>,
    pub collapsed: bool,
    pub title: Option<String>,
    pub in_home: bool,
    pub in_note: bool,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}
