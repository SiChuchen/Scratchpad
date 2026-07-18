# Unified Main Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将主窗口的收纳、全部、收藏和原资料库内容收敛为一个连续工作区，让用户在同一搜索、卡片、详情和生命周期操作中管理所有内容类型。

**Architecture:** 主窗口只维护一个 `ContentBrowserController` 和一个 `UnifiedSearchController`，通过 `contentApi` 读取统一摘要与详情。`ContentWorkspace.svelte` 负责浏览/搜索状态切换，内容卡与详情根据 kind/capabilities 分派到轻量或结构化详情组件；旧视图仍保留在仓库中直到第三份计划完成 Quick Access 迁移和引用审计。

**Tech Stack:** Svelte 5 runes, TypeScript, Vitest, Testing Library, Tauri 2 IPC/events, shared theme tokens

---

## Prerequisites and UX constraints

- Complete `docs/superpowers/plans/2026-07-18-unified-content-foundation.md` first.
- Read `docs/superpowers/specs/2026-07-18-unified-content-lifecycle-search-design.md` before changing UI.
- Design and test at the configured default main-window size `360 × 640` and minimum `240 × 180`. At widths below `680px` use one column; never require a permanent side panel.
- Keep lightweight capture intact: paste/drag/+ creates temporary content in 收纳. Organized capture remains in Quick Access and is already saved.
- Search is global across all existing content, regardless of the active 收纳/全部/收藏 scope.
- Clearing search restores the prior scope, selected item, and scroll position.
- The primary lifecycle action always reads 收藏/取消收藏. Do not expose “Dock”, “Vault”, source tables, AI status, or migration terminology in normal content cards.
- The main window uses the existing theme tokens and locale pipeline. No hard-coded dark/light colors.

## File map

### New state and tests

- `src/lib/state/content-browser.ts` — browse scope, type filter, selection, revision, stale refresh, optimistic reorder.
- `src/lib/state/content-browser.test.ts` — scope order, selection preservation, revision recovery.
- `src/lib/state/content-search.ts` — debounced all-content local search and stable selection.
- `src/lib/state/content-search.test.ts` — race cancellation, search clear, ordering/selection contract.

### New shared UI

- `src/lib/components/content/ContentKindIcon.svelte` — six-kind semantic icon map.
- `src/lib/components/content/ContentSummaryCard.svelte` — compact summary, lifecycle button, main actions.
- `src/lib/components/content/ContentDetail.svelte` — capability-driven detail dispatcher.
- `src/lib/components/content/SimpleContentDetail.svelte` — text/image/file detail and edit adapter.
- `src/lib/components/content/StructuredContentDetail.svelte` — credential/bookmark/note detail/editor adapter.
- `src/lib/components/content/ContentList.svelte` — keyed list, keyboard selection, drag ordering.
- `src/lib/components/content/ContentSearchBar.svelte` — global search, clear, type filters.
- `src/lib/components/views/ContentWorkspace.svelte` — single main content surface.
- Matching `*.test.ts` files beside state/components where behavior is non-trivial.

### Existing files changed

- `src/App.svelte` — one content controller, one workspace, unified mutation handlers, event/revision refresh.
- `src/lib/components/TopBar.svelte` — 收纳/全部/收藏/设置 only.
- `src/lib/api/dock.ts` — keep only lightweight capture/file-specific adapters used by main.
- `src/lib/api/vault.ts` — keep structured edit and AI settings adapters; no main list/search ownership.
- `src/lib/i18n/types.ts` — unified workspace message contract.
- `src/lib/i18n/locales/zh-CN.ts` — user-facing Chinese terminology.
- `src/lib/i18n/locales/en.ts` — matching English terminology.
- `src/app.css` — shared focus, truncation, and responsive workspace rules only.

## Stable frontend state contracts

```typescript
export interface ContentBrowserState {
  scope: BrowseScope
  kind: ContentKind | null
  items: ContentSummary[]
  selectedId: string | null
  revision: number
  phase: 'idle' | 'loading' | 'ready' | 'error'
  error: string | null
}

export interface ContentSearchState {
  query: string
  hits: ContentSearchHit[]
  selectedId: string | null
  phase: 'idle' | 'searching' | 'ready' | 'error'
  error: string | null
}
```

### Task 1: Build the unified browse controller

**Files:**
- Create: `src/lib/state/content-browser.ts`
- Create: `src/lib/state/content-browser.test.ts`

- [ ] **Step 1: Write failing controller tests**

Use a fake API that returns explicit scope lists and revisions:

