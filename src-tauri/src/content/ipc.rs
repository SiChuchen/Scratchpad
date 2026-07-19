use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, MutexGuard};
use std::time::Duration as StdDuration;
use std::time::Instant;
use tauri::{Emitter, Manager};

use crate::content::catalog::{bump_revision, current_revision, summary_by_id};
use crate::content::models::{
    BrowseScope, ContentChange, ContentChangedEvent, ContentDeleteFailedEvent, ContentDetail,
    ContentKind, ContentMutation, ContentOperation, ContentRevision, ContentSearchHit,
    ContentSummary, DeleteUndoToken, MainContentOpen, PlannedUnifiedSearch, UnifiedContentId,
    UnifiedQueryPlan,
};
use crate::storage::error::{StorageError, StorageResult};
use crate::AppState;

const DELETE_GRACE: Duration = Duration::seconds(10);
const DEFAULT_SEARCH_LIMIT: usize = 50;
const UNDO_EXPIRED: &str = "content_delete_undo_expired";
const DELETE_WORKER_MAX_SLEEP: StdDuration = StdDuration::from_secs(60 * 60);

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

pub(crate) fn emit_content_changed<T, R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    mutation: &ContentMutation<T>,
) {
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

struct DeleteSchedulerInner {
    gate: DeleteWorkerGate,
    notify: tokio::sync::Notify,
}

impl Default for DeleteSchedulerInner {
    fn default() -> Self {
        Self {
            gate: DeleteWorkerGate::new(),
            notify: tokio::sync::Notify::new(),
        }
    }
}

#[derive(Clone, Default)]
pub struct DeleteSchedulerState {
    inner: Arc<DeleteSchedulerInner>,
}

struct DeleteWorkerLease {
    scheduler: Arc<DeleteSchedulerInner>,
    armed: bool,
}

impl Drop for DeleteWorkerLease {
    fn drop(&mut self) {
        if self.armed {
            self.scheduler.gate.running.store(false, Ordering::Release);
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
    if expires_at <= now {
        return StdDuration::ZERO;
    }
    match (expires_at - now).to_std() {
        Ok(delay) => delay.min(DELETE_WORKER_MAX_SLEEP),
        Err(_) => DELETE_WORKER_MAX_SLEEP,
    }
}

enum MarkFailedOutcome {
    Marked,
    Gone,
    Deferred,
}

async fn retry_mark_failed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pending: &PendingDelete,
) -> MarkFailedOutcome {
    let mut backoff = StdDuration::from_millis(25);
    for attempt in 0..3 {
        let result = {
            let state = app.state::<AppState>();
            let mut conn = lock_app_db(&state);
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
                return MarkFailedOutcome::Marked;
            }
            Ok(false) => return MarkFailedOutcome::Gone,
            Err(error) if is_retryable_busy(&error) => {
                if attempt == 2 {
                    return MarkFailedOutcome::Deferred;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(StdDuration::from_secs(1));
            }
            Err(error) => {
                eprintln!(
                    "failed to mark content delete token {} failed: {error}",
                    pending.token
                );
                return MarkFailedOutcome::Deferred;
            }
        }
    }
    MarkFailedOutcome::Deferred
}

fn lock_app_db(state: &AppState) -> MutexGuard<'_, Connection> {
    state.db.lock().unwrap_or_else(|error| error.into_inner())
}

async fn run_delete_worker<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    scheduler: Arc<DeleteSchedulerInner>,
) {
    let mut busy_backoff = StdDuration::from_millis(25);
    let mut token_backoff = HashMap::<String, (Instant, u32)>::new();
    loop {
        let records = {
            let state = app.state::<AppState>();
            let conn = lock_app_db(&state);
            list_pending_deletes(&conn)
        };
        let records = match records {
            Ok(records) if records.is_empty() => {
                scheduler.gate.running.store(false, Ordering::Release);
                let has_work = {
                    let state = app.state::<AppState>();
                    let conn = lock_app_db(&state);
                    list_pending_deletes(&conn).is_ok_and(|records| !records.is_empty())
                };
                if has_work && scheduler.gate.claim() {
                    continue;
                }
                return;
            }
            Ok(records) => records,
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
        token_backoff.retain(|token, _| records.iter().any(|record| &record.token == token));
        let instant_now = Instant::now();
        let plan = plan_pending_work(&records, &token_backoff, instant_now, Utc::now());
        let Some(pending) = plan.eligible else {
            tokio::select! {
                () = tokio::time::sleep(plan.wake_after) => {}
                () = scheduler.notify.notified() => {}
            }
            continue;
        };
        if !plan.wake_after.is_zero() {
            tokio::select! {
                () = tokio::time::sleep(plan.wake_after) => {}
                () = scheduler.notify.notified() => {}
            }
            continue;
        }
        let step = {
            let state = app.state::<AppState>();
            let mut conn = lock_app_db(&state);
            pending_worker_step(&mut conn, &pending.token, Utc::now())
        };
        match step {
            Ok(PendingWorkerStep::Gone) => {
                token_backoff.remove(&pending.token);
                busy_backoff = StdDuration::from_millis(25);
            }
            Ok(PendingWorkerStep::NotDue(expires_at)) => {
                let _next_delay = worker_delay(expires_at, Utc::now());
                busy_backoff = StdDuration::from_millis(25);
            }
            Ok(PendingWorkerStep::Deleted(committed)) => {
                token_backoff.remove(&pending.token);
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
                match retry_mark_failed(&app, &pending).await {
                    MarkFailedOutcome::Marked | MarkFailedOutcome::Gone => {
                        token_backoff.remove(&pending.token);
                    }
                    MarkFailedOutcome::Deferred => {
                        let failures = token_backoff
                            .get(&pending.token)
                            .map_or(1, |(_, failures)| failures.saturating_add(1));
                        let exponent = failures.saturating_sub(1).min(7);
                        let delay = StdDuration::from_millis(250 * (1_u64 << exponent));
                        token_backoff.insert(
                            pending.token.clone(),
                            (
                                Instant::now() + delay.min(StdDuration::from_secs(30)),
                                failures,
                            ),
                        );
                    }
                }
            }
        }
    }
}

struct PendingWorkPlan {
    eligible: Option<PendingDelete>,
    wake_after: StdDuration,
}

fn plan_pending_work(
    records: &[PendingDelete],
    token_backoff: &HashMap<String, (Instant, u32)>,
    instant_now: Instant,
    wall_now: DateTime<Utc>,
) -> PendingWorkPlan {
    let eligible = records
        .iter()
        .find(|record| {
            token_backoff
                .get(&record.token)
                .is_none_or(|(until, _)| *until <= instant_now)
        })
        .cloned();
    let deferred_wake = records
        .iter()
        .filter_map(|record| token_backoff.get(&record.token))
        .filter_map(|(until, _)| (*until > instant_now).then_some(*until - instant_now))
        .min();
    let selected_wake = eligible
        .as_ref()
        .map(|record| worker_delay(record.expires_at, wall_now));
    let wake_after = match (selected_wake, deferred_wake) {
        (Some(selected), Some(deferred)) => selected.min(deferred),
        (Some(selected), None) => selected,
        (None, Some(deferred)) => deferred,
        (None, None) => DELETE_WORKER_MAX_SLEEP,
    }
    .min(DELETE_WORKER_MAX_SLEEP);
    PendingWorkPlan {
        eligible,
        wake_after,
    }
}

fn list_pending_deletes(conn: &Connection) -> StorageResult<Vec<PendingDelete>> {
    let mut statement = conn.prepare(
        "SELECT token, unified_id, created_at, expires_at
         FROM content_pending_deletes
         WHERE status='pending'
         ORDER BY expires_at, token",
    )?;
    let rows = statement
        .query_map([], pending_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
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
            Err(error) => eprintln!("ignoring invalid pending delete token {token}: {error}"),
        }
    }
    Ok(pending)
}

