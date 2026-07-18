import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  contentApi,
  onContentChanged,
  onContentDeleteFailed,
} from '$lib/api/content';
import type {
  BrowseScope,
  ContentCapabilities,
  ContentChangedEvent,
  ContentDeleteFailedEvent,
  ContentDetail,
  ContentKind,
  ContentOperation,
  ContentRevision,
  ContentSearchHit,
  ContentSource,
  ContentSummary,
  ContentTagSource,
  DeleteUndoToken,
  RetentionState,
  SearchSource,
  UnifiedField,
  UnifiedQueryPlan,
  UnifiedTag,
} from '$lib/types/content';

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

const capabilities = {
  copyText: true,
  copyImage: false,
  copyFile: false,
  copyPath: false,
  openUrl: false,
  revealSensitive: false,
  edit: true,
  save: true,
  unsave: false,
  delete: true,
  reorder: true,
} satisfies ContentCapabilities;

const summary = {
  id: 'dock:text-1',
  kind: 'text',
  retention: 'temporary',
  title: 'Example',
  preview: null,
  createdAt: '2026-07-19T00:00:00Z',
  updatedAt: '2026-07-19T00:01:00Z',
  cleanupAt: null,
  capabilities,
} satisfies ContentSummary;

describe('unified content type contract', () => {
  it('represents every enum value and detail variant', () => {
    const kinds = [
      'text',
      'image',
      'file',
      'credential',
      'bookmark',
      'note',
    ] satisfies ContentKind[];
    const sources = ['dock', 'vault'] satisfies ContentSource[];
    const retention = ['temporary', 'saved'] satisfies RetentionState[];
    const scopes = ['temporary', 'all', 'saved'] satisfies BrowseScope[];
    const operations = [
      'created',
      'updated',
      'retention',
      'reordered',
      'deleted',
      'restored',
    ] satisfies ContentOperation[];
    const tagSources = ['manual', 'ai'] satisfies ContentTagSource[];
    const searchSources = ['local', 'aiExpanded'] satisfies SearchSource[];
    const field = {
      key: 'username',
      value: 'operator',
      isSensitive: false,
      sortOrder: 0,
    } satisfies UnifiedField;
    const tag = {
      tag: 'Work',
      normalizedTag: 'work',
      source: 'manual',
    } satisfies UnifiedTag;
    const details = [
      { kind: 'text', summary, title: 'Example', body: 'body' },
      {
        kind: 'image',
        summary: { ...summary, kind: 'image' },
        fileName: 'image.png',
        assetPath: 'assets/image.png',
        mimeType: null,
        width: null,
        height: null,
        available: true,
      },
      {
        kind: 'file',
        summary: { ...summary, kind: 'file' },
        fileName: 'report.pdf',
        assetPath: 'assets/report.pdf',
        mimeType: 'application/pdf',
        sizeBytes: 42,
        available: true,
      },
      {
        kind: 'credential',
        summary: { ...summary, kind: 'credential', retention: 'saved' },
        fields: [field],
        notes: null,
        tags: [tag],
      },
      {
        kind: 'bookmark',
        summary: { ...summary, kind: 'bookmark', retention: 'saved' },
        url: 'https://example.test',
        fields: [field],
        notes: 'note',
        tags: [tag],
      },
      {
        kind: 'note',
        summary: { ...summary, kind: 'note', retention: 'saved' },
        body: 'remember',
        fields: [field],
        tags: [tag],
      },
    ] satisfies ContentDetail[];

    expect({ kinds, sources, retention, scopes, operations, tagSources, searchSources }).toBeTruthy();
    expect(details.map((detail) => detail.kind)).toEqual(kinds);
  });
});

