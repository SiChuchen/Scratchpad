import type {
  BrowseScope,
  ContentKind,
  ContentRevision,
  ContentSummary,
} from "$lib/types/content";

export type ContentBrowserPhase = "idle" | "loading" | "ready" | "error";

export interface ContentBrowserState {
  scope: BrowseScope;
  kind: ContentKind | null;
  items: ContentSummary[];
  selectedId: string | null;
  revision: number;
  phase: ContentBrowserPhase;
  error: string | null;
}

export interface ContentBrowserApi {
  list(scope: BrowseScope, kind: ContentKind | null): Promise<ContentSummary[]>;
  revision(): Promise<ContentRevision>;
  reorder(scope: BrowseScope, orderedIds: string[]): Promise<void>;
}

export class ContentBrowserController {
  private state: ContentBrowserState;
  private requestVersion = 0;

  constructor(
    private readonly api: ContentBrowserApi,
    private readonly onState: (state: ContentBrowserState) => void,
  ) {
    this.state = {
      scope: "temporary",
      kind: null,
      items: [],
      selectedId: null,
      revision: 0,
      phase: "idle",
      error: null,
    };
  }

  get snapshot(): ContentBrowserState {
    return structuredClone(this.state);
  }

  select(id: string | null): void {
    this.publish({ selectedId: id });
  }

  async setKind(kind: ContentKind | null): Promise<void> {
    await this.load(this.state.scope, kind);
  }

  async load(scope: BrowseScope, kind = this.state.kind): Promise<void> {
    const request = ++this.requestVersion;
    this.publish({ scope, kind, phase: "loading", error: null });

    try {
      const [items, revision] = await Promise.all([
        this.api.list(scope, kind),
        this.api.revision(),
      ]);
      if (request !== this.requestVersion) return;

      const selectedId = items.some((item) => item.id === this.state.selectedId)
        ? this.state.selectedId
        : (items[0]?.id ?? null);
      this.publish({
        items,
        revision: revision.revision,
        selectedId,
        phase: "ready",
        error: null,
      });
    } catch (error) {
      if (request !== this.requestVersion) return;
      this.publish({
        phase: "error",
        error: readableLoadError(error),
      });
    }
  }

  async refresh(): Promise<void> {
    await this.load(this.state.scope, this.state.kind);
  }

  async refreshIfStale(): Promise<boolean> {
    const latest = await this.api.revision();
    if (latest.revision === this.state.revision) return false;
    await this.refresh();
    return true;
  }

  async reorder(orderedIds: string[]): Promise<void> {
    if (this.state.scope === "all") {
      throw new Error("all scope cannot be manually reordered");
    }

    const before = this.state.items;
    const byId = new Map(before.map((item) => [item.id, item]));
    const reordered = orderedIds
      .map((id) => byId.get(id))
      .filter((item): item is ContentSummary => item !== undefined);
    this.publish({ items: reordered });

    try {
      await this.api.reorder(this.state.scope, orderedIds);
      await this.refresh();
    } catch (error) {
      this.publish({ items: before });
      throw error;
    }
  }

  private publish(patch: Partial<ContentBrowserState>): void {
    this.state = { ...this.state, ...patch };
    this.onState(this.snapshot);
  }
}

function readableLoadError(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return `加载内容失败：${error.message}`;
  }
  if (typeof error === "string" && error.trim()) {
    return `加载内容失败：${error}`;
  }
  return "加载内容失败，请稍后重试";
}