```typescript
it('loads a scope and keeps a still-visible selection', async () => {
  const api = fakeContentApi({
    temporary: [summary('dock:a'), summary('dock:b')],
    saved: [summary('vault:c', 'saved')],
  })
  const states: ContentBrowserState[] = []
  const controller = new ContentBrowserController(api, (state) => states.push(state))

  await controller.load('temporary')
  controller.select('dock:b')
  await controller.refresh()

  expect(states.at(-1)?.selectedId).toBe('dock:b')
})

it('repairs a missed event when backend revision advanced', async () => {
  const api = fakeContentApi({
    temporary: [summary('dock:a')],
    revision: 4,
  })
  const controller = new ContentBrowserController(api, vi.fn())
  await controller.load('temporary')
  api.setRevision(5)
  api.setScope('temporary', [summary('dock:new'), summary('dock:a')])

  expect(await controller.refreshIfStale()).toBe(true)
  expect(controller.snapshot.items.map((item) => item.id)).toEqual([
    'dock:new',
    'dock:a',
  ])
})

it('rolls an optimistic reorder back when persistence fails', async () => {
  const api = fakeContentApi({
    saved: [summary('dock:a', 'saved'), summary('vault:b', 'saved')],
    reorderError: new Error('write failed'),
  })
  const controller = new ContentBrowserController(api, vi.fn())
  await controller.load('saved')

  await expect(controller.reorder(['vault:b', 'dock:a'])).rejects.toThrow('write failed')
  expect(controller.snapshot.items.map((item) => item.id)).toEqual(['dock:a', 'vault:b'])
})
```

- [ ] **Step 2: Run the tests and verify the red state**

Run:

```powershell
pnpm exec vitest run src/lib/state/content-browser.test.ts
```

Expected: test collection fails because `ContentBrowserController` does not exist.

- [ ] **Step 3: Implement the controller**

Define the dependency boundary:

```typescript
export interface ContentBrowserApi {
  list(scope: BrowseScope, kind: ContentKind | null): Promise<ContentSummary[]>
  revision(): Promise<ContentRevision>
  reorder(scope: BrowseScope, orderedIds: string[]): Promise<void>
}

export class ContentBrowserController {
  private state: ContentBrowserState
  private requestVersion = 0

  constructor(
    private readonly api: ContentBrowserApi,
    private readonly onState: (state: ContentBrowserState) => void,
  ) {
    this.state = {
      scope: 'temporary',
      kind: null,
      items: [],
      selectedId: null,
      revision: 0,
      phase: 'idle',
      error: null,
    }
  }

  get snapshot(): ContentBrowserState {
    return structuredClone(this.state)
  }

  select(id: string | null): void {
    this.publish({ selectedId: id })
  }

  async load(scope: BrowseScope, kind = this.state.kind): Promise<void> {
    const request = ++this.requestVersion
    this.publish({ scope, kind, phase: 'loading', error: null })
    const [items, revision] = await Promise.all([
      this.api.list(scope, kind),
      this.api.revision(),
    ])
    if (request !== this.requestVersion) return
    const selectedId = items.some((item) => item.id === this.state.selectedId)
      ? this.state.selectedId
      : items[0]?.id ?? null
    this.publish({
      items,
      revision: revision.revision,
      selectedId,
      phase: 'ready',
      error: null,
    })
  }

  async refresh(): Promise<void> {
    await this.load(this.state.scope, this.state.kind)
  }

  async refreshIfStale(): Promise<boolean> {
    const latest = await this.api.revision()
    if (latest.revision === this.state.revision) return false
    await this.refresh()
    return true
  }

  async reorder(orderedIds: string[]): Promise<void> {
    if (this.state.scope === 'all') {
      throw new Error('all scope cannot be manually reordered')
    }
    const before = this.state.items
    const byId = new Map(before.map((item) => [item.id, item]))
    this.publish({ items: orderedIds.map((id) => byId.get(id)!).filter(Boolean) })
    try {
      await this.api.reorder(this.state.scope, orderedIds)
      await this.refresh()
    } catch (error) {
      this.publish({ items: before })
      throw error
    }
  }

  private publish(patch: Partial<ContentBrowserState>): void {
    this.state = { ...this.state, ...patch }
    this.onState(this.snapshot)
  }
}
```

Add `setKind` and an error branch that ignores stale failures, preserves existing items during refresh, and publishes a user-readable error without losing scope.

- [ ] **Step 4: Run the controller tests**

Run:

```powershell
pnpm exec vitest run src/lib/state/content-browser.test.ts
```

Expected: all browse-controller tests pass.

- [ ] **Step 5: Commit browse state**

```powershell
git add src/lib/state/content-browser.ts src/lib/state/content-browser.test.ts
git commit -m "add unified content browser state"
```

### Task 2: Build the shared search controller

**Files:**
- Create: `src/lib/state/content-search.ts`
- Create: `src/lib/state/content-search.test.ts`

- [ ] **Step 1: Write failing search race and selection tests**

