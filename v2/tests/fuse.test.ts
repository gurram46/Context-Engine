import { describe, it, expect } from "vitest";
import { fuseEvidence } from "../src/ranking/fuse.js";

describe("fuse", () => {
  it("deduplicates same file+line", () => {
    const ev: any[] = [
      { source: "exact", file: "a.py", startLine: 10, symbol: "foo", score: 1.0, relation: "reference" },
      { source: "exact", file: "a.py", startLine: 10, symbol: "foo", score: 1.0, relation: "reference" },
      { source: "symbol", file: "a.py", startLine: 10, symbol: "foo", score: 0.99, relation: "definition" },
    ];
    const { ranked, deduped } = fuseEvidence(ev as any, { queryType: "SYMBOL", rawQuery: "foo" });
    expect(deduped).toBeGreaterThan(0);
    expect(ranked.length).toBeLessThanOrEqual(2);
  });
  it("does not drop definition for diversity", () => {
    const ev: any[] = [];
    for (let i=0;i<5;i++) ev.push({ source: "semantic", file: "backend/context_engine/commands/bundle_command.py", startLine: 20+i*10, symbol: "bundle", score: 0.96, relation: "unknown" });
    ev.push({ source: "symbol", file: "backend/context_engine/commands/bundle_command.py", startLine: 22, symbol: "bundle", score: 0.96, relation: "definition" });
    const { ranked } = fuseEvidence(ev as any, { queryType: "SYMBOL", rawQuery: "bundle" });
    expect(ranked.some(r=> r.relation==="definition")).toBe(true);
  });
});
