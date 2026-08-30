//! Unified data export: serialize all content (temporary + saved) into
//! portable files (xlsx / csv / markdown / json) so users can back up or
//! migrate their data to other applications.
//!
//! The export is read-only: it never mutates the database and never emits
//! `content-changed`. Sensitive field values are collected with their real
//! values but masked at write time unless the caller explicitly opts in via
//! `include_sensitive`.

use std::path::Path;

use rusqlite::Connection;

use crate::content::models::{
    BrowseScope, ContentDetail, ContentKind, RetentionState, UnifiedField,
};
use crate::storage::error::{StorageError, StorageResult};

const MASK: &str = "******";

fn kind_str(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Text => "text",
        ContentKind::Image => "image",
        ContentKind::File => "file",
        ContentKind::Credential => "credential",
        ContentKind::Bookmark => "bookmark",
        ContentKind::Note => "note",
    }
}

fn retention_str(retention: RetentionState) -> &'static str {
    match retention {
        RetentionState::Temporary => "temporary",
        RetentionState::Saved => "saved",
    }
}

/// A single field prepared for export. `value` always holds the real value;
/// masking happens in the format writers via `display_value`.
#[derive(Debug, Clone)]
pub struct ExportField {
    pub key: String,
    pub value: String,
    pub sensitive: bool,
}

impl ExportField {
    fn display_value(&self, include_sensitive: bool) -> &str {
        if self.sensitive && !include_sensitive {
            MASK
        } else {
            &self.value
        }
    }
}

/// A flattened, format-independent representation of one content entry.
#[derive(Debug, Clone)]
pub struct ExportItem {
    pub id: String,
    pub kind: String,
    pub retention: String,
    pub title: String,
    pub preview: Option<String>,
    pub body: Option<String>,
    pub url: Option<String>,
    pub file_name: Option<String>,
    pub asset_path: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub fields: Vec<ExportField>,
    pub notes: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub cleanup_at: Option<String>,
}

