<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import TopBar from "$lib/components/TopBar.svelte";
  import QuickAccessFab from "$lib/components/QuickAccessFab.svelte";
  import ContentWorkspace from "$lib/components/views/ContentWorkspace.svelte";
  import SettingsView from "$lib/components/views/SettingsView.svelte";
  import {
    contentApi,
    onContentChanged,
    onContentDeleteFailed,
  } from "$lib/api/content";
  import { dockApi } from "$lib/api/dock";
  import {
    ContentBrowserController,
    type ContentBrowserState,
  } from "$lib/state/content-browser";
  import {
    UnifiedSearchController,
    initialSearchState,
    type ContentSearchState,
  } from "$lib/state/content-search";
  import { broadcastPreferences } from "$lib/state/preferences-sync";
  import { computeThemeTokens } from "$lib/themes/engine";
  import { messages, loadLocale, detectLanguage } from "$lib/i18n";
  import type {
    BrowseScope,
    ContentDetail,
    ContentKind,
    ContentSummary,
  } from "$lib/types/content";
  import type { DockPreferences } from "$lib/types/dock";

  type MainView = BrowseScope | "settings";
  let currentView = $state<MainView>("temporary");
  let lastScope = $state<BrowseScope>("temporary");
  let browserState = $state<ContentBrowserState>({
    scope: "temporary",
    kind: null,
    items: [],
    selectedId: null,
    revision: 0,
    phase: "idle",
    error: null,
  });
  let searchState = $state<ContentSearchState>(initialSearchState());
  let selectedDetail = $state<ContentDetail | null>(null);
  let detailLoading = $state(false);
  let detailRequest = 0;
  let pendingDeleteIds = $state<string[]>([]);
  let preferences = $state<DockPreferences | null>(null);
  let systemDark = $state(
    window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false,
  );
  let toast = $state<{
    text: string;
    kind: "success" | "error";
    undo?: () => void;
  } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  let quickAccessOpening = $state(false);
  let composeOpen = $state(false);
  let composeText = $state("");
  let dragOverlay = $state(false);
  const browser = new ContentBrowserController(
    contentApi,
    (s) => (browserState = s),
  );
  const search = new UnifiedSearchController(
    contentApi,
    (s) => (searchState = s),
  );
  const visibleBrowser = $derived({
    ...browserState,
    items: browserState.items.filter((x) => !pendingDeleteIds.includes(x.id)),
  });
  const visibleSearch = $derived({
    ...searchState,
    hits: searchState.hits.filter(
      (x) => !pendingDeleteIds.includes(x.summary.id),
    ),
  });

  onMount(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    void Promise.all([dockApi.getPreferences(), browser.load("temporary")])
      .then(([prefs]) => {
        if (disposed) return;
        preferences = prefs;
        if (!prefs.language) {
          prefs = { ...prefs, language: detectLanguage() };
          preferences = prefs;
          void dockApi.setPreferences(prefs);
        }
        loadLocale(prefs.language);
      })
      .catch((e) => notify(`加载失败：${format(e)}`, "error"));
    void onContentChanged(async (event) => {
      if (event.revision <= browser.snapshot.revision) return;
      const selected = selectedDetail?.summary.id;
      const remoteDeleted =
        selected &&
        event.changes.some(
          (c) => c.id === selected && c.operation === "deleted",
        ) &&
        !pendingDeleteIds.includes(selected);
      if (remoteDeleted) {
        selectedDetail = null;
        browser.select(null);
        search.select(null);
        notify("该内容已在另一窗口删除", "error");
      }
      for (const c of event.changes)
        if (c.operation === "deleted")
          pendingDeleteIds = pendingDeleteIds.filter((id) => id !== c.id);
      await refreshAll();
    }).then((fn) => (disposed ? fn() : cleanups.push(fn)));
    void onContentDeleteFailed(async (e) => {
      pendingDeleteIds = pendingDeleteIds.filter((id) => id !== e.id);
      await refreshAll();
      notify("删除失败，内容已恢复", "error");
    }).then((fn) => (disposed ? fn() : cleanups.push(fn)));
    const focus = () =>
      void browser.refreshIfStale().then((stale) => {
        if (stale) return refreshSearch();
      });
    window.addEventListener("focus", focus);
    window.addEventListener("paste", handlePaste);
    cleanups.push(
      () => window.removeEventListener("focus", focus),
      () => window.removeEventListener("paste", handlePaste),
    );
    void import("@tauri-apps/api/window")
      .then(async ({ getCurrentWindow }) => {
        const un = await getCurrentWindow().onDragDropEvent((e: any) => {
          if (e.payload.type === "enter") dragOverlay = true;
          else if (e.payload.type === "leave") dragOverlay = false;
          else if (e.payload.type === "drop") {
            dragOverlay = false;
            void importPaths(e.payload.paths);
          }
        });
        if (disposed) un();
        else cleanups.push(un);
      })
      .catch(() => {});
    return () => {
      disposed = true;
      for (const c of cleanups) c();
    };
  });
  onMount(() => {
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!mq) return;
    const h = (e: MediaQueryListEvent) => (systemDark = e.matches);
    mq.addEventListener("change", h);
    return () => mq.removeEventListener("change", h);
  });
  onDestroy(() => {
    browser.dispose?.();
    search.dispose();
    if (toastTimer) clearTimeout(toastTimer);
  });
  $effect(() => {
    if (!preferences) return;
    const root = document.documentElement.style;
    for (const [k, v] of Object.entries(
      computeThemeTokens(preferences, systemDark),
    ))
      root.setProperty(k, v);
    root.setProperty("--font-family-zh", preferences.fontFamilyZh);
    root.setProperty("--font-family-en", preferences.fontFamilyEn);
  });

  async function navigate(scope: BrowseScope) {
    currentView = scope;
    lastScope = scope;
    selectedDetail = null;
    await browser.load(scope);
  }
  function toggleSettings() {
    currentView = currentView === "settings" ? lastScope : "settings";
  }
  async function minimize() {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().hide();
    } catch (e) {
      notify(format(e), "error");
    }
  }
  async function openQuickAccess() {
    if (quickAccessOpening) return;
    quickAccessOpening = true;
    try {
      await import("@tauri-apps/api/core").then(({ invoke }) =>
        invoke("ipc_quick_access_open"),
      );
    } catch (e) {
      notify(format(e), "error");
    } finally {
      quickAccessOpening = false;
    }
  }
  async function select(id: string | null) {
    if (searchState.query.trim()) search.select(id);
    else browser.select(id);
    if (!id) {
      selectedDetail = null;
      return;
    }
    const request = ++detailRequest;
    detailLoading = true;
    try {
      const detail = await contentApi.detail(id);
      if (request === detailRequest) selectedDetail = detail;
    } catch (e) {
      if (request === detailRequest) {
        selectedDetail = null;
        notify(`详情加载失败：${format(e)}`, "error");
      }
    } finally {
      if (request === detailRequest) detailLoading = false;
    }
  }
  function doSearch(q: string) {
    void search.search(q);
  }
  function clearSearch() {
    void search.search("");
  }
  async function setKind(kind: ContentKind | null) {
    search.setKinds(kind ? [kind] : []);
    await browser.setKind(kind);
    if (searchState.query.trim()) await search.search(searchState.query);
  }
  async function refreshSearch() {
    if (searchState.query.trim()) await search.search(searchState.query);
  }
  async function refreshAll() {
    await browser.refresh();
    await refreshSearch();
    if (selectedDetail) await reloadDetail(selectedDetail.summary.id);
  }
  async function reloadDetail(id: string) {
    const request = ++detailRequest;
    try {
      const d = await contentApi.detail(id);
      if (request === detailRequest) selectedDetail = d;
    } catch {
      if (request === detailRequest) selectedDetail = null;
    }
  }
  async function toggleSaved(item: ContentSummary) {
    try {
      item.retention === "saved"
        ? await contentApi.unsave(item.id)
        : await contentApi.save(item.id);
      await refreshAll();
      notify(item.retention === "saved" ? "已取消收藏" : "已收藏");
    } catch (e) {
      notify(format(e), "error");
    }
  }
  async function copySummary(item: ContentSummary) {
    try {
      const d = await contentApi.detail(item.id);
      if (d.kind === "text") await navigator.clipboard.writeText(d.body);
      else if (d.kind === "image" && d.available)
        await dockApi.copyImage(d.assetPath);
      else if (d.kind === "file" && d.available)
        await dockApi.copyFile(d.assetPath);
      else if (d.kind === "bookmark")
        await navigator.clipboard.writeText(d.url);
      else if (d.kind === "note") await navigator.clipboard.writeText(d.body);
      else if (d.kind === "credential") {
        const first = d.fields.find((f) => !f.isSensitive) ?? d.fields[0];
        if (first) await navigator.clipboard.writeText(first.value);
      }
      notify("已复制");
    } catch (e) {
      notify(format(e), "error");
    }
  }
  async function remove(item: ContentSummary) {
    if (pendingDeleteIds.includes(item.id)) return;
    pendingDeleteIds = [...pendingDeleteIds, item.id];
    if (selectedDetail?.summary.id === item.id) selectedDetail = null;
    try {
      const token = await contentApi.delete(item.id);
      notify("已删除", "success", async () => {
        try {
          await contentApi.restore(token.token);
          pendingDeleteIds = pendingDeleteIds.filter((id) => id !== item.id);
          await refreshAll();
          notify("已恢复");
        } catch (e) {
          notify(`撤销失败：${format(e)}`, "error");
        }
      });
    } catch (e) {
      pendingDeleteIds = pendingDeleteIds.filter((id) => id !== item.id);
      notify(`删除失败：${format(e)}`, "error");
    }
  }
  async function saveText() {
    const text = composeText.trim();
    if (!text) return;
    try {
      await dockApi.createText("home", text, "manual");
      composeOpen = false;
      composeText = "";
      notify("已收纳");
      setTimeout(() => void refreshAll(), 0);
    } catch (e) {
      notify(format(e), "error");
    }
  }
  async function handlePaste(e: ClipboardEvent) {
    const target = e.target as HTMLElement;
    if (target?.closest('input,textarea,[contenteditable="true"]')) return;
    const files = [...(e.clipboardData?.files ?? [])];
    const text = e.clipboardData?.getData("text/plain")?.trim();
    if (files.length) {
      e.preventDefault();
      for (const f of files)
        f.type.startsWith("image/")
          ? await dockApi.importImageBlob(
              f,
              f.name || `pasted-${Date.now()}.png`,
              "home",
            )
          : await dockApi.importFileBlob(
              f,
              f.name || `file-${Date.now()}`,
              "home",
            );
      notify("已收纳");
      setTimeout(() => void refreshAll(), 0);
    } else if (text) {
      e.preventDefault();
      await dockApi.createText("home", text, "manual");
      notify("已收纳");
      setTimeout(() => void refreshAll(), 0);
    }
  }
  async function importPaths(paths: string[]) {
    try {
      for (const p of paths) await dockApi.importFile(p, "home");
      notify(`已收纳 ${paths.length} 个文件`);
      setTimeout(() => void refreshAll(), 0);
    } catch (e) {
      notify(format(e), "error");
    }
  }
  function notify(
    text: string,
    kind: "success" | "error" = "success",
    undo?: () => void,
  ) {
    if (toastTimer) clearTimeout(toastTimer);
    toast = { text, kind, undo };
    toastTimer = setTimeout(() => (toast = null), 10000);
  }
  function format(e: unknown) {
    return e instanceof Error
      ? e.message
      : typeof e === "string"
        ? e
        : "未知错误";
  }
  async function updatePreferences(next: DockPreferences) {
    const previous = preferences?.language;
    preferences = next;
    void broadcastPreferences(next).catch(() => {});
    if (previous !== next.language) loadLocale(next.language);
    try {
      await dockApi.setPreferences(next);
    } catch (e) {
      notify(format(e), "error");
    }
  }
