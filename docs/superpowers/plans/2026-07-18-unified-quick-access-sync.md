# Unified Quick Access and Synchronization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Quick Access 的录入与搜索使用全应用统一内容模型，并通过事件加修订号实现主窗口和快捷窗口的即时、可恢复同步，最后移除割裂的资料库遗留路径。

**Architecture:** Quick Access 复用主窗口的 `UnifiedSearchController`、`ContentSummaryCard` 和 capability-driven detail actions，但保持独立 query/draft/selection 状态。AI 只扩展脱敏查询词，不读取完整目录；本地统一搜索始终先返回。两个窗口监听 `content-changed` 并在 show/focus 时比较 revision，`ipc_open_main_content` 提供从快捷使用到主窗口管理的显式交接。

**Tech Stack:** Svelte 5, TypeScript, Vitest, Tauri 2 multi-window APIs/events, Rust, existing OpenAI-compatible planner

---

## Prerequisites and interaction rules

- Complete `docs/superpowers/plans/2026-07-18-unified-content-foundation.md` and `docs/superpowers/plans/2026-07-18-unified-main-workspace.md` first.
- Search covers text, image, file, credential, bookmark, and note in both windows.
- Quick Access is optimized for immediate copy/open; Main is optimized for edit/favorite/delete/reorder.
- Quick Access and Main share backend data, ranking, revision, and lifecycle semantics. They do not share query text, capture draft, selected ID, scroll, or open-detail state.
- Organized capture is permanently saved even when AI is disabled, unconfigured, cooling down, or fails.
- Existing strict shortcut behavior remains: while a window is visible, its global shortcut hides it; while hidden, the same shortcut shows it. Clicking elsewhere does not hide Quick Access.
- Quick Access keeps record and search panels mounted so repeated copy/paste/fill operations do not lose state.
- Theme/locale changes are shared preferences and update both windows through the existing token pipeline.

## File map

### Backend changes

- `src-tauri/src/content/ipc.rs` — unified query planning and open-main-content commands.
- `src-tauri/src/content/models.rs` — `PlannedUnifiedSearch` and main-content-open payload.
- `src-tauri/src/vault/ipc/search.rs` — internal redacted planner adapter; no catalog payload transfer.
- `src-tauri/src/vault/llm/prompt.rs` — query prompt language no longer says the searchable universe is Vault-only.
- `src-tauri/src/lib.rs` — command registration and selected-content main-window handoff.

### Frontend changes

- `src/lib/state/content-search.ts` — local-first optional AI expansion shared by both windows.
- `src/lib/api/content.ts` and `src/lib/types/content.ts` — plan-search and open-main methods.
- `src/lib/components/quick-access/SearchMode.svelte` — unified results/detail/actions.
- `src/lib/components/quick-access/SearchResultList.svelte` — all-kind summaries.
- `src/QuickAccessApp.svelte` — content event/revision sync and capture refresh.
- `src/App.svelte` — consume main-content-open handoff.
- `src/lib/components/content/QuickContentDetail.svelte` — immediate-use details for all kinds.
- `src/lib/state/content-notices.ts` — shared lifecycle/copy/refresh message selection for both visual shells.
- `src/lib/state/content-notices.test.ts` — Chinese/English message-code coverage.

### Cleanup candidates, deleted only after zero-reference proof

- `src/lib/components/views/HomeView.svelte`
- `src/lib/components/views/CategoriesView.svelte`
- `src/lib/components/views/NoteView.svelte`
- `src/lib/components/views/VaultView.svelte`
- `src/lib/components/views/VaultView.test.ts`
- `src/lib/state/dock.ts` and `src/lib/state/dock.test.ts` if no capture helper remains
- `src/lib/state/library-view.ts` and `src/lib/state/library-view.test.ts`
- `src/lib/state/vault-search.ts` and `src/lib/state/vault-search.test.ts`
- `src/lib/components/vault/EntryCard.svelte` if unified components no longer import it
- deprecated aliases and dead `vault-tags-updated` listener in `src/lib/api/vault.ts`

### Documentation

- `README.md` — one lifecycle and one search mental model.
- `README_ZH.md` — matching Chinese product model and usage flow.
- `docs/superpowers/reports/2026-07-18-unified-product-verification.md` — full upgrade/runtime evidence.

### Task 1: Add safe unified AI query planning

**Files:**
- Modify: `src-tauri/src/content/models.rs`
- Modify: `src-tauri/src/content/ipc.rs`
- Modify: `src-tauri/src/vault/ipc/search.rs`
- Modify: `src-tauri/src/vault/llm/prompt.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/content/ipc.rs`
- Test: `src-tauri/src/vault/ipc/search.rs`

