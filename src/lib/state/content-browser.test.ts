import { describe, expect, it, vi } from "vitest";
import type {
  BrowseScope,
  ContentKind,
  ContentRevision,
  ContentSummary,
} from "$lib/types/content";
import {
  ContentBrowserController,
  type ContentBrowserApi,
  type ContentBrowserState,
} from "./content-browser";

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
};

function summary(
  id: string,
  retention: "temporary" | "saved" = "temporary",
  kind: ContentKind = "text",
): ContentSummary {
  return {
    id,
    kind,
    retention,
    title: id,
    preview: null,
    createdAt: "2026-07-18T00:00:00Z",
    updatedAt: "2026-07-18T00:00:00Z",
    cleanupAt: retention === "temporary" ? "2026-07-19T00:00:00Z" : null,
    capabilities: { ...capabilities },
  };
}

interface FakeContentApi extends ContentBrowserApi {
  setRevision(revision: number): void;
  setScope(scope: BrowseScope, items: ContentSummary[]): void;
}

function fakeContentApi(options: {
  temporary?: ContentSummary[];
  all?: ContentSummary[];
  saved?: ContentSummary[];
  revision?: number;
  reorderError?: unknown;
}): FakeContentApi {
  const scopes: Record<BrowseScope, ContentSummary[]> = {
    temporary: options.temporary ?? [],
    all: options.all ?? [],
    saved: options.saved ?? [],
  };
  let revision = options.revision ?? 1;

  return {
    async list(scope, kind) {
      const items = scopes[scope];
      return kind === null
        ? [...items]
        : items.filter((item) => item.kind === kind);
    },
    async revision() {
      return { revision };
    },
    async reorder(_scope, orderedIds) {
      if (options.reorderError !== undefined) throw options.reorderError;
      const byId = new Map(scopes[_scope].map((item) => [item.id, item]));
      scopes[_scope] = orderedIds
        .map((id) => byId.get(id))
        .filter((item): item is ContentSummary => item !== undefined);
      revision += 1;
    },
    setRevision(nextRevision) {
      revision = nextRevision;
    },
    setScope(scope, items) {
      scopes[scope] = [...items];
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("ContentBrowserController", () => {
  it("starts with a stable idle snapshot", () => {
    const controller = new ContentBrowserController(
      fakeContentApi({}),
      vi.fn(),
    );

    expect(controller.snapshot).toEqual({
      scope: "temporary",
      kind: null,
      items: [],
      selectedId: null,
      revision: 0,
      phase: "idle",
      error: null,
    });
  });

  it("loads a scope and keeps a still-visible selection", async () => {
    const api = fakeContentApi({
      temporary: [summary("dock:a"), summary("dock:b")],
      saved: [summary("vault:c", "saved")],
    });
    const states: ContentBrowserState[] = [];
    const controller = new ContentBrowserController(api, (state) =>
      states.push(state),
    );

    await controller.load("temporary");
    controller.select("dock:b");
    await controller.refresh();

    expect(states[states.length - 1]?.selectedId).toBe("dock:b");
    expect(controller.snapshot.phase).toBe("ready");
  });

  it("selects the first item when the previous selection is no longer visible", async () => {
    const api = fakeContentApi({
      temporary: [summary("dock:a"), summary("dock:b")],
    });
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("temporary");
    controller.select("dock:b");
    api.setScope("temporary", [summary("dock:new")]);

    await controller.refresh();

    expect(controller.snapshot.selectedId).toBe("dock:new");
  });

  it("setKind reloads the current scope with the requested kind", async () => {
    const api = fakeContentApi({
      saved: [
        summary("vault:text", "saved"),
        summary("vault:note", "saved", "note"),
      ],
    });
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("saved");

    await controller.setKind("note");

    expect(controller.snapshot.scope).toBe("saved");
    expect(controller.snapshot.kind).toBe("note");
    expect(controller.snapshot.items.map((item) => item.id)).toEqual([
      "vault:note",
    ]);
  });

  it("repairs a missed event when backend revision advanced", async () => {
    const api = fakeContentApi({
      temporary: [summary("dock:a")],
      revision: 4,
    });
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("temporary");
    api.setRevision(5);
    api.setScope("temporary", [summary("dock:new"), summary("dock:a")]);

    expect(await controller.refreshIfStale()).toBe(true);
    expect(controller.snapshot.items.map((item) => item.id)).toEqual([
      "dock:new",
      "dock:a",
    ]);
    expect(controller.snapshot.revision).toBe(5);
  });

  it("does not reload when the backend revision is current", async () => {
    const api = fakeContentApi({ temporary: [summary("dock:a")], revision: 4 });
    const list = vi.spyOn(api, "list");
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("temporary");
    list.mockClear();

    expect(await controller.refreshIfStale()).toBe(false);
    expect(list).not.toHaveBeenCalled();
  });

  it("keeps existing items visible while a refresh is loading", async () => {
    const api = fakeContentApi({ temporary: [summary("dock:a")] });
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("temporary");
    const nextList = deferred<ContentSummary[]>();
    vi.spyOn(api, "list").mockReturnValueOnce(nextList.promise);

    const refresh = controller.refresh();

    expect(controller.snapshot.phase).toBe("loading");
    expect(controller.snapshot.items.map((item) => item.id)).toEqual([
      "dock:a",
    ]);
    nextList.resolve([summary("dock:b")]);
    await refresh;
  });

  it("only publishes the latest concurrent load result", async () => {
    const first = deferred<ContentSummary[]>();
    const second = deferred<ContentSummary[]>();
    let revision = 1;
    const api: ContentBrowserApi = {
      list: vi
        .fn()
        .mockReturnValueOnce(first.promise)
        .mockReturnValueOnce(second.promise),
      revision: async (): Promise<ContentRevision> => ({
        revision: revision++,
      }),
      reorder: async () => {},
    };
    const controller = new ContentBrowserController(api, vi.fn());

    const olderLoad = controller.load("temporary");
    const latestLoad = controller.load("saved");
    second.resolve([summary("vault:new", "saved")]);
    await latestLoad;
    first.resolve([summary("dock:old")]);
    await olderLoad;

    expect(controller.snapshot.scope).toBe("saved");
    expect(controller.snapshot.items.map((item) => item.id)).toEqual([
      "vault:new",
    ]);
    expect(controller.snapshot.revision).toBe(2);
  });

  it("ignores a stale load failure", async () => {
    const first = deferred<ContentSummary[]>();
    const second = deferred<ContentSummary[]>();
    const api: ContentBrowserApi = {
      list: vi
        .fn()
        .mockReturnValueOnce(first.promise)
        .mockReturnValueOnce(second.promise),
      revision: async () => ({ revision: 3 }),
      reorder: async () => {},
    };
    const states: ContentBrowserState[] = [];
    const controller = new ContentBrowserController(api, (state) =>
      states.push(state),
    );

    const olderLoad = controller.load("temporary");
    const latestLoad = controller.load("saved");
    second.resolve([summary("vault:new", "saved")]);
    await latestLoad;
    const publishedAfterLatest = states.length;
    first.reject(new Error("old network failure"));
    await expect(olderLoad).resolves.toBeUndefined();

    expect(states).toHaveLength(publishedAfterLatest);
    expect(controller.snapshot.error).toBeNull();
    expect(controller.snapshot.items.map((item) => item.id)).toEqual([
      "vault:new",
    ]);
  });

  it("publishes a readable current-load error without losing scope, kind, or items", async () => {
    const api = fakeContentApi({ temporary: [summary("dock:a")] });
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("temporary");
    await controller.setKind("text");
    vi.spyOn(api, "list").mockRejectedValueOnce(
      new Error("service unavailable"),
    );

    await expect(controller.refresh()).resolves.toBeUndefined();

    expect(controller.snapshot.scope).toBe("temporary");
    expect(controller.snapshot.kind).toBe("text");
    expect(controller.snapshot.items.map((item) => item.id)).toEqual([
      "dock:a",
    ]);
    expect(controller.snapshot.phase).toBe("error");
    expect(controller.snapshot.error).toContain("service unavailable");
  });

  it("rolls an optimistic reorder back when persistence fails", async () => {
    const api = fakeContentApi({
      saved: [summary("dock:a", "saved"), summary("vault:b", "saved")],
      reorderError: new Error("write failed"),
    });
    const states: ContentBrowserState[] = [];
    const controller = new ContentBrowserController(api, (state) =>
      states.push(state),
    );
    await controller.load("saved");

    await expect(controller.reorder(["vault:b", "dock:a"])).rejects.toThrow(
      "write failed",
    );

    expect(states.some((state) => state.items[0]?.id === "vault:b")).toBe(true);
    expect(controller.snapshot.items.map((item) => item.id)).toEqual([
      "dock:a",
      "vault:b",
    ]);
  });

  it("does not roll a failed reorder into a newer scope", async () => {
    const pending = deferred<void>();
    const api = fakeContentApi({
      temporary: [summary("dock:a"), summary("dock:b")],
      saved: [summary("vault:saved", "saved")],
    });
    vi.spyOn(api, "reorder").mockReturnValueOnce(pending.promise);
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("temporary");
    const reorder = controller.reorder(["dock:b", "dock:a"]);
    await controller.load("saved");
    pending.reject(new Error("write failed"));
    await expect(reorder).rejects.toThrow("write failed");
    expect(controller.snapshot.scope).toBe("saved");
    expect(controller.snapshot.items.map((item) => item.id)).toEqual(["vault:saved"]);
  });

  it("persists a reorder and refreshes the authoritative order", async () => {
    const api = fakeContentApi({
      saved: [summary("dock:a", "saved"), summary("vault:b", "saved")],
    });
    const reorder = vi.spyOn(api, "reorder");
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("saved");

    await controller.reorder(["vault:b", "dock:a"]);

    expect(reorder).toHaveBeenCalledWith("saved", ["vault:b", "dock:a"]);
    expect(controller.snapshot.items.map((item) => item.id)).toEqual([
      "vault:b",
      "dock:a",
    ]);
  });

  it("rejects manual reorder in the all scope without changing state", async () => {
    const api = fakeContentApi({
      all: [summary("dock:a"), summary("vault:b", "saved")],
    });
    const reorder = vi.spyOn(api, "reorder");
    const controller = new ContentBrowserController(api, vi.fn());
    await controller.load("all");
    const before = controller.snapshot;

    await expect(controller.reorder(["vault:b", "dock:a"])).rejects.toThrow(
      "all scope cannot be manually reordered",
    );

    expect(reorder).not.toHaveBeenCalled();
    expect(controller.snapshot).toEqual(before);
  });
});