describe('contentApi', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('forwards revision, list, detail, save, unsave, delete, and restore', async () => {
    const revision = { revision: 7 } satisfies ContentRevision;
    const undo = {
      token: 'undo-1',
      expiresAt: '2026-07-19T00:05:00Z',
    } satisfies DeleteUndoToken;
    mockedInvoke
      .mockResolvedValueOnce(revision)
      .mockResolvedValueOnce([summary])
      .mockResolvedValueOnce(
        { kind: 'text', summary, title: 'Example', body: 'body' } satisfies ContentDetail,
      )
      .mockResolvedValueOnce(summary)
      .mockResolvedValueOnce(summary)
      .mockResolvedValueOnce(undo)
      .mockResolvedValueOnce(summary);

    await expect(contentApi.revision()).resolves.toBe(revision);
    await expect(contentApi.list('temporary', null)).resolves.toEqual([summary]);
    await expect(contentApi.detail('dock:text-1')).resolves.toEqual({
      kind: 'text',
      summary,
      title: 'Example',
      body: 'body',
    });
    await expect(contentApi.save('dock:text-1')).resolves.toBe(summary);
    await expect(contentApi.unsave('dock:text-1')).resolves.toBe(summary);
    await expect(contentApi.delete('dock:text-1')).resolves.toBe(undo);
    await expect(contentApi.restore('undo-1')).resolves.toBe(summary);

    expect(mockedInvoke.mock.calls).toEqual([
      ['ipc_content_revision'],
      ['ipc_content_list', { scope: 'temporary', kind: null }],
      ['ipc_content_detail', { id: 'dock:text-1' }],
      ['ipc_content_save', { id: 'dock:text-1' }],
      ['ipc_content_unsave', { id: 'dock:text-1' }],
      ['ipc_content_delete', { id: 'dock:text-1' }],
      ['ipc_content_restore', { token: 'undo-1' }],
    ]);
  });

  it('forwards null and populated local search plans with explicit limits', async () => {
    const hit = {
      summary,
      score: 0.75,
      sources: ['local', 'aiExpanded'],
    } satisfies ContentSearchHit;
    const plan = {
      kinds: ['text'],
      keywords: ['example'],
      aliases: [],
      dateFrom: null,
      dateTo: null,
    } satisfies UnifiedQueryPlan;
    mockedInvoke.mockResolvedValue([hit]);

    await expect(contentApi.searchLocal('first', null, 12)).resolves.toEqual([hit]);
    await contentApi.searchLocal('second', plan, 5);

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'ipc_content_search_local', {
      query: 'first',
      plan: null,
      limit: 12,
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'ipc_content_search_local', {
      query: 'second',
      plan,
      limit: 5,
    });
  });

  it('uses orderedIds camel casing for reorder', async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await expect(contentApi.reorder('saved', ['vault:1', 'vault:2'])).resolves.toBeUndefined();

    expect(mockedInvoke).toHaveBeenCalledWith('ipc_content_reorder', {
      scope: 'saved',
      orderedIds: ['vault:1', 'vault:2'],
    });
  });
});

describe('content events', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('forwards content-changed payload and unlisten function', async () => {
    const unlisten = vi.fn();
    const payload = {
      revision: 8,
      changes: [{ id: 'dock:text-1', operation: 'updated' }],
    } satisfies ContentChangedEvent;
    mockedListen.mockImplementationOnce(async (name, handler) => {
      expect(name).toBe('content-changed');
      handler({ event: name, id: 1, payload });
      return unlisten;
    });
    const callback = vi.fn();

    await expect(onContentChanged(callback)).resolves.toBe(unlisten);
    expect(callback).toHaveBeenCalledWith(payload);
  });

  it('forwards content-delete-failed payload and unlisten function', async () => {
    const unlisten = vi.fn();
    const payload = {
      token: 'undo-1',
      id: 'dock:text-1',
      code: 'asset_delete_failed',
    } satisfies ContentDeleteFailedEvent;
    mockedListen.mockImplementationOnce(async (name, handler) => {
      expect(name).toBe('content-delete-failed');
      handler({ event: name, id: 2, payload });
      return unlisten;
    });
    const callback = vi.fn();

    await expect(onContentDeleteFailed(callback)).resolves.toBe(unlisten);
    expect(callback).toHaveBeenCalledWith(payload);
  });
});