impl ExportItem {
    /// `key=value` pairs joined for flat formats (csv/xlsx).
    fn fields_joined(&self, include_sensitive: bool) -> String {
        self.fields
            .iter()
            .map(|f| format!("{}={}", f.key, f.display_value(include_sensitive)))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn tags_joined(&self) -> String {
        self.tags.join(", ")
    }
}

/// Load every unified content entry with full detail. Temporary and saved
/// entries are both included; ordering matches the "all" scope (updated_at).
pub fn collect_items(conn: &Connection) -> StorageResult<Vec<ExportItem>> {
    let summaries = crate::content::service::list(conn, BrowseScope::All, None)?;
    let mut items = Vec::with_capacity(summaries.len());
    for summary in summaries {
        let detail = crate::content::service::detail(conn, &summary.id)?;
        items.push(export_item(detail));
    }
    Ok(items)
}

fn export_fields(fields: &[UnifiedField]) -> Vec<ExportField> {
    let mut sorted: Vec<UnifiedField> = fields.to_vec();
    sorted.sort_by_key(|f| f.sort_order);
    sorted
        .into_iter()
        .map(|f| ExportField {
            key: f.key,
            value: f.value,
            sensitive: f.is_sensitive,
        })
        .collect()
}

fn export_item(detail: ContentDetail) -> ExportItem {
    match detail {
        ContentDetail::Text {
            summary,
            title,
            body,
            ..
        } => ExportItem {
            body: Some(body),
            ..base(&summary, &title)
        },
        ContentDetail::Image {
            summary,
            file_name,
            asset_path,
            mime_type,
            width,
            height,
            ..
        } => ExportItem {
            file_name: Some(file_name),
            asset_path: Some(asset_path),
            mime_type,
            width,
            height,
            ..base(&summary, &summary.title)
        },
        ContentDetail::File {
            summary,
            file_name,
            asset_path,
            mime_type,
            size_bytes,
            ..
        } => ExportItem {
            file_name: Some(file_name),
            asset_path: Some(asset_path),
            mime_type,
            size_bytes,
            ..base(&summary, &summary.title)
        },
        ContentDetail::Credential {
            summary,
            fields,
            notes,
            tags,
            ..
        } => ExportItem {
            fields: export_fields(&fields),
            notes,
            tags: tag_list(&tags),
            ..base(&summary, &summary.title)
        },
        ContentDetail::Bookmark {
            summary,
            url,
            fields,
            notes,
            tags,
            ..
        } => ExportItem {
            url: Some(url),
            fields: export_fields(&fields),
            notes,
            tags: tag_list(&tags),
            ..base(&summary, &summary.title)
        },
        ContentDetail::Note {
            summary,
            body,
            fields,
            tags,
            ..
        } => ExportItem {
            body: Some(body),
            fields: export_fields(&fields),
            tags: tag_list(&tags),
            ..base(&summary, &summary.title)
        },
    }
}

fn tag_list(tags: &[crate::content::models::UnifiedTag]) -> Vec<String> {
    tags.iter().map(|t| t.tag.clone()).collect()
}

fn base(summary: &crate::content::models::ContentSummary, title: &str) -> ExportItem {
    ExportItem {
        id: summary.id.clone(),
        kind: kind_str(summary.kind).to_string(),
        retention: retention_str(summary.retention).to_string(),
        title: title.to_string(),
        preview: summary.preview.clone(),
        body: None,
        url: None,
        file_name: None,
        asset_path: None,
        mime_type: None,
        size_bytes: None,
        width: None,
        height: None,
        fields: Vec::new(),
        notes: None,
        tags: Vec::new(),
        created_at: summary.created_at.clone(),
        updated_at: summary.updated_at.clone(),
        cleanup_at: summary.cleanup_at.clone(),
    }
}

// --- Format writers ------------------------------------------------------

/// Write `items` to `path` in the requested format
/// (`xlsx` | `csv` | `markdown` | `json`). Returns the item count written.
pub fn write_export(
    path: &Path,
    items: &[ExportItem],
    format: &str,
    include_sensitive: bool,
) -> StorageResult<usize> {
    match format {
        "xlsx" => write_xlsx(path, items, include_sensitive)?,
        "csv" => write_csv(path, items, include_sensitive)?,
        "markdown" => write_markdown(path, items, include_sensitive)?,
        "json" => write_json(path, items, include_sensitive)?,
        other => {
            return Err(StorageError::Validation(format!(
                "unknown export format: {other}"
            )))
        }
    }
    Ok(items.len())
}

const CSV_COLUMNS: &[&str] = &[
    "id",
    "kind",
    "retention",
    "title",
    "preview",
    "content",
    "url",
    "file_name",
    "asset_path",
    "mime_type",
    "size_bytes",
    "width",
    "height",
    "fields",
    "notes",
    "tags",
    "created_at",
    "updated_at",
    "cleanup_at",
];

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_row(values: &[String]) -> String {
    values
        .iter()
        .map(|v| csv_cell(v))
        .collect::<Vec<_>>()
        .join(",")
}

/// Flatten one item into the CSV/XLSX column order.
fn flat_row(item: &ExportItem, include_sensitive: bool) -> Vec<String> {
    vec![
        item.id.clone(),
        item.kind.clone(),
        item.retention.clone(),
        item.title.clone(),
        item.preview.clone().unwrap_or_default(),
        item.body.clone().unwrap_or_default(),
        item.url.clone().unwrap_or_default(),
        item.file_name.clone().unwrap_or_default(),
        item.asset_path.clone().unwrap_or_default(),
        item.mime_type.clone().unwrap_or_default(),
        item.size_bytes.map(|v| v.to_string()).unwrap_or_default(),
        item.width.map(|v| v.to_string()).unwrap_or_default(),
        item.height.map(|v| v.to_string()).unwrap_or_default(),
        item.fields_joined(include_sensitive),
        item.notes.clone().unwrap_or_default(),
        item.tags_joined(),
        item.created_at.clone(),
        item.updated_at.clone(),
        item.cleanup_at.clone().unwrap_or_default(),
    ]
}

/// Build the full CSV document. A UTF-8 BOM keeps Excel from mangling CJK
/// text when the file is opened by double-click.
pub fn to_csv(items: &[ExportItem], include_sensitive: bool) -> String {
    let mut out = String::from("\u{feff}");
    let header: Vec<String> = CSV_COLUMNS.iter().map(|s| s.to_string()).collect();
    out.push_str(&csv_row(&header));
    out.push('\n');
    for item in items {
        out.push_str(&csv_row(&flat_row(item, include_sensitive)));
        out.push('\n');
    }
    out
}

fn write_csv(path: &Path, items: &[ExportItem], include_sensitive: bool) -> StorageResult<()> {
    std::fs::write(path, to_csv(items, include_sensitive).as_bytes()).map_err(StorageError::Io)
}

/// Serialize items as a JSON array. Sensitive values are masked unless
/// `include_sensitive`; `ExportItem` is deliberately not `Serialize` so the
/// masked path is the only way out.
pub fn to_json(items: &[ExportItem], include_sensitive: bool) -> StorageResult<Vec<u8>> {
    let values: Vec<serde_json::Value> = items
        .iter()
        .map(|item| item_to_json(item, include_sensitive))
        .collect();
    serde_json::to_vec_pretty(&values).map_err(StorageError::Serialization)
}

fn item_to_json(item: &ExportItem, include_sensitive: bool) -> serde_json::Value {
    let fields: Vec<serde_json::Value> = item
        .fields
        .iter()
        .map(|f| {
            serde_json::json!({
                "key": f.key,
                "value": f.display_value(include_sensitive),
                "sensitive": f.sensitive,
            })
        })
        .collect();
    serde_json::json!({
        "id": item.id,
        "kind": item.kind,
        "retention": item.retention,
        "title": item.title,
        "preview": item.preview,
        "body": item.body,
        "url": item.url,
        "fileName": item.file_name,
        "assetPath": item.asset_path,
        "mimeType": item.mime_type,
        "sizeBytes": item.size_bytes,
        "width": item.width,
        "height": item.height,
        "fields": fields,
        "notes": item.notes,
        "tags": item.tags,
        "createdAt": item.created_at,
        "updatedAt": item.updated_at,
        "cleanupAt": item.cleanup_at,
    })
}

fn write_json(path: &Path, items: &[ExportItem], include_sensitive: bool) -> StorageResult<()> {
    std::fs::write(path, to_json(items, include_sensitive)?).map_err(StorageError::Io)
}

const KIND_LABELS: &[(&str, &str)] = &[
    ("text", "Text"),
    ("image", "Image"),
    ("file", "File"),
    ("credential", "Credential"),
    ("bookmark", "Bookmark"),
    ("note", "Note"),
];

/// Build a readable Markdown document grouped by content kind.
pub fn to_markdown(items: &[ExportItem], include_sensitive: bool) -> String {
    let mut out = String::from("# Soma Scratchpad Export\n\n");
    let now = chrono::Utc::now().to_rfc3339();
    out.push_str(&format!("- Exported at: {now}\n"));
    out.push_str(&format!("- Total items: {}\n", items.len()));
    if !include_sensitive {
        out.push_str(&format!("- Sensitive fields are masked with `{MASK}`\n"));
    }
    out.push('\n');

    for (kind, label) in KIND_LABELS {
        let group: Vec<&ExportItem> = items.iter().filter(|i| &i.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("## {label} ({} items)\n\n", group.len()));
        for item in group {
            let title = if item.title.is_empty() {
                "(untitled)"
            } else {
                &item.title
            };
            out.push_str(&format!("### {title}\n\n"));
            out.push_str(&format!("- ID: `{}`\n", item.id));
            out.push_str(&format!("- Retention: {}\n", item.retention));
            out.push_str(&format!("- Created: {}\n", item.created_at));
            out.push_str(&format!("- Updated: {}\n", item.updated_at));
            if let Some(cleanup) = &item.cleanup_at {
                out.push_str(&format!("- Cleanup at: {cleanup}\n"));
            }
            if let Some(url) = &item.url {
                out.push_str(&format!("- URL: {url}\n"));
            }
            if let Some(name) = &item.file_name {
                out.push_str(&format!("- File: {name}\n"));
            }
            if let Some(path) = &item.asset_path {
                out.push_str(&format!("- Path: `{path}`\n"));
            }
            if !item.fields.is_empty() {
                out.push_str("\n| Field | Value |\n| --- | --- |\n");
                for f in &item.fields {
                    out.push_str(&format!(
                        "| {} | {} |\n",
                        f.key.replace('|', "\\|"),
                        f.display_value(include_sensitive).replace('|', "\\|")
                    ));
                }
            }
            if let Some(body) = &item.body {
                if !body.is_empty() {
                    out.push_str("\n```\n");
                    out.push_str(body);
                    out.push_str("\n```\n");
                }
            }
            if let Some(notes) = &item.notes {
                if !notes.is_empty() {
                    let notes = notes.replace('\n', " ");
                    out.push_str(&format!("\n> Notes: {notes}\n"));
                }
            }
            if !item.tags.is_empty() {
                let tags = item
                    .tags
                    .iter()
                    .map(|t| format!("`{t}`"))
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push_str(&format!("\nTags: {tags}\n"));
            }
            out.push('\n');
        }
    }
    out
}

fn write_markdown(path: &Path, items: &[ExportItem], include_sensitive: bool) -> StorageResult<()> {
    std::fs::write(path, to_markdown(items, include_sensitive).as_bytes()).map_err(StorageError::Io)
}

fn write_xlsx(path: &Path, items: &[ExportItem], include_sensitive: bool) -> StorageResult<()> {
    use rust_xlsxwriter::{Format, Workbook};

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Scratchpad").map_err(xlsx_err)?;

    let header_format = Format::new().set_bold();
    for (col, header) in CSV_COLUMNS.iter().enumerate() {
        let col = col as u16;
        sheet
            .write_with_format(0, col, *header, &header_format)
            .map_err(xlsx_err)?;
        // Give long columns (content/paths) room without wall-of-text.
        let width = match *header {
            "id" | "asset_path" | "fields" => 34.0,
            "content" | "preview" | "notes" | "url" => 40.0,
            "created_at" | "updated_at" | "cleanup_at" => 24.0,
            "title" => 22.0,
            _ => 12.0,
        };
        sheet.set_column_width(col, width).map_err(xlsx_err)?;
    }
    sheet.set_freeze_panes(1, 0).map_err(xlsx_err)?;

    for (row_idx, item) in items.iter().enumerate() {
        let row = (row_idx + 1) as u32;
        for (col, value) in flat_row(item, include_sensitive).into_iter().enumerate() {
            sheet
                .write(row, col as u16, value.as_str())
                .map_err(xlsx_err)?;
        }
    }

    workbook.save(path).map_err(xlsx_err)?;
    Ok(())
}

fn xlsx_err(e: rust_xlsxwriter::XlsxError) -> StorageError {
    StorageError::Other(e.to_string())
}

/// Export every content entry to `path` in the requested format
/// (`xlsx` | `csv` | `markdown` | `json`). Returns the number of items
/// written so the UI can confirm the result to the user.
#[tauri::command]
pub(crate) fn ipc_content_export(
    state: tauri::State<'_, crate::AppState>,
    path: String,
    format: String,
    include_sensitive: bool,
) -> Result<usize, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let items = collect_items(&conn).map_err(|e| e.to_string())?;
    write_export(
        std::path::Path::new(&path),
        &items,
        &format,
        include_sensitive,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::models::{BrowseScope, ContentKind};
    use crate::models::entry::EntryView;
    use crate::vault::models::{EntryKind, FieldInput, VaultEntryInput};
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::scratchpad::storage::ensure_dock_schema(&mut conn).unwrap();
        crate::vault::storage::ensure_vault_schema(&mut conn).unwrap();
        crate::content::migrations::ensure_content_schema(&mut conn, 7).unwrap();
        conn
    }

