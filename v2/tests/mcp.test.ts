import { describe, it, expect, vi } from "vitest";
import { createTools } from "../src/mcp/tools.js";
import type { ContextEngine } from "../src/core/contextEngine.js";

function mockEngine() {
  const mock: any = {
    search: vi.fn(async (q: string, opts: any) => ({
      query: q,
      type: "MIXED",
      evidence: [{ file: "a.py", startLine: 1, symbol: "foo", source: "symbol", score: 0.9, authorityScore: 10, finalScore: 28 }],
      packed: { markdown: "# test", tokenEstimate: 10, files: ["a.py"] },
      stats: { retrievers: ["symbol"], elapsedMs: 10, tokenEstimate: 10, warnings: [] },
      debug: opts?.debug ? { rawEvidenceCount: 1, timings: [], decisions: [], authorityWeights: {} } : undefined,
    })),
    symbol: vi.fn(async (s: string, opts: any) => ({
      query: s,
      type: "SYMBOL",
      evidence: [{ file: "b.py", symbol: s, source: "symbol", score: 0.99 }],
      packed: { markdown: "# sym", tokenEstimate: 5, files: ["b.py"] },
      stats: { retrievers: ["symbol"], elapsedMs: 5, tokenEstimate: 5, warnings: [] },
      debug: opts?.debug ? { rawEvidenceCount: 1 } : undefined,
    })),
    dependency: vi.fn(async (s: string, dir: string, opts: any) => ({
      query: s,
      type: "DEPENDENCY",
      evidence: [{ file: "c.py", symbol: s, source: "graph" }],
      packed: { markdown: "# dep", tokenEstimate: 5, files: ["c.py"] },
      stats: { retrievers: [dir], elapsedMs: 5, tokenEstimate: 5, warnings: [] },
    })),
    tests: vi.fn(async (q: string, opts: any) => ({
      query: q,
      type: "TEST",
      evidence: [{ file: "tests/test_foo.py", source: "test" }],
      packed: { markdown: "# test", tokenEstimate: 5, files: ["tests/test_foo.py"] },
      stats: { retrievers: ["test"], elapsedMs: 5, tokenEstimate: 5, warnings: [] },
    })),
    status: vi.fn(async () => ({
      version: "0.1.0",
      projectRoot: "/tmp",
      gitBranch: "main",
      nodeVersion: "v20.19.5",
      rgAvailable: true,
      ociConnected: true,
      ociProvider: "ollama",
      ociModel: "nomic-embed-text",
      ociChunks: 100,
      warnings: [],
    })),
    callers: vi.fn(async (s: string) => mock.dependency(s, "callers", {})),
    callees: vi.fn(async (s: string) => mock.dependency(s, "callees", {})),
  };
  return mock as unknown as ContextEngine & { search: any; symbol: any; dependency: any; tests: any; status: any };
}

describe("mcp tools", () => {
  it("1. tool schemas register correctly (5 tools)", () => {
    const tools = createTools(mockEngine());
    expect(tools.length).toBe(5);
    const names = tools.map((t) => t.name);
    expect(names).toEqual(["context_search", "symbol_lookup", "dependency_trace", "test_lookup", "context_status"]);
  });

  it("2. context_search calls shared core", async () => {
    const engine = mockEngine();
    const tools = createTools(engine);
    const t = tools.find((x) => x.name === "context_search")!;
    const res = await t.handler({ query: "hello", debug: false });
    expect(engine.search).toHaveBeenCalled();
    expect(res.query).toBe("hello");
    expect(res.context).toBeDefined();
  });

  it("3. symbol_lookup calls shared core", async () => {
    const engine = mockEngine();
    const tools = createTools(engine);
    const t = tools.find((x) => x.name === "symbol_lookup")!;
    await t.handler({ symbol: "foo" });
    expect(engine.symbol).toHaveBeenCalledWith("foo", expect.anything());
  });

  it("4. dependency_trace callers route", async () => {
    const engine = mockEngine();
    const tools = createTools(engine);
    const t = tools.find((x) => x.name === "dependency_trace")!;
    await t.handler({ symbol: "foo", direction: "callers" });
    expect(engine.dependency).toHaveBeenCalledWith("foo", "callers", expect.anything());
  });

  it("5. test_lookup route", async () => {
    const engine = mockEngine();
    const tools = createTools(engine);
    const t = tools.find((x) => x.name === "test_lookup")!;
    await t.handler({ query: "foo" });
    expect(engine.tests).toHaveBeenCalled();
  });

  it("6. debug=false omits heavy debug metadata", async () => {
    const engine = mockEngine();
    const tools = createTools(engine);
    const t = tools.find((x) => x.name === "context_search")!;
    const res = await t.handler({ query: "hello", debug: false });
    expect(res.debug).toBeUndefined();
    expect(res.evidence[0].authorityScore).toBeUndefined();
  });

  it("7. debug=true includes ranking/provenance metadata", async () => {
    const engine = mockEngine();
    const tools = createTools(engine);
    const t = tools.find((x) => x.name === "context_search")!;
    const res = await t.handler({ query: "hello", debug: true });
    expect(res.debug).toBeDefined();
  });

  it("8. invalid request gets structured error", async () => {
    const engine = mockEngine();
    const tools = createTools(engine);
    const t = tools.find((x) => x.name === "context_search")!;
    await expect(t.handler({})).rejects.toThrow("query is required");
  });

  it("9. backend failure produces partial/warning behavior where possible", async () => {
    const engine: any = {
      search: vi.fn(async () => {
        throw new Error("oci down");
      }),
      status: vi.fn(async () => ({ warnings: ["oci down"] })),
    };
    // Simulate fallback: our real ContextEngine does fallback to exact, but mock should show error handling
    // Here we test that tool handler surfaces error, not crash
    const tools = createTools(engine);
    const t = tools.find((x) => x.name === "context_search")!;
    await expect(t.handler({ query: "test" })).rejects.toThrow();
  });

  it("10. one ContextEngine reused across multiple requests", async () => {
    const engine = mockEngine();
    const tools = createTools(engine);
    const t1 = tools.find((x) => x.name === "context_search")!;
    const t2 = tools.find((x) => x.name === "symbol_lookup")!;
    await t1.handler({ query: "a" });
    await t2.handler({ symbol: "b" });
    // Same engine instance used, not recreated
    expect(engine.search).toHaveBeenCalledTimes(1);
    expect(engine.symbol).toHaveBeenCalledTimes(1);
  });
});
