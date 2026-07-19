import { describe, expect, it, vi } from "vitest";
import { UnifiedSearchController } from "./content-search";
import type { ContentSearchHit, ContentSummary } from "$lib/types/content";

const caps = { copyText:true, copyImage:false, copyFile:false, copyPath:false, openUrl:false, revealSensitive:false, edit:true, save:true, unsave:false, delete:true, reorder:true };
function hit(id: string): ContentSearchHit { return { summary: { id, kind:"text", retention:"temporary", title:id, preview:null, createdAt:"", updatedAt:"", cleanupAt:null, capabilities:caps } as ContentSummary, score:1, sources:["local"] }; }
function deferred<T>() { let resolve!: (v:T)=>void; const promise = new Promise<T>((r)=>resolve=r); return {promise,resolve}; }
function planned(keywords: string[] = []) { return { plan: { kinds: [], keywords, aliases: [], dateFrom: null, dateTo: null }, understoodTerms: keywords, audit: { providerId:"dummy", model:"dummy-model", sentAt:"", messages:[] } }; }
function api(searchLocal: (...args:any[])=>Promise<ContentSearchHit[]>, planSearch=vi.fn(async()=>planned()), cancelPlan=vi.fn(async()=>{})) { return { searchLocal: vi.fn(searchLocal), planSearch, cancelPlan }; }

describe("UnifiedSearchController", () => {
  it("publishes only the latest out-of-order response", async () => {
    const a=deferred<ContentSearchHit[]>(), b=deferred<ContentSearchHit[]>();
    const searchApi=api((q:string)=>q==="a"?a.promise:b.promise);
    const c=new UnifiedSearchController(searchApi,vi.fn(),{debounceMs:0,aiDelayMs:0,usePlanner:false});
    const first=c.search("a"), second=c.search("ab"); b.resolve([hit("vault:new")]); a.resolve([hit("dock:old")]); await Promise.all([first,second]);
    expect(c.snapshot.query).toBe("ab"); expect(c.snapshot.hits[0].summary.id).toBe("vault:new");
  });
  it("retains selection if possible and last valid results on error", async () => {
    const searchApi=api(vi.fn().mockResolvedValueOnce([hit("a"),hit("b")]).mockResolvedValueOnce([hit("b")]).mockRejectedValueOnce(new Error("offline")));
    const c=new UnifiedSearchController(searchApi,vi.fn(),{debounceMs:0,aiDelayMs:0,usePlanner:false}); await c.search("one"); c.select("b"); await c.search("two"); await c.search("three");
    expect(c.snapshot.selectedId).toBe("b"); expect(c.snapshot.hits[0].summary.id).toBe("b"); expect(c.snapshot.phase).toBe("error");
  });
  it("sends an explicit kind plan only when selected and clears synchronously", async () => {
    const searchApi=api(vi.fn().mockResolvedValue([])); const c=new UnifiedSearchController(searchApi,vi.fn(),{debounceMs:0,aiDelayMs:0,usePlanner:false}); c.setKinds(["image"]); await c.search(" screenshot ");
    expect(searchApi.searchLocal).toHaveBeenCalledWith("screenshot",expect.objectContaining({kinds:["image"]}),50); await c.search("  "); expect(c.snapshot).toMatchObject({query:"",hits:[],phase:"idle"});
  });

  it("publishes local hits before optional AI expansion", async () => {
    const planner=deferred<ReturnType<typeof planned>>();
    const searchApi=api(vi.fn().mockResolvedValueOnce([hit("dock:local")]).mockResolvedValueOnce([hit("dock:local"),hit("vault:expanded")]),vi.fn(()=>planner.promise));
    const states:any[]=[]; const c=new UnifiedSearchController(searchApi,s=>states.push(s),{debounceMs:0,aiDelayMs:0,usePlanner:true});
    const pending=c.search("生产");
    await vi.waitFor(()=>expect(states.at(-1)?.phase).toBe("planning"));
    expect(states.at(-1)?.hits[0].summary.id).toBe("dock:local");
    planner.resolve(planned(["prod"])); await pending;
    expect(c.snapshot.phase).toBe("expanded"); expect(c.snapshot.hits.map(h=>h.summary.id)).toEqual(["dock:local","vault:expanded"]);
  });

  it("keeps local hits when planning fails and cancels the previous planner", async () => {
    const first=deferred<ReturnType<typeof planned>>(); const cancelPlan=vi.fn(async()=>{});
    const planSearch=vi.fn().mockImplementationOnce(()=>first.promise).mockRejectedValueOnce(new Error("offline"));
    const searchApi=api(async()=>[hit("dock:local")],planSearch,cancelPlan);
    const c=new UnifiedSearchController(searchApi,vi.fn(),{debounceMs:0,aiDelayMs:0,usePlanner:true});
    void c.search("first"); await vi.waitFor(()=>expect(planSearch).toHaveBeenCalledTimes(1));
    await c.search("second");
    expect(cancelPlan).toHaveBeenCalledWith(expect.stringMatching(/^content-search-/));
    expect(c.snapshot).toMatchObject({phase:"local",hits:[expect.objectContaining({summary:expect.objectContaining({id:"dock:local"})})]});
  });
});
