<script lang="ts">
  import { onMount } from "svelte";
  import {
    createContentBrowser,
    type ContentBrowserState,
  } from "$lib/state/content-browser";
  import {
    createContentSearch,
    type ContentSearchState,
  } from "$lib/state/content-search";
  import { contentApi } from "$lib/api/content";
  import { dockApi } from "$lib/api/dock";
  import {
    onContentChanged,
    onContentDeleteFailed,
    onPreferencesChanged,
    onDockView,
  } from "$lib/api/events";
  import { applyTokens } from "$lib/themes/engine";
  import TopBar from "$lib/components/TopBar.svelte";
  import ContentWorkspace from "$lib/components/views/ContentWorkspace.svelte";
  import SettingsView from "$lib/components/views/SettingsView.svelte";
  import MinimizedTab from "$lib/components/MinimizedTab.svelte";
  import QuickAccessFab from "$lib/components/QuickAccessFab.svelte";
  import type {
    BrowseScope,
    ContentDetail,
    ContentKind,
    ContentSummary,
  } from "$lib/types/content";
  import type { DockPreferences } from "$lib/types/dock";
  import { computeThemeTokens } from "$lib/themes/engine";
  import { messages, loadLocale, detectLanguage } from "$lib/i18n";
  import Icon from "$lib/components/Icon.svelte";

  const browser: ContentBrowserState = createContentBrowser();
  const search: ContentSearchState = createContentSearch();
  let currentView = $state<BrowseScope | "settings">("temporary");
  let preferences = $state<DockPreferences | null>(null);
  let systemDark = $state(true);
  let isMac = $state(false);
  let minimized = $state(false);
  let quickAccessOpening = $state(false);
  let selectedDetail = $state<ContentDetail | null>(null);
  let detailLoading = $state(false);
  let detailRequest = 0;
  let composeOpen = $state(false);
  let composeText = $state("");
  let toast = $state<{
    text: string;
    kind: "success" | "error";
    undo?: () => void;
  } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let dragOverlay = $state(false);
  let pendingDeleteIds = $state<string[]>([]);
  let loadGeneration = 0;

  async function refreshAll() {
    const generation = ++loadGeneration;
    const items = await contentApi.list(browser.scope, browser.kind);
    if (generation !== loadGeneration) return;
    browser.syncFromBackend(items, browser.kind);
    await search.refresh();
    if (selectedDetail) {
      try {
        selectedDetail = await contentApi.detail(selectedDetail.summary.id);
      } catch {
        selectedDetail = null;
      }
    }
  }

  onMount(() => {
    let disposed = false;
    const cleanups: Array<() => void> = [];
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const syncSystemDark = () => (systemDark = media.matches);
    syncSystemDark();
    media.addEventListener("change", syncSystemDark);
    isMac = navigator.userAgent.includes("Mac");

    async function initialize() {
      preferences = await dockApi.getPreferences();
      minimized = preferences.dockMinimized;
      if (preferences.language !== "auto") {
        await loadLocale(preferences.language);
      } else {
        await loadLocale(detectLanguage());
      }
      await refreshAll();
    }

    void initialize()
      .catch((e) => notify(`${messages.toast.loadFailed}：${format(e)}`, "error"));
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
        notify(messages.workspace.notices.deletedRemotely, "error");
      }
      await refreshAll();
    }).then((fn) => (disposed ? fn() : cleanups.push(fn)));
    void onContentDeleteFailed(async (e) => {
      pendingDeleteIds = pendingDeleteIds.filter((id) => id !== e.id);
      await refreshAll();
      notify(messages.workspace.notices.deleteFailedRestored, "error");
    }).then((fn) => (disposed ? fn() : cleanups.push(fn)));
    void onPreferencesChanged(async (p) => {
      preferences = p;
      if (p.language !== "auto") await loadLocale(p.language);
    }).then((fn) => (disposed ? fn() : cleanups.push(fn)));
    void onDockView(async (view) => {
      if (view === "settings") {
        currentView = "settings";
      } else {
        currentView = "temporary";
        browser.setScope("temporary");
      }
      minimized = false;
      await refreshAll();
    }).then((fn) => (disposed ? fn() : cleanups.push(fn)));

    return () => {
      disposed = true;
      media.removeEventListener("change", syncSystemDark);
      cleanups.forEach((fn) => fn());
    };
  });

  const tokens = $derived(
    preferences ? computeThemeTokens(preferences, systemDark) : {},
  );
  $effect(() => {
    applyTokens(tokens);
  });
  $effect(() => {
    document.documentElement.style.setProperty(
      "--font-ui-size",
      `${preferences?.uiTextSizePx ?? 12}px`,
    );
    document.documentElement.style.setProperty(
      "--font-content-size",
      `${preferences?.contentTextSizePx ?? 14}px`,
    );
    document.documentElement.style.setProperty(
      "--font-family-zh",
      preferences?.fontFamilyZh || "Microsoft YaHei",
    );
    document.documentElement.style.setProperty(
      "--font-family-en",
      preferences?.fontFamilyEn || "Segoe UI",
    );
  });

  function navigate(view: BrowseScope) {
    currentView = view;
    browser.setScope(view);
    selectedDetail = null;
    void refreshAll();
  }
  function toggleSettings() {
    currentView = currentView === "settings" ? browser.scope : "settings";
  }
  async function minimize() {
    try {
      await dockApi.minimizeToTab();
    } catch (e) {
      notify(format(e), "error");
    }
  }
  async function openQuickAccess() {
    if (quickAccessOpening) return;
    quickAccessOpening = true;
    try {
      await dockApi.openQuickAccess();
    } catch (e) {
      notify(format(e), "error");
    } finally {
      quickAccessOpening = false;
    }
  }
  async function select(id: string | null) {
    browser.select(id);
    search.select(id);
    selectedDetail = null;
    if (!id) return;
    const request = ++detailRequest;
    detailLoading = true;
    try {
      const d = await contentApi.detail(id);
      if (request === detailRequest) selectedDetail = d;
    } catch (e) {
      if (request === detailRequest) {
        selectedDetail = null;
        notify(`${messages.workspace.notices.detailLoadFailed}：${format(e)}`, "error");
      }
    } finally {
      if (request === detailRequest) detailLoading = false;
    }
  }
  async function toggleSaved(item: ContentSummary) {
    try {
      item.retention === "saved"
        ? await contentApi.unsave(item.id)
        : await contentApi.save(item.id);
      await refreshAll();
      notify(
        item.retention === "saved"
          ? messages.workspace.notices.unsaved
          : messages.workspace.notices.saved,
      );
    } catch (e) {
      notify(format(e), "error");
    }
  }
  async function copyItem(item: ContentSummary) {
    try {
      const d = await contentApi.detail(item.id);
      if (d.kind === "text") await navigator.clipboard.writeText(d.body);
      else if (d.kind === "image" && d.available)
        await dockApi.copyImage(d.assetPath);
      else if (d.kind === "file" && d.available)
        await dockApi.copyFile(d.assetPath);
      else if (d.kind === "bookmark") await navigator.clipboard.writeText(d.url);
      else if (d.kind === "note") await navigator.clipboard.writeText(d.body);
      else if (d.kind === "credential") {
        const first = d.fields.find((f) => !f.isSensitive) ?? d.fields[0];
        if (first) await navigator.clipboard.writeText(first.value);
      }
      notify(messages.toast.copied);
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
      notify(messages.workspace.notices.deleted, "success", async () => {
        try {
          await contentApi.restore(token.token);
          pendingDeleteIds = pendingDeleteIds.filter((id) => id !== item.id);
          await refreshAll();
          notify(messages.workspace.notices.restored);
        } catch (e) {
          notify(`${messages.workspace.notices.undoFailed}：${format(e)}`, "error");
        }
      });
    } catch (e) {
      pendingDeleteIds = pendingDeleteIds.filter((id) => id !== item.id);
      notify(`${messages.toast.deleteFailed}：${format(e)}`, "error");
    }
  }
  async function saveText() {
    const text = composeText.trim();
    if (!text) return;
    try {
      await dockApi.createText("home", text, "manual");
      composeOpen = false;
      composeText = "";
      notify(messages.toast.storedText);
      setTimeout(() => void refreshAll(), 0);
    } catch (e) {
      notify(format(e), "error");
    }
  }
  async function paste(e: ClipboardEvent) {
    if (
      (e.target as HTMLElement).closest("input, textarea, [contenteditable]")
    )
      return;
    const files = Array.from(e.clipboardData?.files ?? []);
    const text = e.clipboardData?.getData("text/plain");
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
      notify(messages.toast.storedText);
      setTimeout(() => void refreshAll(), 0);
    } else if (text) {
      e.preventDefault();
      await dockApi.createText("home", text, "manual");
      notify(messages.toast.storedText);
      setTimeout(() => void refreshAll(), 0);
    }
  }
  async function importPaths(paths: string[]) {
    try {
      for (const p of paths) await dockApi.importFile(p, "home");
      notify(
        messages.toast.storedFiles.replace("{n}", String(paths.length)),
      );
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
    // Errors and undoable actions linger; plain confirmations fade quickly.
    const duration = kind === "error" ? 8000 : undo ? 10000 : 4000;
    toastTimer = setTimeout(() => (toast = null), duration);
  }
  function dismissToast() {
    if (toastTimer) clearTimeout(toastTimer);
    toast = null;
  }
  function format(e: unknown) {
    return e instanceof Error
      ? e.message
      : typeof e === "string"
        ? e
        : messages.toast.unknownError;
  }
  function autofocus(el: HTMLElement) {
    setTimeout(() => el.focus(), 0);
  }
  function handleComposeKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      composeOpen = false;
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void saveText();
    }
  }

  void initialize;