#[derive(Default)]
struct PanicRetryPolicy {
    consecutive: u32,
}

impl PanicRetryPolicy {
    fn record_panic(&mut self) -> Option<StdDuration> {
        if self.consecutive >= 3 {
            return None;
        }
        let delay = StdDuration::from_millis(25 * (1_u64 << self.consecutive));
        self.consecutive += 1;
        Some(delay)
    }
}

#[cfg(test)]
async fn run_injected_monitor<F, Fut>(mut worker: F) -> usize
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let mut policy = PanicRetryPolicy::default();
    let mut starts = 0;
    loop {
        starts += 1;
        match tauri::async_runtime::spawn(worker()).await {
            Ok(()) => return starts,
            Err(tauri::Error::JoinError(error)) if error.is_panic() => {
                let Some(delay) = policy.record_panic() else {
                    return starts;
                };
                tokio::time::sleep(delay).await;
            }
            Err(_) => return starts,
        }
    }
}

fn ensure_delete_worker_running<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let scheduler = app.state::<DeleteSchedulerState>().inner.clone();
    scheduler.notify.notify_one();
    if !scheduler.gate.claim() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut panic_policy = PanicRetryPolicy::default();
        loop {
            let worker_app = app.clone();
            let worker_scheduler = scheduler.clone();
            let lease_scheduler = scheduler.clone();
            let handle = tauri::async_runtime::spawn(async move {
                let mut lease = DeleteWorkerLease {
                    scheduler: lease_scheduler,
                    armed: true,
                };
                run_delete_worker(worker_app, worker_scheduler).await;
                lease.armed = false;
            });
            match handle.await {
                Ok(()) => return,
                Err(tauri::Error::JoinError(error)) if error.is_panic() => {
                    let Some(delay) = panic_policy.record_panic() else {
                        eprintln!("content delete worker stopped after repeated panic: {error}");
                        return;
                    };
                    tokio::time::sleep(delay).await;
                    let has_work = {
                        let state = app.state::<AppState>();
                        let conn = lock_app_db(&state);
                        list_pending_deletes(&conn).is_ok_and(|records| !records.is_empty())
                    };
                    if !has_work || !scheduler.gate.claim() {
                        return;
                    }
                }
                Err(error) => {
                    eprintln!("content delete worker stopped after repeated panic: {error}");
                    return;
                }
            }
        }
    });
}