```typescript
it('publishes only the latest query when responses arrive out of order', async () => {
  const alpha = deferred<ContentSearchHit[]>()
  const beta = deferred<ContentSearchHit[]>()
  const api = {
    searchLocal: vi.fn((query: string) => query === 'a' ? alpha.promise : beta.promise),
  }
  const states: ContentSearchState[] = []
  const controller = new UnifiedSearchController(api, (state) => states.push(state), 0)

  const first = controller.search('a')
  const second = controller.search('ab')
  beta.resolve([hit('vault:new')])
  alpha.resolve([hit('dock:old')])
  await Promise.all([first, second])

  expect(states.at(-1)?.query).toBe('ab')
  expect(states.at(-1)?.hits[0].summary.id).toBe('vault:new')
})

it('keeps selection when the id survives and selects first otherwise', async () => {
  const api = queuedSearchApi([
    [hit('dock:a'), hit('vault:b')],
    [hit('vault:b'), hit('dock:c')],
    [hit('dock:c')],
  ])
  const controller = new UnifiedSearchController(api, vi.fn(), 0)
  await controller.search('first')
  controller.select('vault:b')
  await controller.search('second')
  expect(controller.snapshot.selectedId).toBe('vault:b')
  await controller.search('third')
  expect(controller.snapshot.selectedId).toBe('dock:c')
})

it('applies only an explicitly selected kind to global search', async () => {
  const api = queuedSearchApi([[hit('dock:image', { kind: 'image' })]])
  const controller = new UnifiedSearchController(api, vi.fn(), 0)
  controller.setKinds(['image'])
  await controller.search('截图')

  expect(api.searchLocal).toHaveBeenCalledWith('截图', {
    kinds: ['image'],
    keywords: [],
    aliases: [],
    dateFrom: null,
    dateTo: null,
  }, 50)
})

it('keeps the last valid results when a refresh fails', async () => {
  const api = queuedSearchApi([
    [hit('dock:still-visible')],
    new Error('temporary read failure'),
  ])
  const controller = new UnifiedSearchController(api, vi.fn(), 0)
  await controller.search('first')
  controller.select('dock:still-visible')
  await controller.search('refresh')

  expect(controller.snapshot.hits[0].summary.id).toBe('dock:still-visible')
  expect(controller.snapshot.selectedId).toBe('dock:still-visible')
  expect(controller.snapshot.phase).toBe('error')
})
```

- [ ] **Step 2: Verify tests fail**

Run:

```powershell
pnpm exec vitest run src/lib/state/content-search.test.ts
```

Expected: test collection fails because `UnifiedSearchController` is missing.

- [ ] **Step 3: Implement debounced all-content search**

Use this API and state transition:

```typescript
export interface UnifiedSearchApi {
  searchLocal(
    query: string,
    plan: UnifiedQueryPlan | null,
    limit: number,
  ): Promise<ContentSearchHit[]>
}

export class UnifiedSearchController {
  private state: ContentSearchState = {
    query: '',
    hits: [],
    selectedId: null,
    phase: 'idle',
    error: null,
  }
  private requestVersion = 0
  private kinds: ContentKind[] = []

  constructor(
    private readonly api: UnifiedSearchApi,
    private readonly onState: (state: ContentSearchState) => void,
    private readonly delayMs = 200,
    private readonly limit = 50,
  ) {}

  get snapshot(): ContentSearchState {
    return structuredClone(this.state)
  }

  select(id: string | null): void {
    this.publish({ selectedId: id })
  }

  setKinds(kinds: ContentKind[]): void {
    this.kinds = [...kinds]
  }

  async search(query: string): Promise<void> {
    const normalized = query.trim()
    const request = ++this.requestVersion
    if (!normalized) {
      this.publish({ query: '', hits: [], selectedId: null, phase: 'idle', error: null })
      return
    }
    this.publish({ query, phase: 'searching', error: null })
    await new Promise<void>((resolve) => setTimeout(resolve, this.delayMs))
    if (request !== this.requestVersion) return
    try {
      const plan: UnifiedQueryPlan | null = this.kinds.length
        ? {
            kinds: this.kinds,
            keywords: [],
            aliases: [],
            dateFrom: null,
            dateTo: null,
          }
        : null
      const hits = await this.api.searchLocal(normalized, plan, this.limit)
      if (request !== this.requestVersion) return
      const selectedId = hits.some((hit) => hit.summary.id === this.state.selectedId)
        ? this.state.selectedId
        : hits[0]?.summary.id ?? null
      this.publish({ hits, selectedId, phase: 'ready', error: null })
    } catch (error) {
      if (request === this.requestVersion) {
        this.publish({ phase: 'error', error: String(error) })
      }
    }
  }

  dispose(): void {
    this.requestVersion += 1
  }

  private publish(patch: Partial<ContentSearchState>): void {
    this.state = { ...this.state, ...patch }
    this.onState(this.snapshot)
  }
}
```

The controller intentionally searches all content and does not receive the active browse scope. `ContentSearchBar` calls `setKinds` only when the user explicitly selects a type; clearing the type chip calls `setKinds([])`.

- [ ] **Step 4: Run search tests**

Run:

```powershell
pnpm exec vitest run src/lib/state/content-search.test.ts
```

Expected: latest-query, selection, clear, and error tests pass.

- [ ] **Step 5: Commit search state**

```powershell
git add src/lib/state/content-search.ts src/lib/state/content-search.test.ts
git commit -m "add shared unified search state"
```

### Task 3: Create one card language for all six content kinds

**Files:**
- Create: `src/lib/components/content/ContentKindIcon.svelte`
- Create: `src/lib/components/content/ContentSummaryCard.svelte`
- Create: `src/lib/components/content/ContentSummaryCard.test.ts`
- Create: `src/lib/components/content/ContentList.svelte`
- Create: `src/lib/components/content/ContentList.test.ts`

- [ ] **Step 1: Write failing user-facing card tests**