    fn seed(conn: &mut Connection) {
        crate::scratchpad::storage::create_text_entry_with_revision(
            conn,
            EntryView::Home,
            "multi\nline, with \"quotes\" and, commas",
            "manual",
        )
        .unwrap();
        vault_create(
            conn,
            "Server login",
            EntryKind::Credential,
            vec![
                FieldInput {
                    key: "username".into(),
                    value: "admin".into(),
                    is_sensitive: false,
                },
                FieldInput {
                    key: "password".into(),
                    value: "hunter2".into(),
                    is_sensitive: true,
                },
            ],
        );
        // Vault entries default to saved retention, which the bookmark below
        // exercises without an explicit save call.
        vault_create(
            conn,
            "Docs",
            EntryKind::Bookmark,
            vec![FieldInput {
                key: "url".into(),
                value: "https://example.com".into(),
                is_sensitive: false,
            }],
        );
    }

    fn vault_create(
        conn: &mut Connection,
        title: &str,
        kind: EntryKind,
        fields: Vec<FieldInput>,
    ) -> crate::vault::models::VaultEntryDetail {
        crate::vault::storage::create_entry(
            conn,
            &VaultEntryInput {
                kind,
                title: title.into(),
                fields,
                notes: Some("rotate soon".into()),
                manual_tags: vec!["work".into()],
            },
        )
        .unwrap()
    }

