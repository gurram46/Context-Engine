import { describe, it, expect } from "vitest";
import { scoreAuthority } from "../src/ranking/authority.js";

describe("authority", () => {
  it("boosts exact symbol definition", () => {
    const r = scoreAuthority({ evidence: { source: "symbol", file: "backend/context_engine/core/utils.py", symbol: "count_tokens", relation: "definition", score: 0.99 } as any, queryType: "SYMBOL", rawQuery: "count_tokens" });
    expect(r.score).toBeGreaterThan(20);
  });
  it("penalizes doc when impl wanted", () => {
    const r = scoreAuthority({ evidence: { source: "semantic", file: "docs/guides/03-cli-snippets.md", score: 0.9 } as any, queryType: "SYMBOL", rawQuery: "bundle" });
    expect(r.score).toBeLessThan(0);
  });
  it("penalizes broad context helpers", () => {
    const r = scoreAuthority({ evidence: { source: "semantic", file: "backend/context_engine/core/task_manager.py", symbol: "_ensure_context_dir", score: 0.96 } as any, queryType: "CONCEPTUAL", rawQuery: "bundle" });
    expect(r.score).toBeLessThan(5);
  });
});