```typescript
it('shows kind, useful preview, retention, and aligned primary actions', () => {
  const { getByRole, getByText } = render(ContentSummaryCard, {
    props: {
      item: summary('vault:db', 'saved', {
        kind: 'credential',
        title: '生产数据库',
        preview: 'alice · db.internal',
      }),
      selected: false,
      busy: false,
      onSelect: vi.fn(),
      onToggleSaved: vi.fn(),
      onCopy: vi.fn(),
      onDelete: vi.fn(),
    },
  })

  expect(getByText('生产数据库')).toBeVisible()
  expect(getByText('alice · db.internal')).toBeVisible()
  expect(getByRole('button', { name: '取消收藏' })).toBeVisible()
  expect(getByRole('button', { name: '复制' })).toBeVisible()
  expect(getByRole('button', { name: '删除' })).toBeVisible()
})

it('uses the same favorite action for dock and vault ids', () => {
  const dock = renderCard(summary('dock:a', 'temporary'))
  const vault = renderCard(summary('vault:b', 'temporary', { kind: 'bookmark' }))

  expect(dock.getByRole('button', { name: '收藏' })).toBeVisible()
  expect(vault.getByRole('button', { name: '收藏' })).toBeVisible()
})
```

- [ ] **Step 2: Verify component tests fail**

Run:

```powershell
pnpm exec vitest run src/lib/components/content/ContentSummaryCard.test.ts src/lib/components/content/ContentList.test.ts
```

Expected: test collection fails because the components do not exist.

- [ ] **Step 3: Implement accessible compact cards and list interaction**

`ContentSummaryCard` props:

```typescript
interface Props {
  item: ContentSummary
  selected: boolean
  busy: boolean
  onSelect: (id: string) => void
  onToggleSaved: (item: ContentSummary) => void
  onCopy: (item: ContentSummary) => void
  onDelete: (item: ContentSummary) => void
}
```

Render in this visual order:

1. kind icon and one-line title at `0.82rem` or `var(--font-md)`;
2. two-line preview at `0.72rem` or `var(--font-sm)`;
3. metadata row containing human kind label and “临时保留至 …” only for temporary items;
4. right-aligned 32×32 minimum copy, favorite, and delete buttons.

Render buttons only from `item.capabilities`:

- choose the copy label/action from `copyText`, `copyImage`, `copyFile`, and `copyPath`;
- render 收藏 only when `save` is true and 取消收藏 only when `unsave` is true;
- render delete only when `delete` is true;
- show the drag handle only when both the list and `reorder` capability permit it.

Use `aria-pressed` for 收藏, `aria-current="true"` for selected cards, visible `:focus-visible` outlines, and real `button` elements. Sensitive values are never rendered in a summary.

`ContentList` receives:

```typescript
interface Props {
  items: ContentSummary[]
  selectedId: string | null
  reorderable: boolean
  onSelect: (id: string) => void
  onReorder: (orderedIds: string[]) => Promise<void>
  onToggleSaved: (item: ContentSummary) => void
  onCopy: (item: ContentSummary) => void
  onDelete: (item: ContentSummary) => void
}
```

ArrowUp/ArrowDown changes selection, Enter opens detail, and drag handles appear only when `reorderable` is true. Do not make the whole card draggable; this avoids accidental drags while selecting/copying.

- [ ] **Step 4: Run card/list tests and accessibility checks**

Run:

```powershell
pnpm exec vitest run src/lib/components/content/ContentSummaryCard.test.ts src/lib/components/content/ContentList.test.ts
pnpm check
```

Expected: component tests pass; no new Svelte accessibility warning is introduced.

- [ ] **Step 5: Commit unified cards**

```powershell
git add src/lib/components/content/ContentKindIcon.svelte src/lib/components/content/ContentSummaryCard.svelte src/lib/components/content/ContentSummaryCard.test.ts src/lib/components/content/ContentList.svelte src/lib/components/content/ContentList.test.ts
git commit -m "add unified content cards"
```

### Task 4: Add capability-driven details and editing

**Files:**
- Create: `src/lib/components/content/ContentDetail.svelte`
- Create: `src/lib/components/content/SimpleContentDetail.svelte`
- Create: `src/lib/components/content/StructuredContentDetail.svelte`
- Create: `src/lib/components/content/ContentDetail.test.ts`
- Modify: `src-tauri/src/content/service.rs`
- Modify: `src-tauri/src/content/ipc.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/content/ipc.rs`
- Modify: `src/lib/api/content.ts`
- Modify: `src/lib/api/dock.ts`
- Modify: `src/lib/api/vault.ts`

- [ ] **Step 1: Write failing detail dispatch tests**

```typescript
it.each([
  ['text', '编辑文本'],
  ['image', '复制图片'],
  ['file', '打开文件'],
  ['credential', '可直接使用的信息'],
  ['bookmark', '打开链接'],
  ['note', '编辑备注'],
] as const)('renders the useful primary action for %s', async (kind, actionName) => {
  const detail = detailFixture(kind)
  const view = render(ContentDetail, {
    props: detailProps(detail),
  })

  expect(view.getByRole('button', { name: actionName })).toBeVisible()
})

it('keeps credential copy buttons aligned at the far right', () => {
  const view = render(ContentDetail, {
    props: detailProps(detailFixture('credential')),
  })
  const rows = view.container.querySelectorAll('[data-field-row]')

  expect(rows.length).toBeGreaterThan(1)
  for (const row of rows) {
    expect(row.lastElementChild).toHaveAttribute('data-copy-action')
  }
})

it('shows a missing attachment state without disabling edit or delete', () => {
  const view = render(ContentDetail, {
    props: detailProps(detailFixture('file', { available: false })),
  })

  expect(view.getByText('文件不可用')).toBeVisible()
  expect(view.getByRole('button', { name: '重命名' })).toBeEnabled()
  expect(view.getByRole('button', { name: '删除' })).toBeEnabled()
  expect(view.getByRole('button', { name: '复制文件' })).toBeDisabled()
})
```

