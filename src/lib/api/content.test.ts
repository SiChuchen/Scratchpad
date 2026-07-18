import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  contentApi,
  onContentChanged,
  onContentDeleteFailed,
} from "$lib/api/content";
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
} from "$lib/types/content";

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

function makeSummary<K extends ContentKind>(kind: K): ContentSummary<K> {
  return {
    id: `dock:${kind}-1`,
    kind,
    retention: "temporary",
    title: "Example",
    preview: null,
    createdAt: "2026-07-19T00:00:00Z",
    updatedAt: "2026-07-19T00:01:00Z",
    cleanupAt: null,
    capabilities,
  };
}

const textSummary = makeSummary("text");

describe("unified content type contract", () => {
  it("covers every enum value and detail variant bidirectionally", () => {
    const kinds = {
      text: true,
      image: true,
      file: true,
      credential: true,
      bookmark: true,
      note: true,
    } satisfies Record<ContentKind, true>;
    const sources = { dock: true, vault: true } satisfies Record<
      ContentSource,
      true
    >;
    const retention = {
      temporary: true,
      saved: true,
    } satisfies Record<RetentionState, true>;
    const scopes = {
      temporary: true,
      all: true,
      saved: true,
    } satisfies Record<BrowseScope, true>;
    const operations = {
      created: true,
      updated: true,
      retention: true,
      reordered: true,
      deleted: true,
      restored: true,
    } satisfies Record<ContentOperation, true>;
    const tagSources = { manual: true, ai: true } satisfies Record<
      ContentTagSource,
      true
    >;
    const searchSources = {
      local: true,
      aiExpanded: true,
    } satisfies Record<SearchSource, true>;
    const field = {
      key: "username",
      value: "operator",
      isSensitive: false,
      sortOrder: 0,
    } satisfies UnifiedField;
    const tag = {
      tag: "Work",
      normalizedTag: "work",
      source: "manual",
    } satisfies UnifiedTag;
    const details = {
      text: {
        kind: "text",
        summary: textSummary,
        title: "Example",
        body: "body",
      },
      image: {
        kind: "image",
        summary: makeSummary("image"),
        fileName: "image.png",
        assetPath: "assets/image.png",
        mimeType: null,
        width: null,
        height: null,
        available: true,
      },
      file: {
        kind: "file",
        summary: makeSummary("file"),
        fileName: "report.pdf",
        assetPath: "assets/report.pdf",
        mimeType: "application/pdf",
        sizeBytes: 42,
        available: true,
      },
      credential: {
        kind: "credential",
        summary: makeSummary("credential"),
        fields: [field],
        notes: null,
        tags: [tag],
      },
      bookmark: {
        kind: "bookmark",
        summary: makeSummary("bookmark"),
        url: "https://example.test",
        fields: [field],
        notes: "note",
        tags: [tag],
      },
      note: {
        kind: "note",
        summary: makeSummary("note"),
        body: "remember",
        fields: [field],
        tags: [tag],
      },
    } satisfies Record<ContentKind, ContentDetail>;

    expect(Object.keys(details)).toEqual(Object.keys(kinds));
    expect({
      sources,
      retention,
      scopes,
      operations,
      tagSources,
      searchSources,
    }).toBeTruthy();
  });

  it("narrows detail and summary kinds together", () => {
    const detail = {
      kind: "text",
      summary: textSummary,
      title: "Example",
      body: "body",
    } satisfies ContentDetail;
    const narrowed: ContentSummary<"text"> = detail.summary;

    expect(narrowed.kind).toBe("text");
  });

  it("rejects a detail whose summary has a different kind", () => {
    const imageSummary = makeSummary("image");
    const invalidDetail = {
      kind: "text",
      summary: imageSummary,
      title: "Example",
      body: "body",
      // @ts-expect-error a text detail cannot contain an image summary
    } satisfies ContentDetail;

    expect(invalidDetail.summary.kind).toBe("image");
  });
});

