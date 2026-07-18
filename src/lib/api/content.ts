import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  BrowseScope,
  ContentChangedEvent,
  ContentDeleteFailedEvent,
  ContentDetail,
  ContentKind,
  ContentRevision,
  ContentSearchHit,
  ContentSummary,
  DeleteUndoToken,
  UnifiedQueryPlan,
} from '$lib/types/content';

export const contentApi = {
  revision(): Promise<ContentRevision> {
    return invoke<ContentRevision>('ipc_content_revision');
  },

  list(scope: BrowseScope, kind: ContentKind | null): Promise<ContentSummary[]> {
    return invoke<ContentSummary[]>('ipc_content_list', { scope, kind });
  },

  detail(id: string): Promise<ContentDetail> {
    return invoke<ContentDetail>('ipc_content_detail', { id });
  },

  searchLocal(
    query: string,
    plan: UnifiedQueryPlan | null,
    limit?: number,
  ): Promise<ContentSearchHit[]> {
    return invoke<ContentSearchHit[]>('ipc_content_search_local', {
      query,
      plan,
      limit: limit ?? null,
    });
  },

  save(id: string): Promise<ContentSummary> {
    return invoke<ContentSummary>('ipc_content_save', { id });
  },

  unsave(id: string): Promise<ContentSummary> {
    return invoke<ContentSummary>('ipc_content_unsave', { id });
  },

  reorder(scope: BrowseScope, orderedIds: string[]): Promise<void> {
    return invoke<void>('ipc_content_reorder', { scope, orderedIds });
  },

  delete(id: string): Promise<DeleteUndoToken> {
    return invoke<DeleteUndoToken>('ipc_content_delete', { id });
  },

  restore(token: string): Promise<ContentSummary> {
    return invoke<ContentSummary>('ipc_content_restore', { token });
  },
};

export function onContentChanged(
  callback: (event: ContentChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<ContentChangedEvent>('content-changed', (event) => callback(event.payload));
}

export function onContentDeleteFailed(
  callback: (event: ContentDeleteFailedEvent) => void,
): Promise<UnlistenFn> {
  return listen<ContentDeleteFailedEvent>('content-delete-failed', (event) =>
    callback(event.payload),
  );
}