- [ ] **Step 2: Verify detail tests fail**

Run:

```powershell
pnpm exec vitest run src/lib/components/content/ContentDetail.test.ts
```

Expected: test collection fails because `ContentDetail.svelte` is missing.

- [ ] **Step 3: Implement detail adapters**

`ContentDetail` accepts:

```typescript
interface Props {
  detail: ContentDetail
  resetToken: string | number
  onClose: () => void
  onChanged: (id: string) => Promise<void>
  onNotify: (message: string, kind?: 'success' | 'error') => void
}
```

Dispatch on the tagged user-facing `detail.kind`:

```svelte
{#if detail.kind === 'text' || detail.kind === 'image' || detail.kind === 'file'}
  <SimpleContentDetail {detail} {onClose} {onChanged} {onNotify} />
{:else if detail.kind === 'credential' || detail.kind === 'bookmark' || detail.kind === 'note'}
  <StructuredContentDetail
    {detail}
    {resetToken}
    {onClose}
    {onChanged}
    {onNotify}
  />
{/if}
```

`SimpleContentDetail` reuses existing file/image clipboard commands. Editing never strips the opaque ID in TypeScript. Add backend wrappers:

```rust
#[tauri::command]
pub fn ipc_content_update_text(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    title: Option<String>,
    body: String,
) -> Result<ContentDetail, String>;

#[tauri::command]
pub fn ipc_content_rename(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    title: Option<String>,
) -> Result<ContentDetail, String>;

#[tauri::command]
pub fn ipc_content_update_structured(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    input: VaultEntryInput,
) -> Result<ContentDetail, String>;
```

Each command parses `UnifiedContentId` only in Rust, rejects an incompatible kind/source, delegates to the existing repository transaction hook, emits one `Updated` event, and returns the unified detail. Add `contentApi.updateText`, `rename`, and `updateStructured` with opaque string IDs.

`StructuredContentDetail` reuses `VaultEntryDetail` and `VaultEntryEditor` presentation. Increase readable detail text and copy targets using tokens:

```css
.detail-value {
  font-size: max(var(--font-md, 0.85rem), 0.85rem);
  line-height: 1.5;
}

.detail-copy {
  min-width: 2.25rem;
  min-height: 2.25rem;
  margin-inline-start: auto;
}
```

Title and directly useful fields appear before notes, tags, timestamps, and AI metadata. Password/sensitive fields remain masked after window blur and after selection changes.

For image/file details, check the backend-provided availability flag before copy/open. A missing attachment renders localized “图片不可用” or “文件不可用”, disables only attachment-dependent actions, and keeps rename/edit/retention/delete available.

- [ ] **Step 4: Run detail, legacy component, and type tests**

Run:

```powershell
pnpm exec vitest run src/lib/components/content/ContentDetail.test.ts src/lib/components/vault/CopyableValue.test.ts
pnpm check
```

Expected: all tests pass and there are no new type or accessibility errors.

- [ ] **Step 5: Commit unified details**

```powershell
git add src/lib/components/content/ContentDetail.svelte src/lib/components/content/SimpleContentDetail.svelte src/lib/components/content/StructuredContentDetail.svelte src/lib/components/content/ContentDetail.test.ts src-tauri/src/content/service.rs src-tauri/src/content/ipc.rs src-tauri/src/lib.rs src/lib/api/content.ts src/lib/api/dock.ts src/lib/api/vault.ts
git commit -m "add unified content details"
```

### Task 5: Build the responsive content workspace

**Files:**
- Create: `src/lib/components/content/ContentSearchBar.svelte`
- Create: `src/lib/components/content/ContentSearchBar.test.ts`
- Create: `src/lib/components/views/ContentWorkspace.svelte`
- Create: `src/lib/components/views/ContentWorkspace.test.ts`

- [ ] **Step 1: Write workspace mental-model tests**

```typescript
it('searches globally and restores browse state when cleared', async () => {
  const view = renderWorkspace({
    scope: 'saved',
    browseItems: [summary('dock:saved', 'saved')],
    searchHits: [hit('vault:match')],
  })
  const list = view.getByTestId('content-scroll')
  list.scrollTop = 180

  await fireEvent.input(view.getByRole('searchbox'), { target: { value: '生产' } })
  expect(await view.findByText('vault:match title')).toBeVisible()

  await fireEvent.click(view.getByRole('button', { name: '清除搜索' }))
  expect(view.getByText('dock:saved title')).toBeVisible()
  expect(list.scrollTop).toBe(180)
})

it('does not offer manual reorder in all or search results', () => {
  const allView = renderWorkspace({ scope: 'all' })
  expect(allView.queryByLabelText('拖动排序')).not.toBeInTheDocument()

  const searchView = renderWorkspace({ scope: 'saved', query: 'db' })
  expect(searchView.queryByLabelText('拖动排序')).not.toBeInTheDocument()
})

it('keeps lightweight capture primary in the temporary scope', () => {
  const view = renderWorkspace({ scope: 'temporary' })
  expect(view.getByRole('button', { name: '新建文本' })).toBeVisible()
  expect(view.getByText('也可直接粘贴或拖入文件')).toBeVisible()
})
```

