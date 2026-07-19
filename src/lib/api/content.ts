import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  BrowseScope,
  ContentChangedEvent,
  ContentDeleteFailedEvent,
  ContentDetail,
  ContentKind,
  PlannedUnifiedSearch,
  ContentRevision,
  ContentSearchHit,
  ContentSummary,
  DeleteUndoToken,
  UnifiedQueryPlan,
} from "$lib/types/content";
import type { VaultEntryInput } from "$lib/types/vault";

export const contentApi = {
  revision(): Promise<ContentRevision> {
    return invoke<ContentRevision>("ipc_content_revision");
  },

  list(
    scope: BrowseScope,
    kind: ContentKind | null,
  ): Promise<ContentSummary[]> {
    return invoke<ContentSummary[]>("ipc_content_list", { scope, kind });
  },

  detail(id: string): Promise<ContentDetail> {
    return invoke<ContentDetail>("ipc_content_detail", { id });
  },

  updateText(
    id: string,
    title: string | null,
    body: string,
  ): Promise<ContentDetail> {
    return invoke<ContentDetail>("ipc_content_update_text", {
      id,
      title,
      body,
    });
  },

  rename(id: string, title: string | null): Promise<ContentDetail> {
    return invoke<ContentDetail>("ipc_content_rename", { id, title });
  },

  updateStructured(
    id: string,
    input: VaultEntryInput,
  ): Promise<ContentDetail> {
    return invoke<ContentDetail>("ipc_content_update_structured", { id, input });
  },

  searchLocal(
    query: string,
    plan: UnifiedQueryPlan | null,
    limit?: number,
  ): Promise<ContentSearchHit[]> {
    return invoke<ContentSearchHit[]>("ipc_content_search_local", {
      query,
      plan,
      limit: limit ?? null,
    });
  },

  planSearch(query: string, requestId: string): Promise<PlannedUnifiedSearch> {
    return invoke<PlannedUnifiedSearch>("ipc_content_plan_search", { query, requestId });
  },

  cancelPlan(requestId: string): Promise<void> {
    return invoke<void>("ipc_content_cancel_search", { requestId });
  },

  save(id: string): Promise<ContentSummary> {
    return invoke<ContentSummary>("ipc_content_save", { id });
  },

  unsave(id: string): Promise<ContentSummary> {
    return invoke<ContentSummary>("ipc_content_unsave", { id });
  },

  reorder(scope: BrowseScope, orderedIds: string[]): Promise<void> {
    return invoke<void>("ipc_content_reorder", { scope, orderedIds });
  },

  delete(id: string): Promise<DeleteUndoToken> {
    return invoke<DeleteUndoToken>("ipc_content_delete", { id });
  },

  restore(token: string): Promise<ContentSummary> {
    return invoke<ContentSummary>("ipc_content_restore", { token });
  },
};

export function onContentChanged(
  callback: (event: ContentChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<ContentChangedEvent>("content-changed", (event) =>
    callback(event.payload),
  );
}

export function onContentDeleteFailed(
  callback: (event: ContentDeleteFailedEvent) => void,
): Promise<UnlistenFn> {
  return listen<ContentDeleteFailedEvent>("content-delete-failed", (event) =>
    callback(event.payload),
  );
}