</script>

<div class="app-shell">
  {#if minimized}<MinimizedTab />{:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="shell"
      class:mac={isMac}
      onpaste={paste}
      ondragover={(e) => {
        e.preventDefault();
        dragOverlay = true;
      }}
      ondragleave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node))
          dragOverlay = false;
      }}
      ondrop={(e) => {
        e.preventDefault();
        dragOverlay = false;
        void (async () => {
          const paths = await dockApi.readClipboardFilePaths(e.dataTransfer);
          if (paths.length) await importPaths(paths);
        })();
      }}
    >
      <TopBar
        {currentView}
        onNavigate={navigate}
        onToggleSettings={toggleSettings}
        onMinimize={minimize}
      />
      {#if currentView === "settings" && preferences}<SettingsView
          {preferences}
          onChange={(p) => (preferences = p)}
          onBack={() => (currentView = browser.scope)}
          notify={notify}
        />{:else}<ContentWorkspace
          {browser}
          {search}
          {selectedDetail}
          {detailLoading}
          {pendingDeleteIds}
          onSearch={(q) => void search.search(q)}
          onClearSearch={() => search.clear()}
          onSelect={select}
          onSetKind={(kind: ContentKind | null) => {
            browser.setKind(kind);
            void refreshAll();
          }}
          onReorder={async (ids: string[]) => {
            browser.reorderLocal(ids);
            await contentApi.reorder(ids);
          }}
          onToggleSaved={toggleSaved}
          onCopy={copyItem}
          onDelete={remove}
          onCreateText={() => (composeOpen = true)}
          onDetailChanged={async (id: string) => {
            await refreshAll();
            if (selectedDetail?.summary.id === id)
              selectedDetail = await contentApi.detail(id);
          }}
          onNotify={notify}
        />{/if}{#if currentView !== "settings"}<QuickAccessFab
        onOpen={openQuickAccess}
        disabled={quickAccessOpening}
      />{/if}
  {#if composeOpen}<div class="compose" role="dialog" aria-label={messages.workspace.createText}>
      <textarea
        bind:value={composeText}
        placeholder={messages.home.inputHint}
        use:autofocus
        onkeydown={handleComposeKeydown}
      ></textarea>
      <div>
        <button
          type="button"
          class="compose-cancel"
          onclick={() => (composeOpen = false)}>{messages.workspace.cancel}</button
        ><button type="button" class="compose-save" onclick={saveText}
          >{messages.workspace.storeAction}</button
        >
      </div>
    </div>{/if}
  {#if toast}<div
      class="toast"
      class:error={toast.kind === "error"}
      role="status"
    >
      <span class="toast-text">{toast.text}</span>{#if toast.undo}<button
          type="button"
          class="toast-undo"
          onclick={toast.undo}>{messages.workspace.undo}</button
        >{/if}<button
        type="button"
        class="toast-close"
        aria-label={messages.workspace.cancel}
        onclick={dismissToast}><Icon name="x" size={11} /></button
      >
    </div>{/if}{#if dragOverlay}<div class="drag-overlay">
      <Icon name="inbox" size={28} strokeWidth={1.5} />
      <span>{messages.toast.dragDropFile}</span>
    </div>{/if}
</div>
  {/if}
</div>

<style>
  .app-shell {
    flex: 1;
    min-width: 0;
    display: flex;
  }
  .shell {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--border-default);
    border-radius: 10px;
    background: var(--surface-0);
    box-shadow: var(--shadow-default);
    backdrop-filter: blur(24px);
    font-size: var(--font-ui-size, 12px);
  }
  .shell.mac {
    border-radius: 12px;
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
    padding: 0.45rem 0.55rem 0.45rem 0.8rem;
    border: 1px solid color-mix(in srgb, var(--color-primary) 45%, transparent);
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-primary);
    font-size: max(var(--font-sm, 0.72rem), 0.72rem);
    transform: translateX(-50%);
    box-shadow: var(--shadow-default);
    animation: toast-in 0.18s ease-out;
  }
  .toast.error {
    border-color: color-mix(in srgb, var(--color-danger) 50%, transparent);
  }
  .toast-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .toast-undo {
    border: 0;
    background: none;
    color: var(--color-primary);
    font: inherit;
    font-weight: 500;
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
    flex: 0 0 auto;
  }
  .toast-close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.35rem;
    height: 1.35rem;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    flex: 0 0 auto;
    transition: color 0.12s, background 0.12s;
  }
  .toast-close:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 12%, transparent);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(0.4rem);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .toast,
    .compose {
      animation: none;
    }
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
    animation: compose-in 0.18s ease-out;
  }
  @keyframes compose-in {
    from {
      opacity: 0;
      transform: translateY(0.4rem);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
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
  .compose textarea:focus {
    outline: none;
    border-color: color-mix(in srgb, var(--color-primary) 45%, transparent);
  }
  .compose div {
    display: flex;
    justify-content: flex-end;
    gap: 0.4rem;
  }
  .compose button {
    min-height: 2.2rem;
    padding: 0.35rem 0.75rem;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text-primary);
    font: inherit;
    cursor: pointer;
    transition: background 0.12s, border-color 0.12s;
  }
  .compose-cancel:hover {
    border-color: var(--border-emphasis);
  }
  .compose-save {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
    background: color-mix(in srgb, var(--color-primary) 14%, var(--surface-1));
    color: var(--color-primary);
    font-weight: 500;
  }
  .compose-save:hover {
    background: color-mix(in srgb, var(--color-primary) 22%, var(--surface-1));
  }
  .drag-overlay {
    position: absolute;
    inset: 0.5rem;
    z-index: 120;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    border: 2px dashed var(--color-primary);
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--surface-0) 90%, transparent);
    color: var(--color-primary);
    font-weight: 600;
  }
</style>