- [ ] **Step 2: Verify workspace tests fail**

Run:

```powershell
pnpm exec vitest run src/lib/components/content/ContentSearchBar.test.ts src/lib/components/views/ContentWorkspace.test.ts
```

Expected: test collection fails because the search bar and workspace are missing.

- [ ] **Step 3: Implement the narrow-first workspace**

`ContentWorkspace` props:

```typescript
interface Props {
  browser: ContentBrowserState
  search: ContentSearchState
  selectedDetail: ContentDetail | null
  detailLoading: boolean
  onSearch: (query: string) => void
  onClearSearch: () => void
  onSelect: (id: string) => void
  onSetKind: (kind: ContentKind | null) => void
  onReorder: (ids: string[]) => Promise<void>
  onToggleSaved: (item: ContentSummary) => void
  onCopy: (item: ContentSummary) => void
  onDelete: (item: ContentSummary) => void
  onCreateText: () => void
  onDetailChanged: (id: string) => Promise<void>
  onNotify: (message: string, kind?: 'success' | 'error') => void
}
```

Layout:

- sticky 40px search row below TopBar;
- optional horizontal kind chips: 全部类型/文本/图片/文件/凭据/书签/笔记;
- compact context line with result count or temporary cleanup explanation;
- one scrolling list;
- selected detail opens as a full-height in-shell layer at widths below `680px`;
- at widths `>= 680px`, list and detail use `minmax(280px, 0.9fr) minmax(340px, 1.1fr)`;
- empty states explain the next useful action instead of only saying “empty”;
- search results show the same cards and retain source-neutral labels.

Store `scopeScrollTop: Record<BrowseScope, number>` and `preSearchSelection` inside the workspace. On the first non-empty query, capture scope scroll/selection. On clear, restore both after `tick()`.

- [ ] **Step 4: Run workspace tests and responsive static checks**

Run:

```powershell
pnpm exec vitest run src/lib/components/content/ContentSearchBar.test.ts src/lib/components/views/ContentWorkspace.test.ts
pnpm check
```

Expected: search/restore, reorder visibility, empty-state, keyboard, and responsive class tests pass.

- [ ] **Step 5: Commit the workspace**

```powershell
git add src/lib/components/content/ContentSearchBar.svelte src/lib/components/content/ContentSearchBar.test.ts src/lib/components/views/ContentWorkspace.svelte src/lib/components/views/ContentWorkspace.test.ts
git commit -m "add unified main content workspace"
```

### Task 6: Replace App-level fragmented state and synchronize revisions

**Files:**
- Modify: `src/App.svelte`
- Create: `src/App.test.ts`
- Modify: `src/lib/api/content.ts`
- Modify: `src/lib/state/content-browser.ts`

- [ ] **Step 1: Write App integration tests**

Mock window and IPC modules, then assert:

```typescript
it('maps main navigation to unified browse scopes', async () => {
  const view = render(App)
  await waitFor(() => expect(contentApi.list).toHaveBeenCalledWith('temporary', null))

  await fireEvent.click(view.getByRole('button', { name: '全部' }))
  expect(contentApi.list).toHaveBeenLastCalledWith('all', null)

  await fireEvent.click(view.getByRole('button', { name: '收藏' }))
  expect(contentApi.list).toHaveBeenLastCalledWith('saved', null)
})

it('refreshes after content events and repairs a missed event on focus', async () => {
  render(App)
  emitContentChanged({ revision: 3, changes: [{ id: 'vault:new', operation: 'created' }] })
  await waitFor(() => expect(contentApi.list).toHaveBeenCalledTimes(2))

  contentApi.revision.mockResolvedValueOnce({ revision: 5 })
  window.dispatchEvent(new Event('focus'))
  await waitFor(() => expect(contentApi.revision).toHaveBeenCalled())
})

it('clears a remotely deleted selection with an explicit notice', async () => {
  const view = render(App)
  await selectItem(view, 'vault:remote')
  emitContentChanged({
    revision: 6,
    changes: [{ id: 'vault:remote', operation: 'deleted' }],
  })

  await waitFor(() => expect(view.queryByText('vault:remote title')).not.toBeInTheDocument())
  expect(view.getByRole('status')).toHaveTextContent('该内容已在另一窗口删除')
})

it('deletes with a backend undo token and restores from the toast', async () => {
  const view = render(App)
  await selectItem(view, 'dock:a')
  await fireEvent.click(view.getByRole('button', { name: '删除' }))
  expect(contentApi.delete).toHaveBeenCalledWith('dock:a')
  expect(view.queryByText('dock:a title')).not.toBeInTheDocument()

  await fireEvent.click(view.getByRole('button', { name: '撤销' }))
  expect(contentApi.restore).toHaveBeenCalledWith('undo-token')
  expect(await view.findByText('dock:a title')).toBeVisible()
})

it('restores an optimistic row when deferred delete commit fails', async () => {
  const view = render(App)
  await selectItem(view, 'vault:b')
  await fireEvent.click(view.getByRole('button', { name: '删除' }))
  emitContentDeleteFailed({
    token: 'undo-token',
    id: 'vault:b',
    code: 'content_delete_commit_failed',
  })

  expect(await view.findByText('vault:b title')).toBeVisible()
  expect(view.getByRole('status')).toHaveTextContent('删除失败')
})
```

