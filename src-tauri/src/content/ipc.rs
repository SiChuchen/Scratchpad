use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration as StdDuration;
use tauri::{Emitter, Manager};

use crate::content::catalog::{bump_revision, current_revision, summary_by_id};
use crate::content::models::{
    BrowseScope, ContentChange, ContentChangedEvent, ContentDeleteFailedEvent, ContentDetail,
    ContentKind, ContentMutation, ContentOperation, ContentRevision, ContentSearchHit,
    ContentSummary, DeleteUndoToken, UnifiedContentId, UnifiedQueryPlan,
};
use crate::storage::error::{StorageError, StorageResult};
use crate::AppState;

const DELETE_GRACE: Duration = Duration::seconds(10);
const DEFAULT_SEARCH_LIMIT: usize = 50;
const UNDO_EXPIRED: &str = "content_delete_undo_expired";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingDelete {
    token: String,
    id: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Debug)]
pub(crate) struct CommittedDelete {
    pub token: String,
    pub id: String,
    pub revision: i64,
    attachment: Option<String>,
}

pub(crate) fn content_changed_event<T>(mutation: &ContentMutation<T>) -> ContentChangedEvent {
    ContentChangedEvent {
        revision: mutation.revision,
        changes: mutation.changes.clone(),
    }
}

pub(crate) fn dispatch_content_changed<T, E>(
    mutation: &ContentMutation<T>,
    emit: impl FnOnce(&str, ContentChangedEvent) -> Result<(), E>,
) {
    if mutation.changes.is_empty() {
        return;
    }
    if emit("content-changed", content_changed_event(mutation)).is_err() {
        eprintln!("failed to emit content-changed after committed content mutation");
    }
}

pub(crate) fn emit_content_changed<T>(app: &tauri::AppHandle, mutation: &ContentMutation<T>) {
    dispatch_content_changed(mutation, |name, payload| app.emit(name, payload));
}

pub(crate) fn content_revision(conn: &Connection) -> StorageResult<ContentRevision> {
    Ok(ContentRevision {
        revision: current_revision(conn)?,
    })
}

fn parse_pending_timestamp(value: &str, field: &str, token: &str) -> StorageResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            StorageError::Validation(format!(
                "invalid pending delete {field} for {token}: {error}"
            ))
        })
}

fn pending_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, String, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

pub(crate) fn prepare_delete(
    conn: &mut Connection,
    id: &str,
    now: DateTime<Utc>,
    grace: Duration,
) -> StorageResult<DeleteUndoToken> {
    UnifiedContentId::parse(id).map_err(StorageError::Validation)?;
    let expires_at = now.checked_add_signed(grace).ok_or_else(|| {
        StorageError::Validation("content delete expiry is out of range".to_string())
    })?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    summary_by_id(&tx, id, false)?;
    tx.execute(
        "DELETE FROM content_pending_deletes WHERE unified_id=?1 AND status='failed'",
        params![id],
    )?;
    if let Some((token, stored_expiry)) = tx
        .query_row(
            "SELECT token, expires_at FROM content_pending_deletes WHERE unified_id=?1 AND status='pending'",
            params![id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        parse_pending_timestamp(&stored_expiry, "expires_at", &token)?;
        tx.commit()?;
        return Ok(DeleteUndoToken { token, expires_at: stored_expiry });
    }
    let token = hex::encode(rand::random::<[u8; 32]>());
    let created_at = now.to_rfc3339();
    let expires_at = expires_at.to_rfc3339();
    tx.execute(
        "INSERT INTO content_pending_deletes(token, unified_id, created_at, expires_at, status) VALUES (?1, ?2, ?3, ?4, 'pending')",
        params![token, id, created_at, expires_at],
    )?;
    tx.commit()?;
    Ok(DeleteUndoToken { token, expires_at })
}

pub(crate) fn cancel_pending_delete(
    conn: &mut Connection,
    token: &str,
    now: DateTime<Utc>,
) -> StorageResult<ContentSummary> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let pending = tx
        .query_row(
            "SELECT unified_id, expires_at FROM content_pending_deletes WHERE token=?1 AND status='pending'",
            params![token],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((id, expires_at)) = pending else {
        return Err(StorageError::Other(UNDO_EXPIRED.to_string()));
    };
    let expires_at = parse_pending_timestamp(&expires_at, "expires_at", token)?;
    if now >= expires_at {
        return Err(StorageError::Other(UNDO_EXPIRED.to_string()));
    }
    let affected = tx.execute(
        "DELETE FROM content_pending_deletes WHERE token=?1 AND status='pending'",
        params![token],
    )?;
    if affected != 1 {
        return Err(StorageError::Other(UNDO_EXPIRED.to_string()));
    }
    let summary = summary_by_id(&tx, &id, false)?;
    tx.commit()?;
    Ok(summary)
}

pub(crate) fn commit_pending_delete(
    conn: &mut Connection,
    token: &str,
    now: DateTime<Utc>,
) -> StorageResult<Option<CommittedDelete>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let pending = tx
        .query_row(
            "SELECT token, unified_id, created_at, expires_at FROM content_pending_deletes WHERE token=?1 AND status='pending'",
            params![token],
            pending_from_row,
        )
        .optional()?;
    let Some((token, id, created_at, expires_at)) = pending else {
        tx.commit()?;
        return Ok(None);
    };
    parse_pending_timestamp(&created_at, "created_at", &token)?;
    let expires_at = parse_pending_timestamp(&expires_at, "expires_at", &token)?;
    if now < expires_at {
        tx.commit()?;
        return Ok(None);
    }
    let attachment = crate::content::service::delete_in_transaction(&tx, &id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(Some(CommittedDelete {
        token,
        id,
        revision,
        attachment,
    }))
}

fn mark_delete_failed(conn: &mut Connection, token: &str) -> StorageResult<bool> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let affected = tx.execute(
        "UPDATE content_pending_deletes SET status='failed' WHERE token=?1 AND status='pending'",
        params![token],
    )?;
    tx.commit()?;
    Ok(affected == 1)
}

