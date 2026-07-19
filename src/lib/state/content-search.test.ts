import { describe, expect, it, vi } from "vitest";
import { UnifiedSearchController } from "./content-search";
import type { ContentSearchHit, ContentSummary } from "$lib/types/content";

const caps = { copyText:true, copyImage:false, copyFile:false, copyPath:false, openUrl:false, revealSensitive:false, edit:true, save:true, unsave:false, delete:true, reorder:true };
function hit(id: string): ContentSearchHit { return { summary: { id, kind:"text", retention:"temporary", title:id, preview:null, createdAt:"", updatedAt:"", cleanupAt:null, capabilities:caps } as ContentSummary, score:1, sources:["local"] }; }
function deferred<T>() { let resolve!: (v:T)=>void; const promise = new Promise<T>((r)=>resolve=r); return {promise,resolve}; }

describe("UnifiedSearchController", () => {
  it("publishes only the latest out-of-order response", async () => {
    const a=deferred<ContentSearchHit[]>(), b=deferred<ContentSearchHit[]>();
    const api={searchLocal:vi.fn((q:string)=>q==="a"?a.promise:b.promise)};
    const c=new UnifiedSearchController(api,vi.fn(),0);
    const first=c.search("a"), second=c.search("ab"); b.resolve([hit("vault:new")]); a.resolve([hit("dock:old")]); await Promise.all([first,second]);
    expect(c.snapshot.query).toBe("ab"); expect(c.snapshot.hits[0].summary.id).toBe("vault:new");
  });
  it("retains selection if possible and last valid results on error", async () => {
    const api={searchLocal:vi.fn().mockResolvedValueOnce([hit("a"),hit("b")]).mockResolvedValueOnce([hit("b")]).mockRejectedValueOnce(new Error("offline"))};
    const c=new UnifiedSearchController(api,vi.fn(),0); await c.search("one"); c.select("b"); await c.search("two"); await c.search("three");
    expect(c.snapshot.selectedId).toBe("b"); expect(c.snapshot.hits[0].summary.id).toBe("b"); expect(c.snapshot.phase).toBe("error");
  });
  it("sends an explicit kind plan only when selected and clears synchronously", async () => {
    const api={searchLocal:vi.fn().mockResolvedValue([])}; const c=new UnifiedSearchController(api,vi.fn(),0); c.setKinds(["image"]); await c.search(" screenshot ");
    expect(api.searchLocal).toHaveBeenCalledWith("screenshot",expect.objectContaining({kinds:["image"]}),50); await c.search("  "); expect(c.snapshot).toMatchObject({query:"",hits:[],phase:"idle"});
  });
});