- [ ] **Step 2: Verify App tests fail**

Run:

```powershell
pnpm exec vitest run src/App.test.ts
```

Expected: assertions fail because `App.svelte` still owns separate Home/Note/Vault lists and routes.

- [ ] **Step 3: Replace content state with the unified controllers**

Use:

```typescript
type MainView = BrowseScope | 'settings'

let currentView = $state<MainView>('temporary')
let browserState = $state<ContentBrowserState>(initialBrowserState())
let searchState = $state<ContentSearchState>(initialSearchState())
let selectedDetail = $state<ContentDetail | null>(null)
let detailRequest = 0
let pendingDeleteIds = $state<string[]>([])

let visibleBrowserState = $derived({
  ...browserState,
  items: browserState.items.filter((item) => !pendingDeleteIds.includes(item.id)),
})
let visibleSearchState = $derived({
  ...searchState,
  hits: searchState.hits.filter(
    (hit) => !pendingDeleteIds.includes(hit.summary.id),
  ),
})

const browser = new ContentBrowserController(contentApi, (state) => {
  browserState = state
})
const search = new UnifiedSearchController(contentApi, (state) => {
  searchState = state
})
```

On mount:

1. load preferences and `browser.load('temporary')` in parallel;
2. subscribe to `onContentChanged` and `onContentDeleteFailed`;
3. if event revision is greater than local revision, refresh the active browser and rerun a non-empty search;
   when a `Deleted` change matches the selected ID and is not this window's pending deletion, close detail and show the localized remote-deletion notice;
4. add window `focus` and Tauri window `tauri://focus` handlers that call `refreshIfStale`;
5. load selected detail with a request counter so late detail responses cannot replace the new selection.

Mutation handlers:

- save/unsave through `contentApi`, then refresh active browse/search/detail;
- delete first adds the ID to `pendingDeleteIds`, then requests a backend pending token and shows the existing toast Undo action;
- if token preparation fails, remove the ID from `pendingDeleteIds` immediately and report the failure;
- restore consumes the token, removes the ID from `pendingDeleteIds`, and refreshes;
- a committed `Deleted` content event removes the ID from `pendingDeleteIds` and refreshes;
- `content-delete-failed` removes the ID from `pendingDeleteIds`, refreshes the still-existing row, and maps its stable code to localized “删除失败，内容已恢复” copy;
- reorder through browser controller;
- summary copy loads detail when the useful copy value is not present in summary;
- text/image/file paste and drag still call Dock capture adapters with Home/temporary membership, then rely on `content-changed` rather than manually splicing arrays.

Destroy controllers and all event listeners in `onDestroy`. Keep QuickAccessFab and SettingsView in the same shell.

- [ ] **Step 4: Run App, state, and full frontend tests**

Run:

```powershell
pnpm exec vitest run src/App.test.ts src/lib/state/content-browser.test.ts src/lib/state/content-search.test.ts
pnpm test:unit
pnpm check
```

Expected: App integration tests pass, all unit tests pass, and `svelte-check` has zero errors.

- [ ] **Step 5: Commit App consolidation**

```powershell
git add src/App.svelte src/App.test.ts src/lib/api/content.ts src/lib/state/content-browser.ts
git commit -m "unify main window content state"
```

### Task 7: Simplify navigation and product language

**Files:**
- Modify: `src/lib/components/TopBar.svelte`
- Create: `src/lib/components/TopBar.test.ts`
- Modify: `src/lib/i18n/types.ts`
- Modify: `src/lib/i18n/locales/zh-CN.ts`
- Modify: `src/lib/i18n/locales/en.ts`
- Modify: `src/App.svelte`

- [ ] **Step 1: Write navigation and terminology tests**

```typescript
it('shows one lifecycle navigation without a library silo', () => {
  const view = render(TopBar, { props: topBarProps('temporary') })

  expect(view.getByRole('button', { name: '收纳' })).toBeVisible()
  expect(view.getByRole('button', { name: '全部' })).toBeVisible()
  expect(view.getByRole('button', { name: '收藏' })).toBeVisible()
  expect(view.queryByRole('button', { name: '资料库' })).not.toBeInTheDocument()
})

it('marks exactly one browse scope as current', () => {
  const view = render(TopBar, { props: topBarProps('saved') })
  const current = view.container.querySelectorAll('[aria-current="page"]')
  expect(current).toHaveLength(1)
  expect(current[0]).toHaveTextContent('收藏')
})
```

Extend i18n tests to assert both locales contain:

```typescript
expect(messages.workspace.scope.temporary).toBeTruthy()
expect(messages.workspace.scope.all).toBeTruthy()
expect(messages.workspace.scope.saved).toBeTruthy()
expect(messages.workspace.searchPlaceholder).toBeTruthy()
expect(messages.workspace.temporaryRetention).toBeTruthy()
```

- [ ] **Step 2: Verify navigation tests fail**

Run:

```powershell
pnpm exec vitest run src/lib/components/TopBar.test.ts src/lib/i18n/__tests__/i18n.test.ts
```

Expected: TopBar test fails because the existing 资料库 tab is still rendered.