#[cfg(test)]
pub(crate) fn process_pending_delete<CE, FE>(
    conn: &mut Connection,
    token: &str,
    now: DateTime<Utc>,
    changed: impl FnOnce(&str, ContentChangedEvent) -> Result<(), CE>,
    failed: impl FnOnce(&str, ContentDeleteFailedEvent) -> Result<(), FE>,
) -> StorageResult<()> {
    match commit_pending_delete(conn, token, now) {
        Ok(Some(committed)) => {
            crate::content::service::remove_attachment(&committed.id, committed.attachment);
            let mutation = ContentMutation {
                value: (),
                revision: committed.revision,
                changes: vec![ContentChange {
                    id: committed.id,
                    operation: ContentOperation::Deleted,
                }],
            };
            dispatch_content_changed(&mutation, changed);
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("content delete commit failed for token {token}: {error}");
            if mark_delete_failed(conn, token)? {
                let id = conn.query_row(
                    "SELECT unified_id FROM content_pending_deletes WHERE token=?1 AND status='failed'",
                    params![token],
                    |row| row.get::<_, String>(0),
                )?;
                let payload = ContentDeleteFailedEvent {
                    token: token.to_string(),
                    id,
                    code: "content_delete_commit_failed".to_string(),
                };
                if failed("content-delete-failed", payload).is_err() {
                    eprintln!("failed to emit content-delete-failed");
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn recover_pending_deletes(
    conn: &mut Connection,
    _now: DateTime<Utc>,
) -> StorageResult<Vec<PendingDelete>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "DELETE FROM content_pending_deletes WHERE status='failed'",
        [],
    )?;
    let rows = {
        let mut statement = tx.prepare(
            "SELECT token, unified_id, created_at, expires_at FROM content_pending_deletes WHERE status='pending' ORDER BY expires_at, token",
        )?;
        let rows = statement
            .query_map([], pending_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut pending = Vec::with_capacity(rows.len());
    for (token, id, created_at, expires_at) in rows {
        let parsed = UnifiedContentId::parse(&id)
            .map_err(StorageError::Validation)
            .and_then(|_| {
                Ok(PendingDelete {
                    created_at: parse_pending_timestamp(&created_at, "created_at", &token)?,
                    expires_at: parse_pending_timestamp(&expires_at, "expires_at", &token)?,
                    token: token.clone(),
                    id,
                })
            });
        match parsed {
            Ok(record) => pending.push(record),
            Err(error) => {
                eprintln!("discarding invalid pending delete token {token}: {error}");
                tx.execute(
                    "DELETE FROM content_pending_deletes WHERE token=?1",
                    params![token],
                )?;
            }
        }
    }
    tx.commit()?;
    Ok(pending)
}

#[derive(Default)]
struct DeleteWorkerGate {
    running: AtomicBool,
}

impl DeleteWorkerGate {
    const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
        }
    }

    fn claim(&self) -> bool {
        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    #[cfg(test)]
    fn release_and_reclaim(&self, has_work: bool) -> bool {
        self.running.store(false, Ordering::Release);
        has_work && self.claim()
    }
}

static DELETE_WORKER_GATE: DeleteWorkerGate = DeleteWorkerGate::new();
static DELETE_WORKER_NOTIFY: tokio::sync::Notify = tokio::sync::Notify::const_new();

struct DeleteWorkerLease {
    armed: bool,
}

impl Drop for DeleteWorkerLease {
    fn drop(&mut self) {
        if self.armed {
            DELETE_WORKER_GATE.running.store(false, Ordering::Release);
        }
    }
}

enum PendingWorkerStep {
    Gone,
    NotDue(DateTime<Utc>),
    Deleted(CommittedDelete),
}

fn pending_by_token(conn: &Connection, token: &str) -> StorageResult<Option<PendingDelete>> {
    let row = conn
        .query_row(
            "SELECT token, unified_id, created_at, expires_at FROM content_pending_deletes WHERE token=?1 AND status='pending'",
            params![token],
            pending_from_row,
        )
        .optional()?;
    row.map(|(token, id, created_at, expires_at)| {
        UnifiedContentId::parse(&id).map_err(StorageError::Validation)?;
        Ok(PendingDelete {
            created_at: parse_pending_timestamp(&created_at, "created_at", &token)?,
            expires_at: parse_pending_timestamp(&expires_at, "expires_at", &token)?,
            token,
            id,
        })
    })
    .transpose()
}

fn next_pending_delete(conn: &Connection) -> StorageResult<Option<PendingDelete>> {
    let token = conn
        .query_row(
            "SELECT token FROM content_pending_deletes WHERE status='pending' ORDER BY expires_at, token LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    token
        .map(|token| pending_by_token(conn, &token))
        .transpose()
        .map(Option::flatten)
}

fn pending_worker_step(
    conn: &mut Connection,
    token: &str,
    now: DateTime<Utc>,
) -> StorageResult<PendingWorkerStep> {
    if let Some(committed) = commit_pending_delete(conn, token, now)? {
        return Ok(PendingWorkerStep::Deleted(committed));
    }
    Ok(match pending_by_token(conn, token)? {
        Some(pending) => PendingWorkerStep::NotDue(pending.expires_at),
        None => PendingWorkerStep::Gone,
    })
}

fn is_retryable_busy(error: &StorageError) -> bool {
    matches!(
        error,
        StorageError::Sqlite(rusqlite::Error::SqliteFailure(inner, _))
            if matches!(
                inner.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

fn worker_delay(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> StdDuration {
    const MAX_SLEEP: StdDuration = StdDuration::from_secs(60 * 60);
    if expires_at <= now {
        return StdDuration::ZERO;
    }
    match (expires_at - now).to_std() {
        Ok(delay) => delay.min(MAX_SLEEP),
        Err(_) => MAX_SLEEP,
    }
}

async fn retry_mark_failed(app: &tauri::AppHandle, pending: &PendingDelete) {
    let mut backoff = StdDuration::from_millis(25);
    loop {
        let result = {
            let state = app.state::<AppState>();
            let mut conn = state.db.lock().unwrap();
            mark_delete_failed(&mut conn, &pending.token)
        };
        match result {
            Ok(true) => {
                let payload = ContentDeleteFailedEvent {
                    token: pending.token.clone(),
                    id: pending.id.clone(),
                    code: "content_delete_commit_failed".to_string(),
                };
                if app.emit("content-delete-failed", payload).is_err() {
                    eprintln!("failed to emit content-delete-failed");
                }
                return;
            }
            Ok(false) => return,
            Err(error) if is_retryable_busy(&error) => {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(StdDuration::from_secs(1));
            }
            Err(error) => {
                eprintln!(
                    "failed to mark content delete token {} failed: {error}",
                    pending.token
                );
                tokio::time::sleep(StdDuration::from_secs(1)).await;
            }
        }
    }
}

async fn run_delete_worker(app: tauri::AppHandle) {
    let mut busy_backoff = StdDuration::from_millis(25);
    loop {
        let next = {
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            next_pending_delete(&conn)
        };
        let pending = match next {
            Ok(Some(pending)) => pending,
            Ok(None) => {
                DELETE_WORKER_GATE.running.store(false, Ordering::Release);
                let has_work = {
                    let state = app.state::<AppState>();
                    let conn = state.db.lock().unwrap();
                    next_pending_delete(&conn).ok().flatten().is_some()
                };
                if has_work && DELETE_WORKER_GATE.claim() {
                    continue;
                }
                return;
            }
            Err(error) if is_retryable_busy(&error) => {
                tokio::time::sleep(busy_backoff).await;
                busy_backoff = (busy_backoff * 2).min(StdDuration::from_secs(1));
                continue;
            }
            Err(error) => {
                eprintln!("failed to inspect pending content deletes: {error}");
                tokio::time::sleep(StdDuration::from_secs(1)).await;
                continue;
            }
        };
        let delay = worker_delay(pending.expires_at, Utc::now());
        if !delay.is_zero() {
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = DELETE_WORKER_NOTIFY.notified() => {}
            }
            continue;
        }
        let step = {
            let state = app.state::<AppState>();
            let mut conn = state.db.lock().unwrap();
            pending_worker_step(&mut conn, &pending.token, Utc::now())
        };
        match step {
            Ok(PendingWorkerStep::Gone) => {
                busy_backoff = StdDuration::from_millis(25);
            }
            Ok(PendingWorkerStep::NotDue(expires_at)) => {
                let _next_delay = worker_delay(expires_at, Utc::now());
                busy_backoff = StdDuration::from_millis(25);
            }
            Ok(PendingWorkerStep::Deleted(committed)) => {
                busy_backoff = StdDuration::from_millis(25);
                let _committed_token = committed.token.as_str();
                crate::content::service::remove_attachment(&committed.id, committed.attachment);
                emit_content_changed(
                    &app,
                    &ContentMutation {
                        value: (),
                        revision: committed.revision,
                        changes: vec![ContentChange {
                            id: committed.id,
                            operation: ContentOperation::Deleted,
                        }],
                    },
                );
            }
            Err(error) if is_retryable_busy(&error) => {
                tokio::time::sleep(busy_backoff).await;
                busy_backoff = (busy_backoff * 2).min(StdDuration::from_secs(1));
            }
            Err(error) => {
                eprintln!(
                    "content delete commit failed for token {}: {error}",
                    pending.token
                );
                retry_mark_failed(&app, &pending).await;
            }
        }
    }
}

fn ensure_delete_worker_running(app: &tauri::AppHandle) {
    DELETE_WORKER_NOTIFY.notify_one();
    if !DELETE_WORKER_GATE.claim() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut lease = DeleteWorkerLease { armed: true };
        run_delete_worker(app).await;
        lease.armed = false;
    });
}

pub(crate) fn resume_pending_deletes(app: &tauri::AppHandle) {
    let has_pending = {
        let state = app.state::<AppState>();
        let mut conn = state.db.lock().unwrap();
        match recover_pending_deletes(&mut conn, Utc::now()) {
            Ok(pending) => !pending.is_empty(),
            Err(error) => {
                eprintln!("failed to recover pending content deletes: {error}");
                return;
            }
        }
    };
    if has_pending {
        ensure_delete_worker_running(app);
    }
}

fn ipc_error(error: StorageError) -> String {
    error.to_string()
}

#[tauri::command]
pub(crate) fn ipc_content_revision(
    state: tauri::State<AppState>,
) -> Result<ContentRevision, String> {
    let conn = state.db.lock().unwrap();
    content_revision(&conn).map_err(ipc_error)
}

#[tauri::command]
pub(crate) fn ipc_content_list(
    state: tauri::State<AppState>,
    scope: BrowseScope,
    kind: Option<ContentKind>,
) -> Result<Vec<ContentSummary>, String> {
    let conn = state.db.lock().unwrap();
    crate::content::service::list(&conn, scope, kind).map_err(ipc_error)
}

#[tauri::command]
pub(crate) fn ipc_content_detail(
    state: tauri::State<AppState>,
    id: String,
) -> Result<ContentDetail, String> {
    let conn = state.db.lock().unwrap();
    crate::content::service::detail(&conn, &id).map_err(ipc_error)
}

#[tauri::command]
pub(crate) fn ipc_content_search_local(
    state: tauri::State<AppState>,
    query: String,
    plan: Option<UnifiedQueryPlan>,
    limit: Option<usize>,
) -> Result<Vec<ContentSearchHit>, String> {
    let conn = state.db.lock().unwrap();
    crate::content::service::search(
        &conn,
        &query,
        plan.as_ref(),
        limit.unwrap_or(DEFAULT_SEARCH_LIMIT),
    )
    .map_err(ipc_error)
}

#[tauri::command]
pub(crate) fn ipc_content_save(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<ContentSummary, String> {
    let mutation = {
        let mut conn = state.db.lock().unwrap();
        crate::content::service::save(&mut conn, &id).map_err(ipc_error)?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
}

#[tauri::command]
pub(crate) fn ipc_content_unsave(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<ContentSummary, String> {
    let mutation = {
        let mut conn = state.db.lock().unwrap();
        let cleanup_days = crate::scratchpad::preferences::load_preferences(&conn)
            .map_err(ipc_error)?
            .auto_cleanup_days;
        crate::content::service::unsave(&mut conn, &id, cleanup_days).map_err(ipc_error)?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
}

#[tauri::command]
pub(crate) fn ipc_content_reorder(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    scope: BrowseScope,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let mutation = {
        let mut conn = state.db.lock().unwrap();
        crate::content::service::reorder(&mut conn, scope, &ordered_ids).map_err(ipc_error)?
    };
    emit_content_changed(&app, &mutation);
    Ok(())
}

#[tauri::command]
pub(crate) fn ipc_content_delete(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<DeleteUndoToken, String> {
    let prepared = {
        let mut conn = state.db.lock().unwrap();
        prepare_delete(&mut conn, &id, Utc::now(), DELETE_GRACE).map_err(ipc_error)?
    };
    ensure_delete_worker_running(&app);
    Ok(prepared)
}

#[tauri::command]
pub(crate) fn ipc_content_restore(
    state: tauri::State<AppState>,
    _app: tauri::AppHandle,
    token: String,
) -> Result<ContentSummary, String> {
    let mut conn = state.db.lock().unwrap();
    cancel_pending_delete(&mut conn, &token, Utc::now()).map_err(ipc_error)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::time::Duration as StdDuration;

    use chrono::{Duration, TimeZone, Utc};
    use rusqlite::{params, Connection};

    use super::*;
    use crate::content::catalog::current_revision;
    use crate::content::models::{ContentChange, ContentMutation, ContentOperation};
    use crate::content::projection::tests::fixture_with_all_kinds;

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap()
    }

    #[test]
    fn event_payload_preserves_revision_and_namespaced_changes_and_dispatch_is_best_effort() {
        let mutation = ContentMutation {
            value: (),
            revision: 14,
            changes: vec![ContentChange {
                id: "vault:credential-1".to_string(),
                operation: ContentOperation::Retention,
            }],
        };
        let event = content_changed_event(&mutation);
        assert_eq!(event.revision, 14);
        assert_eq!(event.changes, mutation.changes);

        let mut attempts = 0;
        dispatch_content_changed(&mutation, |name, payload| {
            attempts += 1;
            assert_eq!(name, "content-changed");
            assert_eq!(payload, event);
            Err::<(), _>("offline")
        });
        assert_eq!(attempts, 1);

        let empty = ContentMutation {
            value: (),
            revision: 14,
            changes: vec![],
        };
        dispatch_content_changed(&empty, |_, _| {
            attempts += 1;
            Ok::<(), ()>(())
        });
        assert_eq!(attempts, 1);
    }

    #[test]
    fn revision_handler_observes_other_backend_mutations() {
        let mut conn = fixture_with_all_kinds();
        assert_eq!(content_revision(&conn).unwrap().revision, 0);
        crate::content::service::save(&mut conn, "dock:text-1").unwrap();
        assert_eq!(content_revision(&conn).unwrap().revision, 1);
    }

    #[test]
    fn prepare_then_cancel_before_expiry_returns_original_summary_once() {
        let mut conn = fixture_with_all_kinds();
        let before = crate::content::catalog::summary_by_id(&conn, "dock:text-1", false).unwrap();
        let prepared =
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap();
        assert_eq!(current_revision(&conn).unwrap(), 0);
        let restored =
            cancel_pending_delete(&mut conn, &prepared.token, now() + Duration::seconds(9))
                .unwrap();
        assert_eq!(restored, before);
        assert_eq!(
            cancel_pending_delete(&mut conn, &prepared.token, now())
                .unwrap_err()
                .to_string(),
            "content_delete_undo_expired"
        );
    }

    #[test]
    fn file_backed_pending_delete_commits_only_at_expiry_and_survives_reopen() {
        let path = std::env::temp_dir().join(format!(
            "scratchpad-pending-{}.sqlite",
            rand::random::<u64>()
        ));
        let conn = fixture_with_all_kinds();
        conn.execute("VACUUM INTO ?1", params![path.to_string_lossy().as_ref()])
            .unwrap();
        drop(conn);

        let prepared = {
            let mut conn = Connection::open(&path).unwrap();
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap()
        };
        {
            let mut conn = Connection::open(&path).unwrap();
            assert!(commit_pending_delete(
                &mut conn,
                &prepared.token,
                now() + Duration::seconds(9)
            )
            .unwrap()
            .is_none());
            assert_eq!(current_revision(&conn).unwrap(), 0);
        }
        {
            let mut conn = Connection::open(&path).unwrap();
            let committed =
                commit_pending_delete(&mut conn, &prepared.token, now() + Duration::seconds(10))
                    .unwrap()
                    .unwrap();
            assert_eq!(committed.token, prepared.token);
            assert_eq!(committed.id, "dock:text-1");
            assert_eq!(committed.revision, 1);
            for table in [
                "entries",
                "content_catalog",
                "content_fts",
                "content_pending_deletes",
            ] {
                let sql = if table == "entries" {
                    format!("SELECT COUNT(*) FROM {table} WHERE id='text-1'")
                } else if table == "content_pending_deletes" {
                    format!("SELECT COUNT(*) FROM {table} WHERE token=?1")
                } else {
                    format!("SELECT COUNT(*) FROM {table} WHERE unified_id='dock:text-1'")
                };
                let count: i64 = if table == "content_pending_deletes" {
                    conn.query_row(&sql, params![prepared.token], |r| r.get(0))
                        .unwrap()
                } else {
                    conn.query_row(&sql, [], |r| r.get(0)).unwrap()
                };
                assert_eq!(count, 0, "{table}");
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn failed_commit_rolls_back_everything_marks_failed_and_recovery_only_clears_token() {
        let mut conn = fixture_with_all_kinds();
        let prepared =
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_content_delete BEFORE DELETE ON content_catalog BEGIN SELECT RAISE(ABORT, 'forced'); END;").unwrap();
        let mut failures = Vec::new();
        process_pending_delete(
            &mut conn,
            &prepared.token,
            now() + Duration::seconds(10),
            |_, _| Ok::<(), ()>(()),
            |name, payload| {
                failures.push((name.to_string(), payload));
                Ok::<(), ()>(())
            },
        )
        .unwrap();
        assert_eq!(current_revision(&conn).unwrap(), 0);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM entries WHERE id='text-1'", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_catalog WHERE unified_id='dock:text-1'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_fts WHERE unified_id='dock:text-1'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT status FROM content_pending_deletes WHERE token=?1",
                params![prepared.token],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "failed"
        );
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, "content-delete-failed");
        assert_eq!(failures[0].1.code, "content_delete_commit_failed");

        conn.execute_batch("DROP TRIGGER fail_content_delete")
            .unwrap();
        let records = recover_pending_deletes(&mut conn, now() + Duration::seconds(20)).unwrap();
        assert!(records.is_empty());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_pending_deletes", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM entries WHERE id='text-1'", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
    }

    #[test]
    fn pending_delete_rows_and_events_contain_only_safe_metadata() {
        let mut conn = fixture_with_all_kinds();
        let prepared = prepare_delete(
            &mut conn,
            "vault:credential-1",
            now(),
            Duration::seconds(10),
        )
        .unwrap();
        let row: (String, String, String, String, String) = conn.query_row("SELECT token, unified_id, created_at, expires_at, status FROM content_pending_deletes", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap();
        assert_eq!(row.0, prepared.token);
        assert_eq!(row.1, "vault:credential-1");
        let serialized = serde_json::to_string(&row).unwrap();
        for forbidden in ["password", "body", "file_path", "fields", "secret-value"] {
            assert!(!serialized.contains(forbidden));
        }
        assert!(prepared.token.len() >= 32);
        assert_eq!(StdDuration::from_secs(10).as_secs(), 10);
    }

    #[test]
    fn retention_and_reorder_mutations_reject_pending_content() {
        let mut conn = fixture_with_all_kinds();
        prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap();

        assert!(crate::content::service::save(&mut conn, "dock:text-1")
            .unwrap_err()
            .to_string()
            .contains("pending delete"));
        let ids = crate::content::catalog::catalog_ids_for_scope(
            &conn,
            crate::content::models::BrowseScope::Temporary,
        )
        .unwrap();
        assert!(crate::content::service::reorder(
            &mut conn,
            crate::content::models::BrowseScope::Temporary,
            &ids,
        )
        .unwrap_err()
        .to_string()
        .contains("pending delete"));
        assert_eq!(current_revision(&conn).unwrap(), 0);
    }

    #[test]
    fn unified_commands_and_startup_recovery_are_wired_into_the_tauri_builder() {
        let lib = include_str!("../lib.rs");
        for command in [
            "content::ipc::ipc_content_revision",
            "content::ipc::ipc_content_list",
            "content::ipc::ipc_content_detail",
            "content::ipc::ipc_content_search_local",
            "content::ipc::ipc_content_save",
            "content::ipc::ipc_content_unsave",
            "content::ipc::ipc_content_reorder",
            "content::ipc::ipc_content_delete",
            "content::ipc::ipc_content_restore",
        ] {
            assert!(
                lib.contains(command),
                "missing command registration: {command}"
            );
        }
        assert!(lib.contains("content::ipc::resume_pending_deletes(app.handle())"));
    }

    #[test]
    fn pending_delete_blocks_legacy_dock_and_vault_delete_paths_until_restore() {
        let mut conn = fixture_with_all_kinds();
        let dock = prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap();
        let vault = prepare_delete(
            &mut conn,
            "vault:credential-1",
            now(),
            Duration::seconds(10),
        )
        .unwrap();

        assert!(crate::content::service::delete(&mut conn, "dock:text-1").is_err());
        assert!(
            crate::vault::storage::delete_entry_with_revision(&mut conn, "credential-1").is_err()
        );
        assert_eq!(current_revision(&conn).unwrap(), 0);
        for (table, column, id) in [
            ("entries", "id", "text-1"),
            ("vault_entries", "id", "credential-1"),
            ("content_catalog", "unified_id", "dock:text-1"),
            ("content_catalog", "unified_id", "vault:credential-1"),
            ("content_fts", "unified_id", "dock:text-1"),
            ("content_fts", "unified_id", "vault:credential-1"),
        ] {
            let count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {column}=?1"),
                    params![id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{table}:{id}");
        }
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_pending_deletes", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            2
        );
        assert_eq!(
            cancel_pending_delete(&mut conn, &dock.token, now())
                .unwrap()
                .id,
            "dock:text-1"
        );
        assert_eq!(
            cancel_pending_delete(&mut conn, &vault.token, now())
                .unwrap()
                .id,
            "vault:credential-1"
        );
    }

    #[test]
    fn prepare_replaces_failed_token_and_recovery_quarantines_only_corrupt_rows() {
        let mut conn = fixture_with_all_kinds();
        let failed =
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap();
        conn.execute(
            "UPDATE content_pending_deletes SET status='failed' WHERE token=?1",
            params![failed.token],
        )
        .unwrap();
        let replacement =
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap();
        assert_ne!(replacement.token, failed.token);

        let valid =
            prepare_delete(&mut conn, "dock:image-1", now(), Duration::seconds(10)).unwrap();
        conn.execute(
            "UPDATE content_pending_deletes SET expires_at='not-rfc3339' WHERE token=?1",
            params![replacement.token],
        )
        .unwrap();
        let records = recover_pending_deletes(&mut conn, now()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].token, valid.token);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_pending_deletes WHERE token=?1",
                params![replacement.token],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM entries WHERE id='text-1'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn worker_gate_claims_once_and_rechecks_when_work_arrives_during_exit() {
        let gate = DeleteWorkerGate::default();
        assert!(gate.claim());
        for _ in 0..1_000 {
            assert!(!gate.claim());
        }
        assert!(gate.release_and_reclaim(true));
        assert!(!gate.claim());
        assert!(!gate.release_and_reclaim(false));
        assert!(gate.claim());
    }

    #[test]
    fn worker_step_reschedules_after_early_wall_clock_wake_then_deletes_once() {
        let mut conn = fixture_with_all_kinds();
        let prepared =
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap();
        assert!(matches!(
            pending_worker_step(&mut conn, &prepared.token, now() + Duration::seconds(9)).unwrap(),
            PendingWorkerStep::NotDue(_)
        ));
        let PendingWorkerStep::Deleted(committed) =
            pending_worker_step(&mut conn, &prepared.token, now() + Duration::seconds(10)).unwrap()
        else {
            panic!("delete must become due")
        };
        assert_eq!(committed.revision, 1);
        assert!(matches!(
            pending_worker_step(&mut conn, &prepared.token, now() + Duration::seconds(20)).unwrap(),
            PendingWorkerStep::Gone
        ));
        assert_eq!(current_revision(&conn).unwrap(), 1);
    }

    #[test]
    fn file_backed_concurrent_prepare_returns_one_stable_token() {
        let path = std::env::temp_dir().join(format!(
            "scratchpad-prepare-race-{}.sqlite",
            rand::random::<u64>()
        ));
        let conn = fixture_with_all_kinds();
        conn.execute("VACUUM INTO ?1", params![path.to_string_lossy().as_ref()])
            .unwrap();
        drop(conn);
        let barrier = Arc::new(Barrier::new(2));
        let connections = (0..2)
            .map(|_| {
                let conn = Connection::open(&path).unwrap();
                crate::storage::connection::configure_connection(&conn).unwrap();
                conn
            })
            .collect::<Vec<_>>();
        let handles = connections
            .into_iter()
            .map(|mut conn| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10))
                        .unwrap()
                        .token
                })
            })
            .collect::<Vec<_>>();
        let tokens = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(tokens[0], tokens[1]);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_pending_deletes", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
        drop(conn);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn file_backed_restore_racing_due_commit_has_exactly_one_winner() {
        let path = std::env::temp_dir().join(format!(
            "scratchpad-restore-race-{}.sqlite",
            rand::random::<u64>()
        ));
        let conn = fixture_with_all_kinds();
        conn.execute("VACUUM INTO ?1", params![path.to_string_lossy().as_ref()])
            .unwrap();
        drop(conn);
        let prepared = {
            let mut conn = Connection::open(&path).unwrap();
            crate::storage::connection::configure_connection(&conn).unwrap();
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap()
        };
        let barrier = Arc::new(Barrier::new(2));
        let cancel = {
            let path = path.clone();
            let barrier = barrier.clone();
            let token = prepared.token.clone();
            std::thread::spawn(move || {
                let mut conn = Connection::open(path).unwrap();
                crate::storage::connection::configure_connection(&conn).unwrap();
                barrier.wait();
                cancel_pending_delete(&mut conn, &token, now() + Duration::seconds(9)).is_ok()
            })
        };
        let commit = {
            let path = path.clone();
            let barrier = barrier.clone();
            let token = prepared.token.clone();
            std::thread::spawn(move || {
                let mut conn = Connection::open(path).unwrap();
                crate::storage::connection::configure_connection(&conn).unwrap();
                barrier.wait();
                commit_pending_delete(&mut conn, &token, now() + Duration::seconds(10))
                    .unwrap()
                    .is_some()
            })
        };
        let cancelled = cancel.join().unwrap();
        let committed = commit.join().unwrap();
        assert_ne!(cancelled, committed);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_pending_deletes", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(current_revision(&conn).unwrap(), i64::from(committed));
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM entries WHERE id='text-1'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            i64::from(cancelled)
        );
        drop(conn);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn file_backed_due_commit_waits_for_short_writer_lock_without_marking_failed() {
        let path = std::env::temp_dir().join(format!(
            "scratchpad-busy-retry-{}.sqlite",
            rand::random::<u64>()
        ));
        let conn = fixture_with_all_kinds();
        conn.execute("VACUUM INTO ?1", params![path.to_string_lossy().as_ref()])
            .unwrap();
        drop(conn);
        let prepared = {
            let mut conn = Connection::open(&path).unwrap();
            crate::storage::connection::configure_connection(&conn).unwrap();
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap()
        };
        let mut blocker = Connection::open(&path).unwrap();
        crate::storage::connection::configure_connection(&blocker).unwrap();
        let tx = blocker
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let worker = {
            let path = path.clone();
            let token = prepared.token.clone();
            std::thread::spawn(move || {
                let mut conn = Connection::open(path).unwrap();
                crate::storage::connection::configure_connection(&conn).unwrap();
                commit_pending_delete(&mut conn, &token, now() + Duration::seconds(10)).unwrap()
            })
        };
        std::thread::sleep(StdDuration::from_millis(100));
        tx.commit().unwrap();
        assert_eq!(worker.join().unwrap().unwrap().revision, 1);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_pending_deletes WHERE status='failed'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(conn);
        drop(blocker);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sqlite_busy_is_retryable_and_never_turns_pending_token_failed() {
        let path = std::env::temp_dir().join(format!(
            "scratchpad-busy-classify-{}.sqlite",
            rand::random::<u64>()
        ));
        let conn = fixture_with_all_kinds();
        conn.execute("VACUUM INTO ?1", params![path.to_string_lossy().as_ref()])
            .unwrap();
        drop(conn);
        let prepared = {
            let mut conn = Connection::open(&path).unwrap();
            crate::storage::connection::configure_connection(&conn).unwrap();
            prepare_delete(&mut conn, "dock:text-1", now(), Duration::seconds(10)).unwrap()
        };
        let mut blocker = Connection::open(&path).unwrap();
        crate::storage::connection::configure_connection(&blocker).unwrap();
        let tx = blocker
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let mut contender = Connection::open(&path).unwrap();
        contender.busy_timeout(StdDuration::ZERO).unwrap();
        let error = mark_delete_failed(&mut contender, &prepared.token).unwrap_err();
        assert!(is_retryable_busy(&error));
        tx.commit().unwrap();
        drop(blocker);
        assert_eq!(
            contender
                .query_row(
                    "SELECT status FROM content_pending_deletes WHERE token=?1",
                    params![prepared.token],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "pending"
        );
        drop(contender);
        std::fs::remove_file(path).unwrap();
    }
}
