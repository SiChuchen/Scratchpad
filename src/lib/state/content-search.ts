import type {
  ContentKind,
  ContentSearchHit,
  PlannedUnifiedSearch,
  UnifiedQueryPlan,
} from "$lib/types/content";

export type ContentSearchPhase =
  | "idle"
  | "searching"
  | "local"
  | "planning"
  | "expanded"
  | "error";

export interface ContentSearchState {
  query: string;
  hits: ContentSearchHit[];
  selectedId: string | null;
  phase: ContentSearchPhase;
  error: string | null;
  understoodTerms: string[];
}

export interface UnifiedSearchApi {
  searchLocal(query: string, plan: UnifiedQueryPlan | null, limit: number): Promise<ContentSearchHit[]>;
  planSearch(query: string, requestId: string): Promise<PlannedUnifiedSearch>;
  cancelPlan(requestId: string): Promise<void>;
}

export interface UnifiedSearchOptions {
  debounceMs: number;
  aiDelayMs: number;
  usePlanner: boolean;
  limit?: number;
}

export function initialSearchState(): ContentSearchState {
  return { query: "", hits: [], selectedId: null, phase: "idle", error: null, understoodTerms: [] };
}

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
const requestId = () => `content-search-${globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`}`;

export class UnifiedSearchController {
  private state = initialSearchState();
  private requestVersion = 0;
  private kinds: ContentKind[] = [];
  private activePlanId: string | null = null;
  private options: UnifiedSearchOptions;

  constructor(
    private readonly api: UnifiedSearchApi,
    private readonly onState: (state: ContentSearchState) => void,
    options: UnifiedSearchOptions | number = { debounceMs: 200, aiDelayMs: 700, usePlanner: false },
    legacyLimit = 50,
  ) {
    this.options = typeof options === "number"
      ? { debounceMs: options, aiDelayMs: 700, usePlanner: false, limit: legacyLimit }
      : options;
  }

  get snapshot(): ContentSearchState { return structuredClone(this.state); }
  select(id: string | null): void { this.publish({ selectedId: id }); }
  setKinds(kinds: ContentKind[]): void { this.kinds = [...kinds]; }
  setPlannerEnabled(enabled: boolean): void { this.options = { ...this.options, usePlanner: enabled }; }

  async search(query: string): Promise<void> {
    const request = ++this.requestVersion;
    await this.cancelActivePlan();
    if (request !== this.requestVersion) return;
    const normalized = query.trim();
    if (!normalized) {
      this.publish(initialSearchState());
      return;
    }
    this.publish({ query, phase: "searching", error: null, understoodTerms: [] });
    await wait(this.options.debounceMs);
    if (request !== this.requestVersion) return;
    const explicitPlan = this.explicitPlan();
    try {
      const hits = await this.api.searchLocal(normalized, explicitPlan, this.options.limit ?? 50);
      if (request !== this.requestVersion) return;
      this.publishHits(query, hits, "local", []);
    } catch (error) {
      if (request === this.requestVersion) this.publish({ phase: "error", error: String(error) });
      return;
    }
    if (!this.options.usePlanner || request !== this.requestVersion) return;
    await wait(this.options.aiDelayMs);
    if (request !== this.requestVersion) return;
    const id = requestId();
    this.activePlanId = id;
    this.publish({ phase: "planning", error: null });
    try {
      const planned = await this.api.planSearch(normalized, id);
      if (request !== this.requestVersion || this.activePlanId !== id) return;
      const plan = { ...planned.plan, kinds: [...this.kinds] };
      const hits = await this.api.searchLocal(normalized, plan, this.options.limit ?? 50);
      if (request !== this.requestVersion || this.activePlanId !== id) return;
      this.publishHits(query, hits, "expanded", planned.understoodTerms);
    } catch {
      if (request === this.requestVersion) this.publish({ phase: "local", error: null });
    } finally {
      if (this.activePlanId === id) this.activePlanId = null;
    }
  }

  dispose(): void {
    this.requestVersion += 1;
    void this.cancelActivePlan();
  }

  private explicitPlan(): UnifiedQueryPlan | null {
    return this.kinds.length
      ? { kinds: [...this.kinds], keywords: [], aliases: [], dateFrom: null, dateTo: null }
      : null;
  }

  private publishHits(query: string, hits: ContentSearchHit[], phase: "local" | "expanded", understoodTerms: string[]) {
    const selectedId = hits.some((hit) => hit.summary.id === this.state.selectedId)
      ? this.state.selectedId
      : (hits[0]?.summary.id ?? null);
    this.publish({ query, hits, selectedId, phase, error: null, understoodTerms });
  }

  private async cancelActivePlan(): Promise<void> {
    const id = this.activePlanId;
    if (!id) return;
    this.activePlanId = null;
    try { await this.api.cancelPlan(id); } catch { /* best-effort cancellation */ }
  }

  private publish(patch: Partial<ContentSearchState>): void {
    this.state = { ...this.state, ...patch };
    this.onState(this.snapshot);
  }
}
