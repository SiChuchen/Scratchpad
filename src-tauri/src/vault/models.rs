// src-tauri/src/vault/models.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Credential,
    Bookmark,
    Note,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Credential => "credential",
            EntryKind::Bookmark => "bookmark",
            EntryKind::Note => "note",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "credential" => Some(Self::Credential),
            "bookmark" => Some(Self::Bookmark),
            "note" => Some(Self::Note),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultField {
    pub id: String,
    pub entry_id: String,
    pub key: String,
    pub value: String,
    pub is_sensitive: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntry {
    pub id: String,
    pub kind: EntryKind,
    pub title: String,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntryDetail {
    pub entry: VaultEntry,
    pub fields: Vec<VaultField>,
    pub tags: Vec<String>,
}

/// IPC 输入：前端创建/更新条目时传入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInput {
    pub key: String,
    pub value: String,
    pub is_sensitive: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntryInput {
    pub kind: EntryKind,
    pub title: String,
    pub fields: Vec<FieldInput>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSearchHit {
    pub entry: VaultEntry,
    pub score: f64,
    pub source: String, // "fts5" | "llm"
}

/// 默认敏感字段名（创建时若 key 命中且未显式指定 is_sensitive，自动标记）
pub const DEFAULT_SENSITIVE_KEYS: &[&str] = &[
    "password", "passwd", "secret", "token", "private_key", "privatekey", "api_key", "apikey",
];

pub fn is_default_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    DEFAULT_SENSITIVE_KEYS.iter().any(|k| lower == *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_kind_round_trip() {
        for k in [EntryKind::Credential, EntryKind::Bookmark, EntryKind::Note] {
            assert_eq!(EntryKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(EntryKind::parse("bogus"), None);
    }

    #[test]
    fn default_sensitive_key_matches_common_names() {
        assert!(is_default_sensitive_key("password"));
        assert!(is_default_sensitive_key("API_KEY"));
        assert!(is_default_sensitive_key("privateKey"));
        assert!(!is_default_sensitive_key("username"));
        assert!(!is_default_sensitive_key("url"));
    }
}