- [ ] **Step 3: Implement the four-destination navigation**

Change TopBar props to:

```typescript
interface Props {
  currentView: BrowseScope | 'settings'
  onNavigate: (view: BrowseScope) => void
  onToggleSettings: () => void
  onMinimize: () => void
}
```

Render only 收纳, 全部, 收藏 as content destinations and the existing settings/minimize controls. Keep each content tab at least 40px high at the minimum window width; use equal flexible widths and text labels, not icon-only navigation.

Add `workspace` messages for scope names, all six kinds, search/error/empty states, retention explanation, create, open, copy, save/unsave, delete/undo, and details. Replace user-visible “保存到资料库” with “保存” or “已收藏” according to context. Retain `library` translation keys only while old Quick Access components still compile; remove them in the third plan.

- [ ] **Step 4: Run navigation, locale, and full frontend gates**

Run:

```powershell
pnpm exec vitest run src/lib/components/TopBar.test.ts src/lib/i18n/__tests__/i18n.test.ts
pnpm test:unit
pnpm check
pnpm build
```

Expected: all tests, type checking, and build pass; the main TopBar has no 资料库 destination.

- [ ] **Step 5: Commit product-language consolidation**

```powershell
git add src/lib/components/TopBar.svelte src/lib/components/TopBar.test.ts src/lib/i18n/types.ts src/lib/i18n/locales/zh-CN.ts src/lib/i18n/locales/en.ts src/App.svelte
git commit -m "simplify main content navigation"
```

### Task 8: Validate main-window user journeys at real Windows sizes

**Files:**
- Create: `docs/superpowers/reports/2026-07-18-unified-main-workspace-verification.md`
- Modify: only files whose observed behavior fails the matrix

- [ ] **Step 1: Add a deterministic UI fixture mode**

Add an App test fixture helper, not a production route, that supplies six summaries and corresponding details:

```typescript
export const allKindSummaries: ContentSummary[] = [
  summary('dock:text', 'temporary', { kind: 'text', title: '会议临时记录' }),
  summary('dock:image', 'temporary', { kind: 'image', title: '错误截图' }),
  summary('dock:file', 'temporary', { kind: 'file', title: '上线清单.pdf' }),
  summary('vault:credential', 'saved', { kind: 'credential', title: '生产数据库' }),
  summary('vault:bookmark', 'saved', { kind: 'bookmark', title: '运维控制台' }),
  summary('vault:note', 'saved', { kind: 'note', title: '发布说明' }),
]
```

Use it in Testing Library tests for 240px and 360px CSS viewport widths; assert no horizontal document overflow and every primary action remains keyboard reachable.

- [ ] **Step 2: Run automated responsive tests before manual validation**

Run:

```powershell
pnpm test:unit
pnpm check
pnpm build
Push-Location src-tauri
cargo test
Pop-Location
```

Expected: all automated gates pass before launching the desktop runtime.

- [ ] **Step 3: Execute the Windows journey matrix**

Run `pnpm tauri dev` and verify each row with a clean copied database and with an upgraded pre-foundation database:

| Size | Journey | Expected observable result |
|---|---|---|
| 360×640 | paste text → 收纳 → 收藏 | new text appears first in 收纳; 收藏 moves it to saved without duplication |
| 360×640 | 收藏 → 取消收藏 | item returns to 收纳 and shows temporary cleanup timing |
| 360×640 | search old Dock text | global result appears even when active scope is 收藏 |
| 360×640 | search credential → copy password | useful fields precede notes; copy buttons align right; sensitive value resets on blur |
| 360×640 | clear search | previous scope, selection, and scroll position return |
| 240×180 | navigate/search/open detail | no horizontal overflow; controls remain reachable; detail uses in-shell layer |
| 720×640 | select result | list/detail split view appears without changing content order |
| any | delete → undo within 10s | exact content and retention return |
| any | delete → undo after expiry | localized expiry message appears; content remains deleted |
| any | hide/show or lose one event | focus revision check refreshes content |

Also switch each existing theme and both languages while content and detail are open. The workspace must use the same token set immediately without remounting or losing search text.

- [ ] **Step 4: Record actual evidence and rerun final gates**

Write the report with:

- database fixture source and row counts;
- each matrix row marked pass with observed text/action;
- screenshots at 240×180, 360×640, and 720×640;
- theme/language switch results;
- all command result summaries;
- any intentionally retained legacy files and why they remain until plan 3.

Then run:

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

Expected: all gates pass and no unreviewed runtime process remains after validation.

- [ ] **Step 5: Commit main-workspace verification**

```powershell
git add docs/superpowers/reports/2026-07-18-unified-main-workspace-verification.md
git commit -m "verify unified main workspace"
```

## Plan acceptance gate

Do not start Quick Access consolidation until:

- 收纳/全部/收藏 show the same six content types through one workspace.
- 资料库 is no longer a main navigation destination.
- Global search includes temporary and saved content and clears back to prior context.
- Existing lightweight paste/drag/+ capture remains temporary-first.
- One 收藏/取消收藏 action works for both storage sources.
- Main details prioritize useful information, enlarge readable values/actions, and align copy controls.
- Event refresh and focus revision repair both work.
- Default, minimum, and expanded window sizes pass the journey matrix.
- Theme and locale changes do not reset content/search state.