pub(crate) fn resume_pending_deletes<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let has_pending = {
        let state = app.state::<AppState>();
        let mut conn = lock_app_db(&state);
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

pub(crate) fn adapt_vault_plan(
    planned: crate::vault::models::PlannedSearch,
) -> PlannedUnifiedSearch {
    PlannedUnifiedSearch {
        plan: UnifiedQueryPlan {
            kinds: Vec::new(),
            keywords: planned.plan.keywords,
            aliases: planned.plan.aliases,
            date_from: planned.plan.date_from,
            date_to: planned.plan.date_to,
        },
        understood_terms: planned.understood_terms,
        audit: planned.audit,
    }
}

#[tauri::command]
pub(crate) async fn ipc_content_plan_search(
    app: tauri::AppHandle,
    query: String,
    request_id: String,
) -> Result<PlannedUnifiedSearch, String> {
    let planned = crate::vault::ipc::search::plan_search_redacted(&app, query, request_id).await?;
    Ok(adapt_vault_plan(planned))
}

#[tauri::command]
pub(crate) fn ipc_content_cancel_search(
    app: tauri::AppHandle,
    request_id: String,
) -> Result<(), String> {
    crate::vault::ipc::search::cancel_search(&app.state(), &request_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn ipc_open_main_content(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let payload = MainContentOpen::new(&id)?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    app.emit("main-open-content", payload)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn ipc_content_update_text<R: tauri::Runtime>(
    state: tauri::State<AppState>,
    app: tauri::AppHandle<R>,
    id: String,
    title: Option<String>,
    body: String,
) -> Result<ContentDetail, String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|error| error.to_string())?;
        crate::content::service::update_text(&mut conn, &id, title.as_deref(), &body)
            .map_err(ipc_error)?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
}

#[tauri::command]
pub(crate) fn ipc_content_rename<R: tauri::Runtime>(
    state: tauri::State<AppState>,
    app: tauri::AppHandle<R>,
    id: String,
    title: Option<String>,
) -> Result<ContentDetail, String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|error| error.to_string())?;
        crate::content::service::rename(&mut conn, &id, title.as_deref()).map_err(ipc_error)?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
}

#[tauri::command]
pub(crate) fn ipc_content_update_structured<R: tauri::Runtime>(
    state: tauri::State<AppState>,
    app: tauri::AppHandle<R>,
    id: String,
    input: crate::vault::models::VaultEntryInput,
) -> Result<ContentDetail, String> {
    let mutation = {
        let mut conn = state.db.lock().map_err(|error| error.to_string())?;
        crate::content::service::update_structured(&mut conn, &id, &input).map_err(ipc_error)?
    };
    emit_content_changed(&app, &mutation);
    Ok(mutation.value)
}

