use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentSource {
    Dock,
    Vault,
}

impl ContentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dock => "dock",
            Self::Vault => "vault",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dock" => Some(Self::Dock),
            "vault" => Some(Self::Vault),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Text,
    Image,
    File,
    Credential,
    Bookmark,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RetentionState {
    Temporary,
    Saved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowseScope {
    Temporary,
    All,
    Saved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentOperation {
    Created,
    Updated,
    Retention,
    Reordered,
    Deleted,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnifiedContentId(String);

impl UnifiedContentId {
    pub fn new(source: ContentSource, source_id: impl AsRef<str>) -> Result<Self, String> {
        let source_id = source_id.as_ref();
        if source_id.is_empty() {
            return Err("content source ID cannot be empty".to_string());
        }

        Ok(Self(format!("{}:{source_id}", source.as_str())))
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let (namespace, source_id) = value
            .split_once(':')
            .ok_or_else(|| "content ID must include a namespace".to_string())?;
        let source = ContentSource::parse(namespace)
            .ok_or_else(|| format!("unknown content namespace: {namespace}"))?;
        Self::new(source, source_id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for UnifiedContentId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for UnifiedContentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentCapabilities {
    pub copy_text: bool,
    pub copy_image: bool,
    pub copy_file: bool,
    pub copy_path: bool,
    pub open_url: bool,
    pub reveal_sensitive: bool,
    pub edit: bool,
    pub save: bool,
    pub unsave: bool,
    pub delete: bool,
    pub reorder: bool,
}

impl ContentCapabilities {
    pub fn for_item(kind: ContentKind, retention: RetentionState, reorderable: bool) -> Self {
        let (copy_text, copy_image, copy_file, copy_path, open_url, reveal_sensitive) = match kind {
            ContentKind::Text => (true, false, false, false, false, false),
            ContentKind::Image => (false, true, false, true, false, false),
            ContentKind::File => (false, false, true, true, false, false),
            ContentKind::Credential => (true, false, false, false, false, true),
            ContentKind::Bookmark => (true, false, false, false, true, false),
            ContentKind::Note => (true, false, false, false, false, false),
        };
        let (save, unsave) = match retention {
            RetentionState::Temporary => (true, false),
            RetentionState::Saved => (false, true),
        };

        Self {
            copy_text,
            copy_image,
            copy_file,
            copy_path,
            open_url,
            reveal_sensitive,
            edit: true,
            save,
            unsave,
            delete: true,
            reorder: reorderable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSummary {
    pub id: String,
    pub kind: ContentKind,
    pub retention: RetentionState,
    pub title: String,
    pub preview: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub cleanup_at: Option<String>,
    pub capabilities: ContentCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum ContentDetail {
    Text {
        summary: ContentSummary,
        title: String,
        body: String,
    },
    Image {
        summary: ContentSummary,
        file_name: String,
        asset_path: String,
        mime_type: Option<String>,
        width: Option<i64>,
        height: Option<i64>,
        available: bool,
    },
    File {
        summary: ContentSummary,
        file_name: String,
        asset_path: String,
        mime_type: Option<String>,
        size_bytes: Option<i64>,
        available: bool,
    },
    Credential {
        summary: ContentSummary,
        fields: Vec<UnifiedField>,
        notes: Option<String>,
        tags: Vec<UnifiedTag>,
    },
    Bookmark {
        summary: ContentSummary,
        url: String,
        fields: Vec<UnifiedField>,
        notes: Option<String>,
        tags: Vec<UnifiedTag>,
    },
    Note {
        summary: ContentSummary,
        body: String,
        fields: Vec<UnifiedField>,
        tags: Vec<UnifiedTag>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedField {
    pub key: String,
    pub value: String,
    pub is_sensitive: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentTagSource {
    Manual,
    Ai,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedTag {
    pub tag: String,
    pub normalized_tag: String,
    pub source: ContentTagSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchSource {
    Local,
    AiExpanded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSearchHit {
    pub summary: ContentSummary,
    pub score: f64,
    pub sources: Vec<SearchSource>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedQueryPlan {
    pub kinds: Vec<ContentKind>,
    pub keywords: Vec<String>,
    pub aliases: Vec<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedUnifiedSearch {
    pub plan: UnifiedQueryPlan,
    pub understood_terms: Vec<String>,
    pub audit: crate::vault::models::AiRequestAudit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MainContentOpen {
    pub id: String,
}

impl MainContentOpen {
    pub fn new(id: &str) -> Result<Self, String> {
        UnifiedContentId::parse(id)?;
        Ok(Self { id: id.to_string() })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentChange {
    pub id: String,
    pub operation: ContentOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentChangedEvent {
    pub revision: i64,
    pub changes: Vec<ContentChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentDeleteFailedEvent {
    pub token: String,
    pub id: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMutation<T> {
    pub value: T,
    pub revision: i64,
    pub changes: Vec<ContentChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteUndoToken {
    pub token: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentRevision {
    pub revision: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_summary(kind: ContentKind, id: &str) -> ContentSummary {
        let retention = match kind {
            ContentKind::Text | ContentKind::Image | ContentKind::File => RetentionState::Temporary,
            ContentKind::Credential | ContentKind::Bookmark | ContentKind::Note => {
                RetentionState::Saved
            }
        };
        ContentSummary {
            id: id.to_string(),
            kind,
            retention,
            title: format!("{kind:?}"),
            preview: None,
            created_at: "2026-07-18T08:00:00Z".to_string(),
            updated_at: "2026-07-18T08:01:00Z".to_string(),
            cleanup_at: None,
            capabilities: ContentCapabilities::for_item(kind, retention, false),
        }
    }

    #[test]
    fn content_source_round_trips_and_rejects_unknown_values() {
        for source in [ContentSource::Dock, ContentSource::Vault] {
            assert_eq!(ContentSource::parse(source.as_str()), Some(source));
        }
        assert_eq!(ContentSource::parse("archive"), None);
    }

    #[test]
    fn unified_content_id_round_trips_for_each_namespace() {
        for (source, source_id, expected) in [
            (ContentSource::Dock, "de-17", "dock:de-17"),
            (ContentSource::Vault, "ve-9", "vault:ve-9"),
        ] {
            let id = UnifiedContentId::new(source, source_id).unwrap();

            assert_eq!(id.as_str(), expected);
            assert_eq!(UnifiedContentId::parse(id.as_str()).unwrap(), id);
        }
    }

    #[test]
    fn unified_content_id_rejects_invalid_namespaces_and_empty_source_ids() {
        assert!(UnifiedContentId::parse("de-17").is_err());
        assert!(UnifiedContentId::parse("archive:de-17").is_err());
        assert!(UnifiedContentId::parse("dock:").is_err());
        assert!(UnifiedContentId::new(ContentSource::Vault, "").is_err());
    }

    #[test]
    fn capabilities_follow_kind_retention_and_reorderability() {
        let kind_capabilities = [
            (ContentKind::Text, true, false, false, false, false, false),
            (ContentKind::Image, false, true, false, true, false, false),
            (ContentKind::File, false, false, true, true, false, false),
            (
                ContentKind::Credential,
                true,
                false,
                false,
                false,
                false,
                true,
            ),
            (
                ContentKind::Bookmark,
                true,
                false,
                false,
                false,
                true,
                false,
            ),
            (ContentKind::Note, true, false, false, false, false, false),
        ];

        for (kind, copy_text, copy_image, copy_file, copy_path, open_url, reveal_sensitive) in
            kind_capabilities
        {
            let temporary = ContentCapabilities::for_item(kind, RetentionState::Temporary, true);
            assert_eq!(temporary.copy_text, copy_text, "{kind:?}");
            assert_eq!(temporary.copy_image, copy_image, "{kind:?}");
            assert_eq!(temporary.copy_file, copy_file, "{kind:?}");
            assert_eq!(temporary.copy_path, copy_path, "{kind:?}");
            assert_eq!(temporary.open_url, open_url, "{kind:?}");
            assert_eq!(temporary.reveal_sensitive, reveal_sensitive, "{kind:?}");
            assert!(temporary.edit, "{kind:?}");
            assert!(temporary.save, "{kind:?}");
            assert!(!temporary.unsave, "{kind:?}");
            assert!(temporary.delete, "{kind:?}");
            assert!(temporary.reorder, "{kind:?}");

            let saved = ContentCapabilities::for_item(kind, RetentionState::Saved, false);
            assert!(!saved.save, "{kind:?}");
            assert!(saved.unsave, "{kind:?}");
            assert!(!saved.reorder, "{kind:?}");
        }
    }

    #[test]
    fn serde_contract_uses_opaque_ids_camel_case_fields_and_frontend_enum_values() {
        let id = UnifiedContentId::new(ContentSource::Dock, "de-17").unwrap();
        assert_eq!(serde_json::to_value(&id).unwrap(), json!("dock:de-17"));
        assert_eq!(
            serde_json::from_value::<UnifiedContentId>(json!("dock:de-17")).unwrap(),
            id
        );

        let summary = ContentSummary {
            id: "dock:de-17".to_string(),
            kind: ContentKind::Text,
            retention: RetentionState::Temporary,
            title: "Snippet".to_string(),
            preview: Some("hello".to_string()),
            created_at: "2026-07-18T08:00:00Z".to_string(),
            updated_at: "2026-07-18T08:01:00Z".to_string(),
            cleanup_at: Some("2026-07-25T08:00:00Z".to_string()),
            capabilities: ContentCapabilities::for_item(
                ContentKind::Text,
                RetentionState::Temporary,
                true,
            ),
        };
        let summary_json = serde_json::to_value(&summary).unwrap();
        assert_eq!(summary_json["id"], json!("dock:de-17"));
        assert_eq!(summary_json["kind"], json!("text"));
        assert_eq!(summary_json["retention"], json!("temporary"));
        assert_eq!(summary_json["cleanupAt"], json!("2026-07-25T08:00:00Z"));
        assert!(summary_json.get("cleanup_at").is_none());

        let field = UnifiedField {
            key: "password".to_string(),
            value: "secret".to_string(),
            is_sensitive: true,
            sort_order: 2,
        };
        let field_json = serde_json::to_value(&field).unwrap();
        assert_eq!(field_json["isSensitive"], json!(true));
        assert_eq!(field_json["sortOrder"], json!(2));

        assert_eq!(
            serde_json::to_value(SearchSource::AiExpanded).unwrap(),
            json!("aiExpanded")
        );

        let undo = DeleteUndoToken {
            token: "undo-1".to_string(),
            expires_at: "2026-07-18T08:05:00Z".to_string(),
        };
        assert_eq!(
            serde_json::to_value(undo).unwrap(),
            json!({
                "token": "undo-1",
                "expiresAt": "2026-07-18T08:05:00Z"
            })
        );
    }

    #[test]
    fn content_detail_is_tagged_by_content_kind_without_source_internals() {
        let summary = ContentSummary {
            id: "vault:ve-9".to_string(),
            kind: ContentKind::Credential,
            retention: RetentionState::Saved,
            title: "Production login".to_string(),
            preview: None,
            created_at: "2026-07-18T08:00:00Z".to_string(),
            updated_at: "2026-07-18T08:01:00Z".to_string(),
            cleanup_at: None,
            capabilities: ContentCapabilities::for_item(
                ContentKind::Credential,
                RetentionState::Saved,
                false,
            ),
        };
        let detail = ContentDetail::Credential {
            summary,
            fields: vec![UnifiedField {
                key: "username".to_string(),
                value: "operator".to_string(),
                is_sensitive: false,
                sort_order: 0,
            }],
            notes: Some("Rotated monthly".to_string()),
            tags: vec![UnifiedTag {
                tag: "Work".to_string(),
                normalized_tag: "work".to_string(),
                source: ContentTagSource::Manual,
            }],
        };

        let value = serde_json::to_value(detail).unwrap();
        assert_eq!(value["kind"], json!("credential"));
        assert_eq!(value["summary"]["id"], json!("vault:ve-9"));
        assert!(value.get("source").is_none());
        assert!(value.get("sourceId").is_none());
        assert!(value.get("table").is_none());
    }

    #[test]
    fn all_content_detail_variants_round_trip_with_kind_tags_and_camel_case_assets() {
        let field = UnifiedField {
            key: "username".to_string(),
            value: "operator".to_string(),
            is_sensitive: false,
            sort_order: 0,
        };
        let tag = UnifiedTag {
            tag: "Work".to_string(),
            normalized_tag: "work".to_string(),
            source: ContentTagSource::Manual,
        };
        let cases = [
            (
                "text",
                ContentDetail::Text {
                    summary: test_summary(ContentKind::Text, "dock:de-text"),
                    title: "Text".to_string(),
                    body: "hello".to_string(),
                },
            ),
            (
                "image",
                ContentDetail::Image {
                    summary: test_summary(ContentKind::Image, "dock:de-image"),
                    file_name: "photo.png".to_string(),
                    asset_path: "assets/photo.png".to_string(),
                    mime_type: Some("image/png".to_string()),
                    width: Some(640),
                    height: Some(480),
                    available: true,
                },
            ),
            (
                "file",
                ContentDetail::File {
                    summary: test_summary(ContentKind::File, "dock:de-file"),
                    file_name: "report.pdf".to_string(),
                    asset_path: "assets/report.pdf".to_string(),
                    mime_type: Some("application/pdf".to_string()),
                    size_bytes: Some(4096),
                    available: true,
                },
            ),
            (
                "credential",
                ContentDetail::Credential {
                    summary: test_summary(ContentKind::Credential, "vault:ve-credential"),
                    fields: vec![field.clone()],
                    notes: Some("Rotate monthly".to_string()),
                    tags: vec![tag.clone()],
                },
            ),
            (
                "bookmark",
                ContentDetail::Bookmark {
                    summary: test_summary(ContentKind::Bookmark, "vault:ve-bookmark"),
                    url: "https://example.test".to_string(),
                    fields: vec![field.clone()],
                    notes: None,
                    tags: vec![tag.clone()],
                },
            ),
            (
                "note",
                ContentDetail::Note {
                    summary: test_summary(ContentKind::Note, "vault:ve-note"),
                    body: "Remember this".to_string(),
                    fields: vec![field],
                    tags: vec![tag],
                },
            ),
        ];

        for (expected_kind, detail) in cases {
            let value = serde_json::to_value(&detail).unwrap();
            assert_eq!(value["kind"], json!(expected_kind));
            assert_eq!(value["summary"]["kind"], json!(expected_kind));

            if expected_kind == "image" {
                assert_eq!(value["fileName"], json!("photo.png"));
                assert_eq!(value["assetPath"], json!("assets/photo.png"));
                assert_eq!(value["mimeType"], json!("image/png"));
                assert!(value.get("file_name").is_none());
                assert!(value.get("asset_path").is_none());
                assert!(value.get("mime_type").is_none());
            }
            if expected_kind == "file" {
                assert_eq!(value["sizeBytes"], json!(4096));
                assert!(value.get("size_bytes").is_none());
            }

            let decoded: ContentDetail = serde_json::from_value(value).unwrap();
            assert_eq!(decoded, detail);
        }
    }

    #[test]
    fn browse_scope_and_content_operation_use_stable_wire_values() {
        for (scope, wire) in [
            (BrowseScope::Temporary, "temporary"),
            (BrowseScope::All, "all"),
            (BrowseScope::Saved, "saved"),
        ] {
            assert_eq!(serde_json::to_value(scope).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<BrowseScope>(json!(wire)).unwrap(),
                scope
            );
        }

        for (operation, wire) in [
            (ContentOperation::Created, "created"),
            (ContentOperation::Updated, "updated"),
            (ContentOperation::Retention, "retention"),
            (ContentOperation::Reordered, "reordered"),
            (ContentOperation::Deleted, "deleted"),
            (ContentOperation::Restored, "restored"),
        ] {
            assert_eq!(serde_json::to_value(operation).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<ContentOperation>(json!(wire)).unwrap(),
                operation
            );
        }
    }

    #[test]
    fn mutations_and_events_use_stable_camel_case_json_shapes() {
        let change = ContentChange {
            id: "dock:de-17".to_string(),
            operation: ContentOperation::Retention,
        };
        let mutation = ContentMutation {
            value: DeleteUndoToken {
                token: "undo-1".to_string(),
                expires_at: "2026-07-18T08:05:00Z".to_string(),
            },
            revision: 12,
            changes: vec![change.clone()],
        };
        let mutation_json = json!({
            "value": {
                "token": "undo-1",
                "expiresAt": "2026-07-18T08:05:00Z"
            },
            "revision": 12,
            "changes": [{
                "id": "dock:de-17",
                "operation": "retention"
            }]
        });
        assert_eq!(serde_json::to_value(&mutation).unwrap(), mutation_json);
        assert_eq!(
            serde_json::from_value::<ContentMutation<DeleteUndoToken>>(mutation_json).unwrap(),
            mutation
        );

        let changed = ContentChangedEvent {
            revision: 12,
            changes: vec![change],
        };
        assert_eq!(
            serde_json::to_value(changed).unwrap(),
            json!({
                "revision": 12,
                "changes": [{
                    "id": "dock:de-17",
                    "operation": "retention"
                }]
            })
        );

        let failed = ContentDeleteFailedEvent {
            token: "undo-1".to_string(),
            id: "vault:ve-9".to_string(),
            code: "conflict".to_string(),
        };
        assert_eq!(
            serde_json::to_value(failed).unwrap(),
            json!({
                "token": "undo-1",
                "id": "vault:ve-9",
                "code": "conflict"
            })
        );
    }

    #[test]
    fn unified_content_id_deserialization_rejects_invalid_wire_values() {
        for value in [
            json!("de-17"),
            json!("archive:de-17"),
            json!("dock:"),
            json!(null),
        ] {
            assert!(serde_json::from_value::<UnifiedContentId>(value).is_err());
        }
    }
}
