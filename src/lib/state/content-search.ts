import type { ContentKind, ContentSearchHit, UnifiedQueryPlan } from "$lib/types/content";

export type ContentSearchPhase = "idle" | "searching" | "ready" | "error";

export interface ContentSearchState {
  query: string;
  hits: ContentSearchHit[];
  selectedId: string | null;
  phase: ContentSearchPhase;
  error: string | null;
}

export interface UnifiedSearchApi {
  searchLocal(query: string, plan: UnifiedQueryPlan | null, limit: number): Promise<ContentSearchHit[]>;
}

export function initialSearchState(): ContentSearchState {
  return { query: "", hits: [], selectedId: null, phase: "idle", error: null };
}

export class UnifiedSearchController {
  private state = initialSearchState();
  private requestVersion = 0;
  private kinds: ContentKind[] = [];

  constructor(
    private readonly api: UnifiedSearchApi,
    private readonly onState: (state: ContentSearchState) => void,
    private readonly delayMs = 200,
    private readonly limit = 50,
  ) {}

  get snapshot(): ContentSearchState { return structuredClone(this.state); }

  select(id: string | null): void { this.publish({ selectedId: id }); }

  setKinds(kinds: ContentKind[]): void { this.kinds = [...kinds]; }

  async search(query: string): Promise<void> {
    const normalized = query.trim();
    const request = ++this.requestVersion;
    if (!normalized) {
      this.publish(initialSearchState());
      return;
    }
    this.publish({ query, phase: "searching", error: null });
    await new Promise<void>((resolve) => setTimeout(resolve, this.delayMs));
    if (request !== this.requestVersion) return;
    try {
      const plan: UnifiedQueryPlan | null = this.kinds.length
        ? { kinds: [...this.kinds], keywords: [], aliases: [], dateFrom: null, dateTo: null }
        : null;
      const hits = await this.api.searchLocal(normalized, plan, this.limit);
      if (request !== this.requestVersion) return;
      const selectedId = hits.some((hit) => hit.summary.id === this.state.selectedId)
        ? this.state.selectedId
        : (hits[0]?.summary.id ?? null);
      this.publish({ query, hits, selectedId, phase: "ready", error: null });
    } catch (error) {
      if (request === this.requestVersion) this.publish({ phase: "error", error: String(error) });
    }
  }

  dispose(): void { this.requestVersion += 1; }

  private publish(patch: Partial<ContentSearchState>): void {
    this.state = { ...this.state, ...patch };
    this.onState(this.snapshot);
  }
}