- [ ] **Step 1: Write planner privacy and compatibility tests**

```rust
#[test]
fn unified_plan_uses_terms_and_dates_without_narrowing_legacy_kinds() {
    let legacy = PlannedSearch {
        plan: AiQueryPlan {
            kinds: vec![crate::vault::models::EntryKind::Note],
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
    assert!(unified.kinds.is_empty());
    assert_eq!(unified.keywords, vec!["部署"]);
    assert_eq!(unified.aliases, vec!["release"]);
    assert_eq!(unified.date_from.as_deref(), Some("2026-07-01"));
}

#[test]
fn planning_request_contains_query_but_never_catalog_content() {
    let messages = query_plan_prompt("找生产数据库", "2026-07-18T00:00:00Z");
    let audit = build_request_audit("test", "test-model", &messages);
    let sent = audit
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(sent.contains("找生产数据库"));
    assert!(!sent.contains("NeverSendCatalogFixture"));
}
```

- [ ] **Step 2: Run focused planner tests**

Run from `src-tauri/`:

```powershell
cargo test unified_plan_uses_terms_and_dates_without_narrowing_legacy_kinds -- --nocapture
cargo test planning_request_contains_query_but_never_catalog_content -- --nocapture
```

Expected: compilation fails because `adapt_vault_plan` and the unified planning command are missing.

- [ ] **Step 3: Implement the redacted adapter and command**

Add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedUnifiedSearch {
    pub plan: UnifiedQueryPlan,
    pub understood_terms: Vec<String>,
    pub audit: AiRequestAudit,
}