</script>

<div class="app-shell">
  <TopBar
    {currentView}
    onNavigate={navigate}
    onToggleSettings={toggleSettings}
    onMinimize={minimize}
  />{#if currentView === "settings" && preferences}<SettingsView
      {preferences}
      onChange={updatePreferences}
      onBack={() => (currentView = lastScope)}
      {notify}
    />{:else}<ContentWorkspace
      browser={visibleBrowser}
      search={visibleSearch}
      {selectedDetail}
      {detailLoading}
      {pendingDeleteIds}
      onSearch={doSearch}
      onClearSearch={clearSearch}
      onSelect={select}
      onSetKind={setKind}
      onReorder={(ids) => browser.reorder(ids)}
      onToggleSaved={toggleSaved}
      onCopy={copySummary}
      onDelete={remove}
      onCreateText={() => (composeOpen = true)}
      onDetailChanged={async (id) => {
        await refreshAll();
        await select(id);
      }}
      onNotify={notify}
    />{/if}<QuickAccessFab
    onOpen={openQuickAccess}
    disabled={quickAccessOpening}
  />
  {#if composeOpen}<div class="compose" role="dialog" aria-label="新建文本">
      <textarea
        bind:value={composeText}
        placeholder="输入要收纳的内容"
      ></textarea>
      <div>
        <button type="button" onclick={() => (composeOpen = false)}>取消</button
        ><button type="button" onclick={saveText}>收纳</button>
      </div>
    </div>{/if}
  {#if toast}<div
      class="toast"
      class:error={toast.kind === "error"}
      role="status"
    >
      <span>{toast.text}</span>{#if toast.undo}<button
          type="button"
          onclick={toast.undo}>撤销</button
        >{/if}
    </div>{/if}{#if dragOverlay}<div class="drag-overlay">
      松开即可收纳文件
    </div>{/if}
</div>

<style>
  .app-shell {
    box-sizing: border-box;
    width: 100vw;
    height: 100vh;
    min-width: 240px;
    min-height: 180px;
    position: relative;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface-0);
    color: var(--text-primary);
    border: 1px solid var(--border-emphasis);
    font-family: var(--font-family-zh), var(--font-family-en), sans-serif;
  }
  .toast {
    position: absolute;
    left: 50%;
    bottom: 0.75rem;
    z-index: 110;
    max-width: calc(100% - 1rem);
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.45rem 0.7rem;
    border: 1px solid var(--color-primary);
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-primary);
    font-size: var(--font-sm);
    transform: translateX(-50%);
    box-shadow: var(--shadow-default);
  }
  .toast.error {
    border-color: var(--color-danger);
  }
  .toast button {
    border: 0;
    background: none;
    color: var(--color-primary);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  .compose {
    position: absolute;
    inset: auto 0.6rem 0.6rem;
    z-index: 100;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    padding: 0.6rem;
    border: 1px solid var(--border-emphasis);
    border-radius: var(--radius-lg);
    background: var(--surface-1);
    box-shadow: var(--shadow-default);
  }
  .compose textarea {
    box-sizing: border-box;
    width: 100%;
    min-height: 7rem;
    padding: 0.55rem;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--surface-0);
    color: var(--text-primary);
    font: inherit;
    resize: vertical;
  }
  .compose div {
    display: flex;
    justify-content: flex-end;
    gap: 0.4rem;
  }
  .compose button {
    min-height: 2.2rem;
    padding: 0.35rem 0.7rem;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text-primary);
    font: inherit;
  }
  .drag-overlay {
    position: absolute;
    inset: 0.5rem;
    z-index: 120;
    display: grid;
    place-items: center;
    border: 2px dashed var(--color-primary);
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--surface-0) 90%, transparent);
    font-weight: 700;
  }
  @media (max-width: 260px) {
    .toast {
      bottom: 0.25rem;
    }
    .compose {
      inset: 0.2rem;
    }
    .compose textarea {
      min-height: 4rem;
    }
  }
</style>