#[tauri::command]
pub(crate) fn ipc_content_save<R: tauri::Runtime>(
    state: tauri::State<AppState>,
    app: tauri::AppHandle<R>,
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
pub(crate) fn ipc_content_unsave<R: tauri::Runtime>(
    state: tauri::State<AppState>,
    app: tauri::AppHandle<R>,
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
pub(crate) fn ipc_content_reorder<R: tauri::Runtime>(
    state: tauri::State<AppState>,
    app: tauri::AppHandle<R>,
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
pub(crate) fn ipc_content_delete<R: tauri::Runtime>(
    state: tauri::State<AppState>,
    app: tauri::AppHandle<R>,
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
pub(crate) fn ipc_content_restore<R: tauri::Runtime>(
    state: tauri::State<AppState>,
    app: tauri::AppHandle<R>,
    token: String,
) -> Result<ContentSummary, String> {
    let restored = {
        let mut conn = state.db.lock().unwrap();
        cancel_pending_delete(&mut conn, &token, Utc::now()).map_err(ipc_error)?
    };
    ensure_delete_worker_running(&app);
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::time::Duration as StdDuration;

    use chrono::{Duration, TimeZone, Utc};
    use rusqlite::{params, Connection};
    use tauri::Listener;

    use super::*;
    use crate::vault::models::{AiQueryPlan, AiRequestAudit, EntryKind, PlannedSearch};

    #[test]
    fn unified_plan_uses_terms_and_dates_without_narrowing_legacy_kinds() {
        let legacy = PlannedSearch {
            plan: AiQueryPlan {
                kinds: vec![EntryKind::Note],
                keywords: vec!["部署".into()],
                aliases: vec!["release".into()],
                date_from: Some("2026-07-01".into()),
                date_to: None,
            },
            understood_terms: vec!["部署".into(), "release".into()],
            audit: AiRequestAudit {
                provider_id: "test".into(),
                model: "test-model".into(),
                sent_at: "2026-07-18T00:00:00Z".into(),
                messages: Vec::new(),
            },
        };

        let unified = adapt_vault_plan(legacy);
        assert!(unified.plan.kinds.is_empty());
        assert_eq!(unified.plan.keywords, vec!["部署"]);
        assert_eq!(unified.plan.aliases, vec!["release"]);
        assert_eq!(unified.plan.date_from.as_deref(), Some("2026-07-01"));
        assert_eq!(unified.understood_terms, vec!["部署", "release"]);
        assert_eq!(unified.audit.provider_id, "test");
    }

    #[test]
    fn main_content_open_payload_requires_a_valid_unified_id() {
        assert!(MainContentOpen::new("dock:de-1").is_ok());
        assert!(MainContentOpen::new("vault:ve-1").is_ok());
        assert!(MainContentOpen::new("ve-1").is_err());
    }
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
        let created = crate::vault::storage::create_entry_with_revision(
            &mut conn,
            &crate::vault::models::VaultEntryInput {
                kind: crate::vault::models::EntryKind::Credential,
                title: "Production console".into(),
                fields: vec![crate::vault::models::FieldInput {
                    key: "username".into(),
                    value: "alice".into(),
                    is_sensitive: false,
                }],
                notes: Some("initial notes".into()),
                manual_tags: vec!["Initial".into()],
            },
        )
        .unwrap();
        let entry_id = created.value.entry.id;
        let input = crate::vault::models::VaultEntryInput {
            kind: crate::vault::models::EntryKind::Credential,
            title: "NEVERINDEXMEPASSWORD console".into(),
            fields: vec![
                crate::vault::models::FieldInput {
                    key: "username".into(),
                    value: "alice".into(),
                    is_sensitive: false,
                },
                crate::vault::models::FieldInput {
                    key: " PaSsWoRd ".into(),
                    value: "NeverIndexMePassword".into(),
                    is_sensitive: false,
                },
                crate::vault::models::FieldInput {
                    key: " ToKeN ".into(),
                    value: "NeverIndexMeToken".into(),
                    is_sensitive: false,
                },
            ],
            notes: Some("lowercase neverindexmetoken notes".into()),
            manual_tags: vec!["Access".into(), "NEVERINDEXMETOKEN".into()],
        };
        crate::vault::storage::update_entry_with_revision(&mut conn, &entry_id, &input).unwrap();
        crate::vault::storage::replace_ai_tags_with_revision(
            &mut conn,
            &entry_id,
            &["Useful AI".into(), "neverindexmepassword-ai".into()],
        )
        .unwrap();
        let pending = crate::vault::storage::get_ai_metadata(&conn, &entry_id)
            .unwrap()
            .unwrap();
        crate::vault::storage::set_ai_metadata_with_revision(
            &mut conn,
            &crate::vault::models::VaultAiMetadata {
                entry_id: entry_id.clone(),
                summary: Some("metadata NEVERINDEXMEPASSWORD".into()),
                search_aliases: vec!["neverindexmetoken alias".into()],
                content_hash: pending.content_hash,
                provider_id: Some("validation-provider".into()),
                model: Some("validation-model".into()),
                generated_at: Some("2026-07-18T12:00:00+00:00".into()),
                status: crate::vault::models::AiMetadataStatus::Ready,
            },
        )
        .unwrap();
        for table in ["vault_fts", "content_fts"] {
            let searchable: String = if table == "vault_fts" {
                conn.query_row(
                    "SELECT title || ' ' || notes || ' ' || searchable
                     FROM vault_fts WHERE entry_id=?1",
                    params![entry_id],
                    |row| row.get(0),
                )
                .unwrap()
            } else {
                conn.query_row(
                    "SELECT title || ' ' || body || ' ' || tags || ' ' || aliases
                     FROM content_fts WHERE unified_id=?1",
                    params![format!("vault:{entry_id}")],
                    |row| row.get(0),
                )
                .unwrap()
            };
            assert!(searchable.contains("alice"), "{table} lost useful username");
            assert!(searchable.contains("Access"), "{table} lost useful tag");
            assert!(
                searchable.contains("Useful AI"),
                "{table} lost useful AI tag"
            );
            assert!(!searchable.to_lowercase().contains("neverindexme"));
        }
        let username_hits = crate::content::service::search(&conn, "alice", None, 10).unwrap();
        assert!(username_hits
            .iter()
            .any(|hit| hit.summary.id == format!("vault:{entry_id}")));
        for secret_query in ["NeverIndexMePassword", "neverindexmetoken"] {
            assert!(
                crate::content::service::search(&conn, secret_query, None, 10)
                    .unwrap()
                    .is_empty()
            );
        }
        let prepared = prepare_delete(
            &mut conn,
            &format!("vault:{entry_id}"),
            now(),
            Duration::seconds(10),
        )
        .unwrap();
        let row: (String, String, String, String, String) = conn.query_row("SELECT token, unified_id, created_at, expires_at, status FROM content_pending_deletes", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap();
        assert_eq!(row.0, prepared.token);
        assert_eq!(row.1, format!("vault:{entry_id}"));
        let pending_columns = conn
            .prepare("SELECT name FROM pragma_table_info('content_pending_deletes') ORDER BY cid")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            pending_columns,
            ["token", "unified_id", "created_at", "expires_at", "status"]
        );
        let serialized = serde_json::to_string(&row).unwrap();
        for forbidden in ["password", "body", "file_path", "fields", "secret-value"] {
            assert!(!serialized.contains(forbidden));
        }
        for secret in ["NeverIndexMePassword", "NeverIndexMeToken"] {
            assert_eq!(
                conn.query_row(
                    "SELECT
                        (SELECT COUNT(*) FROM vault_fts
                         WHERE title LIKE ?1 OR notes LIKE ?1 OR searchable LIKE ?1) +
                        (SELECT COUNT(*) FROM content_fts
                         WHERE title LIKE ?1 OR body LIKE ?1 OR tags LIKE ?1 OR aliases LIKE ?1)",
                    params![format!("%{secret}%")],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                0,
                "{secret} reached a search projection"
            );
        }

        let changed_json = serde_json::to_value(content_changed_event(&ContentMutation {
            value: (),
            revision: 12,
            changes: vec![ContentChange {
                id: format!("vault:{entry_id}"),
                operation: ContentOperation::Retention,
            }],
        }))
        .unwrap();
        let undo_json = serde_json::to_value(&prepared).unwrap();
        let failure_log_json = serde_json::to_value(ContentDeleteFailedEvent {
            token: prepared.token.clone(),
            id: format!("vault:{entry_id}"),
            code: "content_delete_commit_failed".into(),
        })
        .unwrap();
        assert_eq!(
            changed_json.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["changes", "revision"]
        );
        assert_eq!(
            changed_json["changes"][0]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["id", "operation"]
        );
        assert_eq!(
            undo_json.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["expiresAt", "token"]
        );
        assert_eq!(
            failure_log_json
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["code", "id", "token"]
        );
        let boundary_json = format!("{changed_json}{undo_json}{failure_log_json}");
        for forbidden in [
            "NeverIndexMePassword",
            "NeverIndexMeToken",
            " PaSsWoRd ",
            " ToKeN ",
        ] {
            assert!(!boundary_json.contains(forbidden));
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
            "content::ipc::ipc_content_plan_search",
            "content::ipc::ipc_content_cancel_search",
            "content::ipc::ipc_open_main_content",
            "content::ipc::ipc_content_update_text",
            "content::ipc::ipc_content_rename",
            "content::ipc::ipc_content_update_structured",
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

    #[test]
    fn monitor_restarts_once_after_injected_panic_and_caps_repeated_panics() {
        let mut conn = fixture_with_all_kinds();
        let prepared = prepare_delete(&mut conn, "dock:text-1", now(), Duration::zero()).unwrap();
        let db = Arc::new(std::sync::Mutex::new(conn));
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worker_db = db.clone();
        let worker_attempts = attempts.clone();
        let token = prepared.token.clone();
        let starts = tauri::async_runtime::block_on(run_injected_monitor(move || {
            let db = worker_db.clone();
            let token = token.clone();
            let attempt = worker_attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    panic!("injected worker panic");
                }
                let mut conn = db.lock().unwrap();
                assert!(matches!(
                    pending_worker_step(&mut conn, &token, now()).unwrap(),
                    PendingWorkerStep::Deleted(_)
                ));
            }
        }));
        assert_eq!(starts, 2);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let conn = db.lock().unwrap();
        assert_eq!(current_revision(&conn).unwrap(), 1);
        assert!(pending_by_token(&conn, &prepared.token).unwrap().is_none());
        drop(conn);

        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let starts = tauri::async_runtime::block_on(run_injected_monitor(|| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { panic!("injected repeated worker panic") }
        }));
        assert_eq!(starts, 4);
        assert_eq!(attempts.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn token_backoff_skips_failed_token_and_processes_other_due_delete() {
        let mut conn = fixture_with_all_kinds();
        let failed = prepare_delete(&mut conn, "dock:text-1", now(), Duration::zero()).unwrap();
        let healthy = prepare_delete(&mut conn, "dock:image-1", now(), Duration::zero()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_token_delete BEFORE DELETE ON entries
             WHEN OLD.id='text-1' BEGIN SELECT RAISE(ABORT, 'fail-token-delete'); END;
             CREATE TRIGGER fail_token_mark BEFORE UPDATE OF status ON content_pending_deletes
             WHEN OLD.unified_id='dock:text-1'
             BEGIN SELECT RAISE(ABORT, 'fail-token-mark'); END;",
        )
        .unwrap();

        assert!(pending_worker_step(&mut conn, &failed.token, now()).is_err());
        assert!(mark_delete_failed(&mut conn, &failed.token).is_err());
        let selection_time = Instant::now();
        let mut backoff = HashMap::new();
        backoff.insert(
            failed.token.clone(),
            (selection_time + StdDuration::from_secs(1), 1),
        );
        let records = list_pending_deletes(&conn).unwrap();
        let selected = plan_pending_work(&records, &backoff, selection_time, now())
            .eligible
            .unwrap();
        assert_eq!(selected.token, healthy.token);
        assert!(matches!(
            pending_worker_step(&mut conn, &selected.token, now()).unwrap(),
            PendingWorkerStep::Deleted(_)
        ));
        assert_eq!(current_revision(&conn).unwrap(), 1);

        let records = list_pending_deletes(&conn).unwrap();
        for _ in 0..1_000 {
            assert!(plan_pending_work(&records, &backoff, selection_time, now())
                .eligible
                .is_none());
        }
        conn.execute_batch("DROP TRIGGER fail_token_delete; DROP TRIGGER fail_token_mark;")
            .unwrap();
        let selected = plan_pending_work(
            &records,
            &backoff,
            selection_time + StdDuration::from_secs(2),
            now(),
        )
        .eligible
        .unwrap();
        assert_eq!(selected.token, failed.token);
        assert!(matches!(
            pending_worker_step(&mut conn, &selected.token, now()).unwrap(),
            PendingWorkerStep::Deleted(_)
        ));
        assert_eq!(current_revision(&conn).unwrap(), 2);
    }

    #[test]
    fn deferred_due_token_sets_wake_before_eligible_future_token() {
        let wall_now = now();
        let instant_now = Instant::now();
        let deferred = PendingDelete {
            token: "deferred".to_string(),
            id: "dock:text-deferred".to_string(),
            created_at: wall_now - Duration::seconds(1),
            expires_at: wall_now,
        };
        let future = PendingDelete {
            token: "future".to_string(),
            id: "dock:text-future".to_string(),
            created_at: wall_now,
            expires_at: wall_now + Duration::hours(1),
        };
        let records = vec![deferred.clone(), future.clone()];
        let mut backoff = HashMap::new();
        backoff.insert(
            deferred.token.clone(),
            (instant_now + StdDuration::from_millis(250), 1),
        );

        let plan = plan_pending_work(&records, &backoff, instant_now, wall_now);
        assert_eq!(plan.eligible.unwrap().token, future.token);
        assert_eq!(plan.wake_after, StdDuration::from_millis(250));

        let after_backoff = instant_now + StdDuration::from_millis(250);
        let plan = plan_pending_work(&records, &backoff, after_backoff, wall_now);
        assert_eq!(plan.eligible.unwrap().token, deferred.token);
        assert_eq!(plan.wake_after, StdDuration::ZERO);

        let later = PendingDelete {
            token: "later".to_string(),
            id: "dock:text-later".to_string(),
            created_at: wall_now,
            expires_at: wall_now + Duration::hours(2),
        };
        let records = vec![deferred.clone(), later.clone()];
        backoff.insert(
            deferred.token.clone(),
            (instant_now + StdDuration::from_millis(500), 2),
        );
        backoff.insert(
            later.token.clone(),
            (instant_now + StdDuration::from_millis(100), 1),
        );
        backoff.insert(
            "already-removed".to_string(),
            (instant_now + StdDuration::from_millis(10), 1),
        );
        let plan = plan_pending_work(&records, &backoff, instant_now, wall_now);
        assert!(plan.eligible.is_none());
        assert_eq!(plan.wake_after, StdDuration::from_millis(100));

        backoff.clear();
        let plan = plan_pending_work(
            &[later],
            &backoff,
            instant_now,
            wall_now - Duration::hours(2),
        );
        assert_eq!(plan.wake_after, DELETE_WORKER_MAX_SLEEP);
    }

    #[test]
    fn pending_scan_parses_many_rows_and_isolates_bad_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE content_pending_deletes(
                token TEXT PRIMARY KEY,
                unified_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                status TEXT NOT NULL
             );",
        )
        .unwrap();
        let created_at = now().to_rfc3339();
        for index in 0..100 {
            conn.execute(
                "INSERT INTO content_pending_deletes VALUES (?1, ?2, ?3, ?4, 'pending')",
                params![
                    format!("token-{index:03}"),
                    format!("dock:text-{index:03}"),
                    created_at,
                    (now() + Duration::seconds(index)).to_rfc3339(),
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO content_pending_deletes VALUES ('bad', 'dock:text-bad', ?1, 'not-rfc3339', 'pending')",
            params![created_at],
        )
        .unwrap();

        let records = list_pending_deletes(&conn).unwrap();
        assert_eq!(records.len(), 100);
        assert_eq!(records.first().unwrap().token, "token-000");
        assert_eq!(records.last().unwrap().token, "token-099");
    }

    #[test]
    fn scheduler_state_is_independent_per_app_instance() {
        let first = DeleteSchedulerState::default();
        let second = DeleteSchedulerState::default();
        assert!(first.inner.gate.claim());
        assert!(!first.inner.gate.claim());
        assert!(second.inner.gate.claim());
        assert!(!second.inner.gate.claim());
    }

    fn invoke_mock(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        cmd: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|body| body.deserialize().unwrap())
    }

    #[test]
    fn mock_tauri_dispatches_unified_commands_shapes_errors_state_and_events() {
        let path = std::env::temp_dir().join(format!(
            "scratchpad-tauri-ipc-{}.sqlite",
            rand::random::<u64>()
        ));
        let fixture = fixture_with_all_kinds();
        fixture
            .execute("VACUUM INTO ?1", params![path.to_string_lossy().as_ref()])
            .unwrap();
        drop(fixture);
        let conn = Connection::open(&path).unwrap();
        crate::storage::connection::configure_connection(&conn).unwrap();
        let app = tauri::test::mock_builder()
            .manage(crate::AppState {
                db: std::sync::Mutex::new(conn),
                main_geometry: std::sync::Mutex::new(None),
                shortcuts: std::sync::Mutex::new(crate::RegisteredShortcuts::default()),
            })
            .manage(DeleteSchedulerState::default())
            .invoke_handler(tauri::generate_handler![
                ipc_content_revision,
                ipc_content_list,
                ipc_content_detail,
                ipc_content_search_local,
                ipc_content_update_text,
                ipc_content_rename,
                ipc_content_update_structured,
                ipc_content_save,
                ipc_content_unsave,
                ipc_content_reorder,
                ipc_content_delete,
                ipc_content_restore,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let list = invoke_mock(
            &webview,
            "ipc_content_list",
            serde_json::json!({"scope":"all", "kind":null}),
        )
        .unwrap();
        assert_eq!(list.as_array().unwrap().len(), 6);
        assert_eq!(list[0]["id"], serde_json::json!("dock:text-1"));
        let search = invoke_mock(&webview, "ipc_content_search_local", serde_json::json!({
            "query":"数据库", "plan":{"kinds":["text"], "keywords":[], "aliases":[], "dateFrom":null, "dateTo":null}, "limit":10
        })).unwrap();
        assert_eq!(search[0]["summary"]["id"], serde_json::json!("dock:text-1"));
        let invalid_scope = invoke_mock(
            &webview,
            "ipc_content_list",
            serde_json::json!({"scope":"invalid", "kind":null}),
        )
        .unwrap_err();
        let invalid_kind = invoke_mock(
            &webview,
            "ipc_content_list",
            serde_json::json!({"scope":"all", "kind":"invalid"}),
        )
        .unwrap_err();
        let invalid_id = invoke_mock(
            &webview,
            "ipc_content_detail",
            serde_json::json!({"id":"invalid"}),
        )
        .unwrap_err();
        let missing_arguments =
            invoke_mock(&webview, "ipc_content_list", serde_json::json!({})).unwrap_err();
        for error in [invalid_scope, invalid_kind, invalid_id, missing_arguments] {
            assert!(
                error.is_string(),
                "IPC errors must use the string JSON shape"
            );
        }

        let events = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured = events.clone();
        app.listen("content-changed", move |event| {
            captured.lock().unwrap().push(event.payload().to_string())
        });
        let updated_text = invoke_mock(
            &webview,
            "ipc_content_update_text",
            serde_json::json!({
                "id":"dock:text-1",
                "title":"Maintenance",
                "body":"Saturday at 02:00"
            }),
        )
        .unwrap();
        assert_eq!(updated_text["kind"], serde_json::json!("text"));
        assert_eq!(updated_text["title"], serde_json::json!("Maintenance"));
        assert_eq!(updated_text["body"], serde_json::json!("Saturday at 02:00"));

        let renamed = invoke_mock(
            &webview,
            "ipc_content_rename",
            serde_json::json!({"id":"dock:file-1", "title":"Runbook"}),
        )
        .unwrap();
        assert_eq!(renamed["summary"]["title"], serde_json::json!("Runbook"));

        let structured = invoke_mock(
            &webview,
            "ipc_content_update_structured",
            serde_json::json!({
                "id":"vault:credential-1",
                "input":{
                    "kind":"credential",
                    "title":"Updated login",
                    "fields":[{"key":"username", "value":"operator", "isSensitive":false}],
                    "notes":"Rotated",
                    "manualTags":["work"]
                }
            }),
        )
        .unwrap();
        assert_eq!(structured["kind"], serde_json::json!("credential"));
        assert_eq!(
            structured["fields"][0]["value"],
            serde_json::json!("operator")
        );
        assert_eq!(events.lock().unwrap().len(), 3);

        let wrong_kind = invoke_mock(
            &webview,
            "ipc_content_update_text",
            serde_json::json!({"id":"dock:image-1", "title":null, "body":"wrong"}),
        )
        .unwrap_err();
        assert!(wrong_kind
            .as_str()
            .unwrap()
            .contains("source and kind do not match"));
        assert_eq!(events.lock().unwrap().len(), 3);

        let saved = invoke_mock(
            &webview,
            "ipc_content_save",
            serde_json::json!({"id":"dock:text-1"}),
        )
        .unwrap();
        assert_eq!(saved["retention"], serde_json::json!("saved"));
        assert!(events.lock().unwrap()[3].contains("dock:text-1"));

        let deleted = invoke_mock(
            &webview,
            "ipc_content_delete",
            serde_json::json!({"id":"dock:image-1"}),
        )
        .unwrap();
        assert!(deleted["token"].as_str().unwrap().len() >= 32);
        assert!(deleted.get("expiresAt").is_some());
        assert_eq!(
            invoke_mock(&webview, "ipc_content_revision", serde_json::json!({})).unwrap(),
            serde_json::json!({"revision":4})
        );
        let restored = invoke_mock(
            &webview,
            "ipc_content_restore",
            serde_json::json!({"token":deleted["token"]}),
        )
        .unwrap();
        assert_eq!(restored["id"], serde_json::json!("dock:image-1"));
        assert_eq!(
            invoke_mock(&webview, "ipc_content_revision", serde_json::json!({})).unwrap(),
            serde_json::json!({"revision":4})
        );
        let idle_deadline = Instant::now() + StdDuration::from_secs(2);
        while app
            .state::<DeleteSchedulerState>()
            .inner
            .gate
            .running
            .load(Ordering::Acquire)
            && Instant::now() < idle_deadline
        {
            std::thread::sleep(StdDuration::from_millis(10));
        }
        assert!(!app
            .state::<DeleteSchedulerState>()
            .inner
            .gate
            .running
            .load(Ordering::Acquire));
        drop(webview);
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}