pub fn adapt_vault_plan(planned: PlannedSearch) -> PlannedUnifiedSearch {
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
```

Expose:

```rust
#[tauri::command]
pub async fn ipc_content_plan_search(
    app: AppHandle,
    query: String,
    request_id: String,
) -> Result<PlannedUnifiedSearch, String> {
    let planned = crate::vault::ipc::search::plan_search_redacted(
        &app,
        query,
        request_id,
    )
    .await?;
    Ok(adapt_vault_plan(planned))
}
```

The adapter deliberately clears legacy Vault `kinds` so an AI classification cannot exclude Dock text/image/file results. Explicit type chips are applied locally by the UI through `UnifiedQueryPlan.kinds` and are not inferred by this compatibility planner.

Update planner prompt product language to “本机内容”/“local content”; keep the existing guarantee that only the redacted query is sent and no catalog row, title, field, preview, tag list, or FTS document is included.

Register `ipc_content_plan_search` and `ipc_content_cancel_search`. The cancel command delegates to the existing request-ID-aware Vault planner cancellation so a late cleanup cannot cancel a newer query.

- [ ] **Step 4: Run planner, privacy, and full Rust tests**

Run:

```powershell
cargo test content::ipc::tests
cargo test vault::ipc::search::tests
cargo test
```

Expected: all tests pass; the catalog sentinel is absent from the captured request audit.

- [ ] **Step 5: Commit unified planning**

```powershell
git add src-tauri/src/content/models.rs src-tauri/src/content/ipc.rs src-tauri/src/vault/ipc/search.rs src-tauri/src/vault/llm/prompt.rs src-tauri/src/lib.rs
git commit -m "add safe unified query planning"
```

### Task 2: Extend the shared search controller to local-first AI expansion

**Files:**
- Modify: `src/lib/types/content.ts`
- Modify: `src/lib/api/content.ts`
- Modify: `src/lib/api/content.test.ts`
- Modify: `src/lib/state/content-search.ts`
- Modify: `src/lib/state/content-search.test.ts`

- [ ] **Step 1: Write failing phase, fallback, and cancellation tests**

```typescript
it('shows local results before optional AI expansion', async () => {
  const planner = deferred<PlannedUnifiedSearch>()
  const api = searchApi({
    local: [hit('dock:local')],
    expanded: [hit('dock:local'), hit('vault:expanded')],
    planner: planner.promise,
  })
  const states: ContentSearchState[] = []
  const controller = new UnifiedSearchController(api, (state) => states.push(state), {
    debounceMs: 0,
    aiDelayMs: 0,
    usePlanner: true,
  })

  const pending = controller.search('生产')
  await waitForState(states, 'local')
  expect(states.at(-1)?.hits[0].summary.id).toBe('dock:local')

  planner.resolve(plannedSearch(['prod']))
  await pending
  expect(states.at(-1)?.phase).toBe('expanded')
  expect(states.at(-1)?.hits.map((hit) => hit.summary.id)).toEqual([
    'dock:local',
    'vault:expanded',
  ])
})

it('keeps local results when planning fails', async () => {
  const api = searchApi({
    local: [hit('dock:local')],
    plannerError: new Error('offline'),
  })
  const controller = new UnifiedSearchController(api, vi.fn(), {
    debounceMs: 0,
    aiDelayMs: 0,
    usePlanner: true,
  })

  await controller.search('维护')
  expect(controller.snapshot.hits[0].summary.id).toBe('dock:local')
  expect(controller.snapshot.phase).toBe('local')
})

it('cancels the previous planner when a new query starts', async () => {
  const api = searchApi({ local: [], planner: pendingForever() })
  const controller = new UnifiedSearchController(api, vi.fn(), {
    debounceMs: 0,
    aiDelayMs: 0,
    usePlanner: true,
  })
  void controller.search('first')
  await controller.search('second')

  expect(api.cancelPlan).toHaveBeenCalledWith(expect.stringMatching(/^content-search-/))
})
```

- [ ] **Step 2: Verify enhanced-controller tests fail**

Run:

```powershell
pnpm exec vitest run src/lib/state/content-search.test.ts src/lib/api/content.test.ts
```

Expected: tests fail because the controller has no planner phases and `contentApi` has no plan/cancel methods.

- [ ] **Step 3: Implement the shared optional planner**

Extend phases to:

```typescript
export type ContentSearchPhase =
  | 'idle'
  | 'searching'
  | 'local'
  | 'planning'
  | 'expanded'
  | 'error'

export interface UnifiedSearchOptions {
  debounceMs: number
  aiDelayMs: number
  usePlanner: boolean
  limit?: number
}

export interface UnifiedSearchApi {
  searchLocal(
    query: string,
    plan: UnifiedQueryPlan | null,
    limit: number,
  ): Promise<ContentSearchHit[]>
  planSearch(query: string, requestId: string): Promise<PlannedUnifiedSearch>
  cancelPlan(requestId: string): Promise<void>
}
```

For each query:

1. cancel the previous request ID;
2. debounce once;
3. call local search with an explicit-kind-only plan when a type chip is selected, otherwise `plan = null`, and publish `local`;
4. if planner disabled, resolve;
5. wait `aiDelayMs`, publish `planning` without clearing hits;
6. call planner, merge any explicit UI type/date filters into the returned plan, and call the same local search endpoint again;
7. publish `expanded`;
8. on planner/expanded failure, keep local hits and return to `local`;
9. ignore all late results by request version.

Use `'content-search-' + crypto.randomUUID()` request IDs. Expose `setPlannerEnabled` so preference changes affect the next query without resetting the current query.

Add `contentApi.planSearch` and `cancelPlan` with the exact unified commands from Task 1.

- [ ] **Step 4: Run API/controller and existing Quick search tests**

Run:

```powershell
pnpm exec vitest run src/lib/api/content.test.ts src/lib/state/content-search.test.ts src/lib/components/quick-access/SearchMode.test.ts
```

Expected: controller/API tests pass; existing Quick test failures identify only the still-unmigrated Vault-shaped props addressed in Task 3.

- [ ] **Step 5: Commit shared hybrid search**

```powershell
git add src/lib/types/content.ts src/lib/api/content.ts src/lib/api/content.test.ts src/lib/state/content-search.ts src/lib/state/content-search.test.ts
git commit -m "extend unified search with local first planning"
```

### Task 3: Migrate Quick Access search to all content kinds

**Files:**
- Create: `src/lib/components/content/QuickContentDetail.svelte`
- Create: `src/lib/components/content/QuickContentDetail.test.ts`
- Modify: `src/lib/components/quick-access/SearchResultList.svelte`
- Modify: `src/lib/components/quick-access/SearchMode.svelte`
- Modify: `src/lib/components/quick-access/SearchMode.test.ts`
- Modify: `src/lib/i18n/types.ts`
- Modify: `src/lib/i18n/locales/zh-CN.ts`
- Modify: `src/lib/i18n/locales/en.ts`

- [ ] **Step 1: Replace Vault-only tests with all-kind user journeys**

```typescript
it('shows temporary dock and saved structured results together', async () => {
  mockContentSearch([
    hit('dock:text-1', 'temporary', { kind: 'text', title: '临时命令' }),
    hit('vault:credential-1', 'saved', { kind: 'credential', title: '生产数据库' }),
  ])
  const view = renderSearchMode()
  await typeQuery(view, '生产')

  expect(await view.findByText('临时命令')).toBeVisible()
  expect(view.getByText('生产数据库')).toBeVisible()
  expect(view.getByText('临时')).toBeVisible()
  expect(view.getByText('已收藏')).toBeVisible()
})

it.each([
  ['text', '复制文本'],
  ['image', '复制图片'],
  ['file', '复制文件'],
  ['credential', '复制密码'],
  ['bookmark', '打开链接'],
  ['note', '复制笔记'],
] as const)('offers the fastest useful action for %s', async (kind, action) => {
  mockSelectedDetail(detailFixture(kind))
  const view = renderSearchMode()
  await selectOnlyResult(view)

  expect(await view.findByRole('button', { name: action })).toBeVisible()
})

it('uses a large right-aligned copy target for every structured field', async () => {
  mockSelectedDetail(detailFixture('credential'))
  const view = renderSearchMode()
  await selectOnlyResult(view)

  for (const row of view.container.querySelectorAll('[data-field-row]')) {
    expect(row.lastElementChild).toHaveAttribute('data-copy-action')
    expect(row.lastElementChild).toHaveClass('quick-copy-target')
  }
})
```

- [ ] **Step 2: Run Quick search tests and verify the red state**

Run:

```powershell
pnpm exec vitest run src/lib/components/quick-access/SearchMode.test.ts src/lib/components/content/QuickContentDetail.test.ts
```

Expected: tests fail because SearchMode still consumes `VaultSearchHit` and `VaultEntryDetail` only.

- [ ] **Step 3: Implement unified Quick results and immediate-use details**

Replace `HybridSearchController`/`vaultApi.searchLocal` with a window-local `UnifiedSearchController`/`contentApi` instance. Keep the current query, selected ID, loaded-detail revision guard, ArrowUp/ArrowDown behavior, mounted panel behavior, and sensitive reset token.

`SearchResultList` props become:

```typescript
interface Props {
  hits: ContentSearchHit[]
  selectedId: string | null
  onSelect: (id: string) => void
}
```

Each result shows title, useful preview, kind label, and a small 临时/已收藏 status. It does not show source names.

`QuickContentDetail` props:

```typescript
interface Props {
  detail: ContentDetail
  resetToken: string | number
  onCopyText: (text: string, sensitive: boolean) => Promise<void>
  onCopyFile: (path: string, kind: 'image' | 'file') => Promise<void>
  onOpen: (target: string) => Promise<void>
  onManage: (id: string) => Promise<void>
  onNotify: (message: string, kind?: 'success' | 'error') => void
}
```

Order detail content:

1. title and primary immediate action;
2. directly useful fields, with copy buttons in the last column;
3. notes;
4. tags and metadata;
5. “在主窗口管理” secondary action.

Use at least `0.88rem` for values and `2.4rem` square primary copy targets. For Dock text, copy full content rather than truncated summary. For images/files, use existing backend clipboard commands. For bookmarks, validate and open the stored URL through the existing Tauri shell/open adapter; do not render raw HTML.

- [ ] **Step 4: Run Quick search, copy, and type gates**

Run:

```powershell
pnpm exec vitest run src/lib/components/quick-access/SearchMode.test.ts src/lib/components/content/QuickContentDetail.test.ts src/lib/components/vault/CopyableValue.test.ts
pnpm check
```

Expected: all six kinds render correct primary actions; sensitive reset and keyboard selection tests pass.

- [ ] **Step 5: Commit unified Quick search**

```powershell
git add src/lib/components/content/QuickContentDetail.svelte src/lib/components/content/QuickContentDetail.test.ts src/lib/components/quick-access/SearchResultList.svelte src/lib/components/quick-access/SearchMode.svelte src/lib/components/quick-access/SearchMode.test.ts src/lib/i18n/types.ts src/lib/i18n/locales/zh-CN.ts src/lib/i18n/locales/en.ts
git commit -m "search all content from quick access"
```

### Task 4: Refresh both windows after capture and content changes

**Files:**
- Modify: `src/QuickAccessApp.svelte`
- Modify: `src/QuickAccessApp.test.ts`
- Modify: `src/lib/components/quick-access/SearchMode.svelte`
- Modify: `src/lib/components/quick-access/CaptureMode.svelte`
- Modify: `src/lib/components/quick-access/CaptureMode.test.ts`
- Create: `src/lib/state/content-notices.ts`
- Create: `src/lib/state/content-notices.test.ts`
- Modify: `src/lib/i18n/types.ts`
- Modify: `src/lib/i18n/locales/zh-CN.ts`
- Modify: `src/lib/i18n/locales/en.ts`

- [ ] **Step 1: Write cross-mode and revision recovery tests**

```typescript
it('organized capture is saved and refreshes an active matching search', async () => {
  const view = render(QuickAccessApp)
  await switchToSearchAndType(view, '数据库')
  await switchToRecordAndSave(view, captureDraft('生产数据库'))

  expect(contentApi.revision).toHaveBeenCalled()
  await switchToSearch(view)
  expect(contentApi.searchLocal).toHaveBeenCalledWith('数据库', null, expect.any(Number))
  expect(view.getByText('已收藏')).toBeVisible()
})

it('repairs missed changes whenever quick access is shown', async () => {
  render(QuickAccessApp)
  contentApi.revision.mockResolvedValueOnce({ revision: 8 })
  emitTauri('quick-access-focus-input')

  await waitFor(() => expect(contentApi.revision).toHaveBeenCalled())
  expect(contentApi.searchLocal).toHaveBeenCalled()
})

it('keeps record draft and search query independent across hide and show', async () => {
  const view = render(QuickAccessApp)
  await typeRecordDraft(view, 'draft text')
  await switchToSearchAndType(view, 'query text')
  emitTauri('quick-access-focus-input')
  await switchToRecord(view)

  expect(view.getByRole('textbox', { name: '录入内容' })).toHaveValue('draft text')
  await switchToSearch(view)
  expect(view.getByRole('searchbox')).toHaveValue('query text')
})

it.each(['saved', 'unsaved', 'deleted', 'deleteFailedRestored', 'undoExpired', 'copyFailed', 'refreshFailed'] as const)(
  'resolves shared content notice %s in both locales',
  (code) => {
    expect(resolveContentNotice(zhCN, code)).not.toHaveLength(0)
    expect(resolveContentNotice(en, code)).not.toHaveLength(0)
  },
)
```

- [ ] **Step 2: Run Quick root/capture tests**

Run:

```powershell
pnpm exec vitest run src/QuickAccessApp.test.ts src/lib/components/quick-access/CaptureMode.test.ts src/lib/state/content-notices.test.ts
```

Expected: refresh assertions fail because `onCaptureSaved` is currently a no-op and Quick Access does not consume ordinary content changes; notice tests fail because the shared resolver is missing.

- [ ] **Step 3: Implement revision-driven refresh without resetting UI state**

In `QuickAccessApp.svelte`:

```typescript
let contentRevision = $state(0)
let searchRefreshToken = $state(0)

async function repairContentIfStale(): Promise<void> {
  const latest = await contentApi.revision()
  if (latest.revision <= contentRevision) return
  contentRevision = latest.revision
  searchRefreshToken += 1
}

function onCaptureSaved(_id: string): void {
  notice(messages.workspace.saved, 'success')
  void repairContentIfStale()
}
```

Change `CaptureMode`'s `onSaved` callback to return the namespaced ID string after `createFromCapture` succeeds. The adapter prefixes the returned Vault source ID with `vault:` before invoking the callback. Do not clear the record textarea until the backend save succeeds. A local/AI failure before save keeps the draft; an AI failure with a valid local draft still allows save and produces saved retention.

Subscribe to `content-changed` on mount. If event revision is newer, update `contentRevision` and bump `searchRefreshToken`. On every `quick-access-focus-input` event, call `repairContentIfStale` after refreshing preferences/AI settings. Pass the token to SearchMode; rerun only when its query is non-empty, preserving selected ID when still present.

Do not remount CaptureMode or SearchMode. Do not couple their text state.

Implement the shared notice resolver:

```typescript
export type ContentNoticeCode =
  | 'saved'
  | 'unsaved'
  | 'deleted'
  | 'deleteFailedRestored'
  | 'undoExpired'
  | 'copyFailed'
  | 'refreshFailed'

export function resolveContentNotice(
  locale: LocaleMessages,
  code: ContentNoticeCode,
): string {
  return locale.workspace.notices[code]
}
```

Add all seven `workspace.notices` keys to the locale contract and both locales. Main continues to render these messages in its bottom toast; Quick continues to render them in its inline notice. Neither component hard-codes competing lifecycle wording.

- [ ] **Step 4: Run Quick root, capture, search, and shortcut tests**

Run:

```powershell
pnpm exec vitest run src/QuickAccessApp.test.ts src/lib/components/quick-access/CaptureMode.test.ts src/lib/components/quick-access/SearchMode.test.ts src/lib/state/content-notices.test.ts src/lib/state/quick-access.test.ts src/lib/state/window.test.ts
```

Expected: active searches refresh, drafts/queries survive hide/show, blur does not hide, and shortcut toggle helper tests remain green.

- [ ] **Step 5: Commit Quick synchronization**

```powershell
git add src/QuickAccessApp.svelte src/QuickAccessApp.test.ts src/lib/components/quick-access/SearchMode.svelte src/lib/components/quick-access/CaptureMode.svelte src/lib/components/quick-access/CaptureMode.test.ts src/lib/state/content-notices.ts src/lib/state/content-notices.test.ts src/lib/i18n/types.ts src/lib/i18n/locales/zh-CN.ts src/lib/i18n/locales/en.ts
git commit -m "synchronize quick access content state"
```

### Task 5: Add explicit Quick-to-Main management handoff

**Files:**
- Modify: `src-tauri/src/content/models.rs`
- Modify: `src-tauri/src/content/ipc.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/content.ts`
- Modify: `src/lib/api/content.test.ts`
- Modify: `src/lib/components/content/QuickContentDetail.svelte`
- Modify: `src/App.svelte`
- Modify: `src/App.test.ts`

- [ ] **Step 1: Write handoff tests**

```rust
#[test]
fn main_content_open_payload_requires_a_valid_unified_id() {
    assert!(MainContentOpen::new("dock:de-1").is_ok());
    assert!(MainContentOpen::new("vault:ve-1").is_ok());
    assert!(MainContentOpen::new("ve-1").is_err());
}
```

```typescript
it('opens the main window on the requested unified item', async () => {
  render(App)
  emitTauri('main-open-content', { id: 'vault:credential-1' })

  await waitFor(() => expect(contentApi.list).toHaveBeenCalledWith('all', null))
  expect(contentApi.detail).toHaveBeenCalledWith('vault:credential-1')
})

it('quick detail delegates management to one backend command', async () => {
  const view = renderQuickDetail(detailFixture('credential'))
  await fireEvent.click(view.getByRole('button', { name: '在主窗口管理' }))
  expect(contentApi.openInMain).toHaveBeenCalledWith('vault:credential-1')
})
```

- [ ] **Step 2: Run backend/frontend handoff tests**

Run:

```powershell
Push-Location src-tauri
cargo test main_content_open_payload_requires_a_valid_unified_id -- --nocapture
Pop-Location
pnpm exec vitest run src/App.test.ts src/lib/components/content/QuickContentDetail.test.ts src/lib/api/content.test.ts
```

Expected: tests fail because the handoff command/event does not exist.

- [ ] **Step 3: Implement a focused handoff**

Add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[tauri::command]
pub fn ipc_open_main_content(app: AppHandle, id: String) -> Result<(), String> {
    let payload = MainContentOpen::new(&id)?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    app.emit("main-open-content", payload)
        .map_err(|error| error.to_string())
}
```

Add `contentApi.openInMain(id)`. In App, the event handler:

1. exits settings;
2. switches to `all` without changing the Quick query;
3. awaits browser load;
4. selects the ID if present;
5. loads detail and opens the responsive detail layer;
6. shows a localized “内容已不存在” message if the item was deleted between search and handoff.

- [ ] **Step 4: Run handoff and window behavior tests**

Run:

```powershell
Push-Location src-tauri
cargo test main_content_open_payload_requires_a_valid_unified_id
Pop-Location
pnpm exec vitest run src/App.test.ts src/lib/components/content/QuickContentDetail.test.ts src/lib/api/content.test.ts src/lib/state/window.test.ts
```

Expected: all handoff tests pass; existing strict shortcut toggle tests remain unchanged.

- [ ] **Step 5: Commit the handoff**

```powershell
git add src-tauri/src/content/models.rs src-tauri/src/content/ipc.rs src-tauri/src/lib.rs src/lib/api/content.ts src/lib/api/content.test.ts src/lib/components/content/QuickContentDetail.svelte src/App.svelte src/App.test.ts
git commit -m "open unified content in main window"
```

### Task 6: Remove obsolete product silos and dead compatibility code

**Files:**
- Delete only zero-reference candidates listed in the file map
- Modify: `src/lib/api/vault.ts`
- Modify: `src/lib/types/vault.ts`
- Modify: `src/lib/api/dock.ts`
- Modify: `src/lib/types/dock.ts`
- Modify: `src/lib/i18n/types.ts`
- Modify: `src/lib/i18n/locales/zh-CN.ts`
- Modify: `src/lib/i18n/locales/en.ts`
- Modify: `README.md`
- Modify: `README_ZH.md`

- [ ] **Step 1: Capture a zero-reference baseline**

Run:

```powershell
rg -n "HomeView|CategoriesView|NoteView|VaultView|LibraryViewController|HybridSearchController|onTagsUpdated|vault-tags-updated|vaultApi\.(search|llmSearch|updateTags|retag|setLlmConfig|testLlm)" src src-tauri/src
```

Expected: every hit is either the candidate's own file/test or a legacy import that must be migrated before deletion. Save the exact result in the final verification report.

- [ ] **Step 2: Add a terminology guard test**

Add to `src/lib/i18n/__tests__/i18n.test.ts`:

```typescript
it('does not expose library as a separate destination', () => {
  for (const locale of [zhCN, en]) {
    expect('library' in locale.nav).toBe(false)
    expect(JSON.stringify(locale.nav)).not.toContain('资料库')
    expect(JSON.stringify(locale.nav)).not.toContain('Library')
  }
})
```

Run:

```powershell
pnpm exec vitest run src/lib/i18n/__tests__/i18n.test.ts
```

Expected: the test fails while `nav.library` and old locale navigation strings remain.

- [ ] **Step 3: Delete only proven-unused code and rename user-facing text**

For each candidate, run `rg -n "<SymbolOrPathStem>" src src-tauri/src` immediately before deletion. Delete it only when no production import remains.

Remove from `vaultApi`:

- deprecated `search`, `llmSearch`, `updateTags`, `retag`, `setLlmConfig`, and `testLlm` aliases;
- `onTagsUpdated` and `TagUpdateEvent` if no backend emitter exists;
- comments that describe Vault as the primary product surface.

Keep:

- structured entry CRUD used by unified editors;
- capture enrichment/create;
- LLM configuration/settings;
- sensitive clipboard handling.

Remove `library` navigation translations and move still-used structured editor labels under `workspace.structured` or `quickAccess`. Change Quick window title in `src-tauri/tauri.conf.json` from “Soma Scratchpad - Library” to “Soma Scratchpad - Quick Access”.

Update README with exactly this lifecycle:

```markdown
## Content lifecycle

- Main-window paste, drag, and new text enter **收纳** temporarily.
- Favoriting any item keeps it permanently; unfavoriting returns it to temporary retention.
- Organized Quick Access capture is saved permanently, with or without AI.
- Search covers every item that still exists: temporary and saved text, images, files, credentials, bookmarks, and notes.
- Quick Access is for immediate capture/use; the main window is for ongoing management.
```

Update `README_ZH.md` with the same five rules using 收纳/全部/收藏/快捷入口 terminology, and update both feature tables and screenshots so neither document advertises 资料库 as a separate destination.

- [ ] **Step 4: Prove cleanup completeness**

Run:

```powershell
rg -n "HomeView|CategoriesView|NoteView|VaultView|LibraryViewController|HybridSearchController|onTagsUpdated|vault-tags-updated|@deprecated|nav\.library" src src-tauri/src
pnpm test:unit
pnpm check
pnpm build
Push-Location src-tauri
cargo test
Pop-Location
```

Expected: reference scan returns no obsolete production symbol; all automated gates pass.

- [ ] **Step 5: Commit legacy cleanup**

Stage only the audited cleanup paths, excluding `src-tauri/Cargo.toml`:

```powershell
git add -- src/lib/components/views/HomeView.svelte src/lib/components/views/CategoriesView.svelte src/lib/components/views/NoteView.svelte src/lib/components/views/VaultView.svelte src/lib/components/views/VaultView.test.ts
git add -- src/lib/state/dock.ts src/lib/state/dock.test.ts src/lib/state/library-view.ts src/lib/state/library-view.test.ts src/lib/state/vault-search.ts src/lib/state/vault-search.test.ts
git add -- src/lib/components/vault/EntryCard.svelte src/lib/api/vault.ts src/lib/types/vault.ts src/lib/api/dock.ts src/lib/types/dock.ts
git add -- src/lib/i18n/types.ts src/lib/i18n/locales/zh-CN.ts src/lib/i18n/locales/en.ts src/lib/i18n/__tests__/i18n.test.ts
git add -- src-tauri/tauri.conf.json README.md README_ZH.md
git commit -m "remove fragmented library interfaces"
```

### Task 7: Execute full upgraded-product and Windows UX verification

**Files:**
- Create: `docs/superpowers/reports/2026-07-18-unified-product-verification.md`
- Modify: `src-tauri/examples/create_validation_fixtures.rs` only when the final matrix needs another deterministic edge case
- Modify: only files whose behavior fails an acceptance row

- [ ] **Step 1: Create two non-production validation databases**

Prepare:

1. fresh schema with one item of each kind;
2. pre-unified schema containing Home-only Dock rows, Note/favorite Dock rows, dual-membership Dock rows, Vault credentials with sensitive fields, bookmarks, notes, AI metadata, and manual ordering.

Generate them without touching live data:

```powershell
Push-Location src-tauri
cargo run --example create_validation_fixtures -- ..\test-data\final-validation
Pop-Location
```

Record SHA-256 hashes and row counts before the run:

```powershell
Get-FileHash .\test-data\final-validation\fresh-validation.sqlite3 -Algorithm SHA256
Get-FileHash .\test-data\final-validation\legacy-validation.sqlite3 -Algorithm SHA256
```

Copy, never move, a fixture into a temporary validation data directory before each run. Do not use the user's live application data.

- [ ] **Step 2: Run the complete automated gate**

```powershell
pnpm test:unit
pnpm check
pnpm build
Push-Location src-tauri
cargo fmt -- --check
cargo test
Pop-Location
git diff --check
```

Expected:

- all frontend tests pass;
- zero Svelte errors;
- build passes;
- Rust formatting and all tests pass;
- no whitespace errors.

- [ ] **Step 3: Run the end-to-end product matrix**

Launch `pnpm tauri dev` once per copied fixture and verify:

| Area | Action | Expected observable result |
|---|---|---|
| Lightweight capture | paste text/image/file in Main | each appears at top of 收纳 as temporary |
| Organized capture | save with AI configured | item appears in 收藏 and searches immediately |
| Organized capture fallback | disable AI and save | local draft still saves permanently and searches |
| Lifecycle | favorite old Dock and Vault items | both use the same 收藏 action and remain after restart |
| Lifecycle | unfavorite either source | both return to 收纳 with new cleanup timing |
| Search | query old Dock text in Quick | temporary result appears with immediate copy |
| Search | query credential in Main | structured result appears; sensitive value stays masked |
| Search | query image/file name | corresponding temporary/saved assets appear |
| Quick details | inspect credential | title/useful fields first, notes later, copy buttons right aligned and large |
| Persistence | click outside Quick | window remains open and record/search state remains |
| Shortcut | press Quick shortcut while visible/hidden | first press hides, next press shows |
| Shortcut | press Main shortcut while visible/hidden | first press hides, next press shows |
| Cross-window | capture in Quick while Main open | Main updates without route reset |
| Missed event | hide a window during mutation, then show | revision comparison repairs the list/search |
| Handoff | click 在主窗口管理 | Main focuses requested item and opens detail |
| Undo | delete then undo | payload, retention, searchability, and order restore |
| Upgrade | open legacy fixture | counts/order/retention map correctly with no duplicate |
| Privacy | search/copy credential | password absent from FTS/events/logs; clipboard reset still works |
| Theme | switch every theme in Main | Quick updates tokens without losing draft/query |
| Locale | switch Chinese/English | both windows update labels without losing state |

- [ ] **Step 4: Audit final product cohesion and record evidence**

The report must include:

- fresh and legacy fixture hashes/counts;
- automated command summaries;
- every matrix row with pass/fail and decisive observation;
- screenshots for Main 240×180, Main 360×640, Quick 680×480, and expanded split layout;
- revision values before/after a hidden-window mutation;
- FTS query proving the sensitive fixture literal is absent;
- event payload sample proving it contains only revision/ID/operation;
- final reference scan proving old top-level interfaces are gone;
- explicit confirmation that draft/query state remain window-local.

If a row fails, fix the earliest responsible layer, rerun that row from a fresh copied fixture, then rerun the complete automated gate.

- [ ] **Step 5: Commit final verification**

```powershell
git add docs/superpowers/reports/2026-07-18-unified-product-verification.md
git commit -m "verify unified scratchpad product"
```

## Final acceptance gate

The feature is complete only when:

- users see two capture paths, one content lifecycle, and one search universe;
- temporary-first lightweight capture and permanently saved organized capture both match the approved rules;
- search finds all existing content without requiring the user to know its source;
- Quick Access and Main have distinct interaction density but identical data/lifecycle/ranking;
- cross-window changes appear immediately or are repaired on focus by revision;
- strict show/hide shortcuts and persistent-on-blur Quick behavior remain correct;
- details prioritize useful information and large aligned actions;
- themes/locales stay synchronized without coupling draft/query state;
- obsolete 资料库 navigation/state/API compatibility layers are removed;
- fresh install, upgrade, privacy, undo, and Windows runtime matrices pass.
