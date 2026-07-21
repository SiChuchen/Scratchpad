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
    pub tags: Vec<VaultTag>,
    pub ai_metadata: Option<VaultAiMetadata>,
}

/// IPC 输入：前端创建/更新条目时传入
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInput {
    pub key: String,
    pub value: String,
    pub is_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntryInput {
    pub kind: EntryKind,
    pub title: String,
    pub fields: Vec<FieldInput>,
    pub notes: Option<String>,
    #[serde(default)]
    pub manual_tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagSource {
    Manual,
    Ai,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultTag {
    pub tag: String,
    pub normalized_tag: String,
    pub source: TagSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMetadataStatus {
    Ready,
    Pending,
    Error,
}

impl AiMetadataStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AiMetadataStatus::Ready => "ready",
            AiMetadataStatus::Pending => "pending",
            AiMetadataStatus::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ready" => Some(Self::Ready),
            "pending" => Some(Self::Pending),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultAiMetadata {
    pub entry_id: String,
    pub summary: Option<String>,
    pub search_aliases: Vec<String>,
    pub content_hash: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub generated_at: Option<String>,
    pub status: AiMetadataStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureField {
    pub draft_id: String,
    pub key: String,
    pub value: String,
    pub is_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProvenance {
    pub provider_id: String,
    pub model: String,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDraft {
    pub kind: EntryKind,
    pub title: String,
    pub notes: Option<String>,
    pub fields: Vec<CaptureField>,
    pub manual_tags: Vec<String>,
    pub ai_tags: Vec<String>,
    pub ai_summary: Option<String>,
    pub search_aliases: Vec<String>,
    pub ai_provenance: Option<AiProvenance>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRequestAudit {
    pub provider_id: String,
    pub model: String,
    pub sent_at: String,
    pub messages: Vec<AuditMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuggestedField {
    pub key: String,
    pub value: String,
    pub is_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSuggestion {
    pub kind: Option<EntryKind>,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub fields: Vec<SuggestedField>,
    pub ai_tags: Vec<String>,
    pub ai_summary: Option<String>,
    pub search_aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureEnrichment {
    pub suggestion: CaptureSuggestion,
    pub audit: AiRequestAudit,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiQueryPlan {
    pub kinds: Vec<EntryKind>,
    pub keywords: Vec<String>,
    pub aliases: Vec<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchSource {
    Local,
    AiExpanded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntrySummary {
    pub entry: VaultEntry,
    pub tags: Vec<VaultTag>,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSearchHit {
    pub summary: VaultEntrySummary,
    pub score: f64,
    pub sources: Vec<SearchSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSearch {
    pub plan: AiQueryPlan,
    pub understood_terms: Vec<String>,
    pub audit: AiRequestAudit,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillStatus {
    pub total: usize,
    pub pending: usize,
    pub processing: usize,
    pub ready: usize,
    pub error: usize,
}

/// 默认敏感字段名（创建时若 key 命中且未显式指定 is_sensitive，自动标记）
pub const DEFAULT_SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "private_key",
    "privatekey",
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "accesskey",
    "密码",
    "私钥",
    "密钥",
    "令牌",
    "访问密钥",
];

pub fn is_default_sensitive_key(key: &str) -> bool {
    let lower = key.trim().to_lowercase();
    DEFAULT_SENSITIVE_KEYS.iter().any(|k| lower == *k)
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("private_key")
        || lower.contains("apikey")
        || lower.contains("api_key")
        || lower.contains("密码")
        || lower.contains("私钥")
        || lower.contains("密钥")
        || lower.contains("令牌")
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
        assert!(is_default_sensitive_key(" \tpassword\u{2003}"));
        assert!(!is_default_sensitive_key("username"));
        assert!(!is_default_sensitive_key("url"));
        assert!(!is_default_sensitive_key("  username  "));
    }

    #[test]
    fn default_sensitive_key_includes_restored_variants() {
        // Task 6 的 M7 统一之后这些 key 一度从默认敏感列表里丢失，
        // 这里回归保护，确保 pwd / api-key / access_key / accesskey 始终敏感。
        assert!(is_default_sensitive_key("pwd"));
        assert!(is_default_sensitive_key("PWD"));
        assert!(is_default_sensitive_key("api-key"));
        assert!(is_default_sensitive_key("API-KEY"));
        assert!(is_default_sensitive_key("access_key"));
        assert!(is_default_sensitive_key("ACCESS_KEY"));
        assert!(is_default_sensitive_key("accesskey"));
        assert!(is_default_sensitive_key("AccessKey"));
        assert!(is_default_sensitive_key("密码"));
        assert!(is_default_sensitive_key("私钥"));
        assert!(is_default_sensitive_key("管理员密码"));
        assert!(is_default_sensitive_key("deploy_private_key"));
    }
}