describe("contentApi", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  const revision = { revision: 7 } satisfies ContentRevision;
  const textDetail = {
    kind: "text",
    summary: textSummary,
    title: "Example",
    body: "body",
  } satisfies ContentDetail;
  const undo = {
    token: "undo-1",
    expiresAt: "2026-07-19T00:05:00Z",
  } satisfies DeleteUndoToken;
  const methodCases = [
    {
      label: "revision",
      call: () => contentApi.revision(),
      command: "ipc_content_revision",
      args: undefined,
      result: revision,
    },
    {
      label: "list with null kind",
      call: () => contentApi.list("temporary", null),
      command: "ipc_content_list",
      args: { scope: "temporary", kind: null },
      result: [textSummary],
    },
    {
      label: "detail",
      call: () => contentApi.detail("dock:text-1"),
      command: "ipc_content_detail",
      args: { id: "dock:text-1" },
      result: textDetail,
    },
    {
      label: "save",
      call: () => contentApi.save("dock:text-1"),
      command: "ipc_content_save",
      args: { id: "dock:text-1" },
      result: textSummary,
    },
    {
      label: "unsave",
      call: () => contentApi.unsave("dock:text-1"),
      command: "ipc_content_unsave",
      args: { id: "dock:text-1" },
      result: textSummary,
    },
    {
      label: "reorder with orderedIds casing",
      call: () => contentApi.reorder("saved", ["vault:1", "vault:2"]),
      command: "ipc_content_reorder",
      args: { scope: "saved", orderedIds: ["vault:1", "vault:2"] },
      result: undefined,
    },
    {
      label: "delete",
      call: () => contentApi.delete("dock:text-1"),
      command: "ipc_content_delete",
      args: { id: "dock:text-1" },
      result: undo,
    },
    {
      label: "restore",
      call: () => contentApi.restore("undo-1"),
      command: "ipc_content_restore",
      args: { token: "undo-1" },
      result: textSummary,
    },
  ];

  it.each(methodCases)(
    "forwards $label",
    async ({ call, command, args, result }) => {
      mockedInvoke.mockResolvedValue(result);

      await expect(call()).resolves.toBe(result);
      if (args === undefined) {
        expect(mockedInvoke).toHaveBeenCalledWith(command);
      } else {
        expect(mockedInvoke).toHaveBeenCalledWith(command, args);
      }
    },
  );

  it("forwards local search arguments and result", async () => {
    const hit = {
      summary: textSummary,
      score: 0.75,
      sources: ["local", "aiExpanded"],
    } satisfies ContentSearchHit;
    const hits = [hit];
    const plan = {
      kinds: ["text"],
      keywords: ["example"],
      aliases: [],
      dateFrom: null,
      dateTo: null,
    } satisfies UnifiedQueryPlan;
    mockedInvoke.mockResolvedValue(hits);

    await expect(contentApi.searchLocal("first", plan, 12)).resolves.toBe(hits);
    expect(mockedInvoke).toHaveBeenCalledWith("ipc_content_search_local", {
      query: "first",
      plan,
      limit: 12,
    });
  });

  it("normalizes an omitted local search limit to null", async () => {
    mockedInvoke.mockResolvedValue([]);

    await contentApi.searchLocal("first", null);

    expect(mockedInvoke).toHaveBeenCalledWith("ipc_content_search_local", {
      query: "first",
      plan: null,
      limit: null,
    });
  });
});

describe("content events", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("forwards content-changed payload and unlisten function", async () => {
    const unlisten = vi.fn();
    const payload = {
      revision: 8,
      changes: [{ id: "dock:text-1", operation: "updated" }],
    } satisfies ContentChangedEvent;
    mockedListen.mockImplementationOnce(async (name, handler) => {
      expect(name).toBe("content-changed");
      handler({ event: name, id: 1, payload });
      return unlisten;
    });
    const callback = vi.fn();

    await expect(onContentChanged(callback)).resolves.toBe(unlisten);
    expect(callback).toHaveBeenCalledWith(payload);
  });

  it("forwards content-delete-failed payload and unlisten function", async () => {
    const unlisten = vi.fn();
    const payload = {
      token: "undo-1",
      id: "dock:text-1",
      code: "asset_delete_failed",
    } satisfies ContentDeleteFailedEvent;
    mockedListen.mockImplementationOnce(async (name, handler) => {
      expect(name).toBe("content-delete-failed");
      handler({ event: name, id: 2, payload });
      return unlisten;
    });
    const callback = vi.fn();

    await expect(onContentDeleteFailed(callback)).resolves.toBe(unlisten);
    expect(callback).toHaveBeenCalledWith(payload);
  });

  it("lets callback errors propagate synchronously from the registered handler", async () => {
    const failure = new Error("consumer failed");
    const payload = { revision: 9, changes: [] } satisfies ContentChangedEvent;
    const unlisten = vi.fn<() => void>();
    mockedListen.mockImplementationOnce(async (_name, handler) => {
      expect(() =>
        handler({ event: "content-changed", id: 3, payload }),
      ).toThrow(failure);
      return unlisten;
    });

    await onContentChanged(() => {
      throw failure;
    });
  });
});