    #[test]
    fn collect_items_returns_every_kind_across_scopes() {
        let mut conn = test_conn();
        seed(&mut conn);
        let items = collect_items(&conn).unwrap();
        assert_eq!(items.len(), 3);
        let kinds: Vec<&str> = items.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"text"));
        assert!(kinds.contains(&"credential"));
        let bookmark = items.iter().find(|i| i.kind == "bookmark").unwrap();
        assert_eq!(bookmark.retention, "saved");
        assert_eq!(bookmark.url.as_deref(), Some("https://example.com"));
        assert_eq!(bookmark.tags, vec!["work".to_string()]);
    }

    #[test]
    fn csv_export_masks_sensitive_fields_unless_opted_in() {
        let mut conn = test_conn();
        seed(&mut conn);
        let items = collect_items(&conn).unwrap();
        let masked = to_csv(&items, false);
        assert!(!masked.contains("hunter2"));
        assert!(masked.contains(MASK));
        let open = to_csv(&items, true);
        assert!(open.contains("hunter2"));
    }

    #[test]
    fn csv_escapes_quotes_commas_and_newlines() {
        let item = ExportItem {
            id: "dock:1".into(),
            kind: "text".into(),
            retention: "temporary".into(),
            title: "quote\",comma".into(),
            preview: None,
            body: Some("line1\nline2".into()),
            url: None,
            file_name: None,
            asset_path: None,
            mime_type: None,
            size_bytes: None,
            width: None,
            height: None,
            fields: Vec::new(),
            notes: None,
            tags: Vec::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            cleanup_at: None,
        };
        let csv = to_csv(&[item], false);
        assert!(csv.starts_with('\u{feff}'));
        // The body contains a newline, so the data record spans two raw lines
        // inside quotes — assert on the document rather than a single line.
        assert!(csv.contains("\"quote\"\",comma\""));
        assert!(csv.contains("\"line1\nline2\""));
    }

    #[test]
    fn markdown_groups_by_kind_and_masks_without_sensitive_opt_in() {
        let mut conn = test_conn();
        seed(&mut conn);
        let items = collect_items(&conn).unwrap();
        let md = to_markdown(&items, false);
        assert!(md.contains("# Soma Scratchpad Export"));
        assert!(md.contains("## Credential (1 items)"));
        assert!(!md.contains("hunter2"));
        assert!(md.contains(MASK));

        let md_open = to_markdown(&items, true);
        assert!(md_open.contains("hunter2"));
    }

    #[test]
    fn json_export_masks_sensitive_values_and_exposes_flag() {
        let mut conn = test_conn();
        seed(&mut conn);
        let items = collect_items(&conn).unwrap();

        let masked = to_json(&items, false).unwrap();
        let masked_str = String::from_utf8(masked).unwrap();
        assert!(!masked_str.contains("hunter2"));
        let parsed: serde_json::Value = serde_json::from_str(&masked_str).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 3);
        assert!(parsed[0]["createdAt"].is_string());

        let open = to_json(&items, true).unwrap();
        assert!(String::from_utf8(open).unwrap().contains("hunter2"));
    }

    #[test]
    fn xlsx_export_writes_a_real_file_starting_with_zip_magic() {
        let mut conn = test_conn();
        seed(&mut conn);
        let items = collect_items(&conn).unwrap();
        let dir = std::env::temp_dir().join(format!("soma-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.xlsx");
        let written = write_export(&path, &items, "xlsx", false).unwrap();
        assert_eq!(written, 3);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_export_rejects_unknown_formats() {
        let dir = std::env::temp_dir();
        let err = write_export(&dir.join("soma-none"), &[], "pdf", false).unwrap_err();
        assert!(matches!(err, StorageError::Validation(_)));
    }

    #[test]
    fn collect_on_empty_database_returns_empty_vec() {
        let conn = test_conn();
        let items = collect_items(&conn).unwrap();
        assert!(items.is_empty());
        assert_eq!(to_csv(&items, false).lines().count(), 1);
    }

    #[test]
    fn export_respects_kind_filtering_through_service_list() {
        let mut conn = test_conn();
        seed(&mut conn);
        let summaries =
            crate::content::service::list(&conn, BrowseScope::Saved, Some(ContentKind::Bookmark))
                .unwrap();
        assert_eq!(summaries.len(), 1);
    }
}
