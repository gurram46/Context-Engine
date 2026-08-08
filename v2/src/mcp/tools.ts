import type { ContextEngine } from "../core/contextEngine.js";

export interface ToolDef {
  name: string;
  description: string;
  inputSchema: Record<string, any>;
  handler: (args: any) => Promise<any>;
}

export function createTools(engine: ContextEngine): ToolDef[] {
  return [
    {
      name: "context_search",
      description: "General codebase question. Uses hybrid retrieval (ripgrep + semantic + symbol) with authority ranking.",
      inputSchema: {
        type: "object",
        properties: {
          query: { type: "string", description: "Natural language question or literal" },
          budgetTokens: { type: "number", description: "Token budget for packed context, default 8000" },
          maxResults: { type: "number", description: "Max evidence items, default 10" },
          debug: { type: "boolean", description: "Include debug metadata" },
        },
        required: ["query"],
      },
      handler: async (args) => {
        if (!args.query || typeof args.query !== "string") throw new Error("query is required");
        const res = await engine.search(args.query, {
          budgetTokens: args.budgetTokens,
          maxResults: args.maxResults,
          debug: !!args.debug,
        });
        return formatResult(res, !!args.debug);
      },
    },
    {
      name: "symbol_lookup",
      description: "Find authoritative definition for a symbol (function/class).",
      inputSchema: {
        type: "object",
        properties: {
          symbol: { type: "string" },
          budgetTokens: { type: "number" },
          debug: { type: "boolean" },
        },
        required: ["symbol"],
      },
      handler: async (args) => {
        if (!args.symbol) throw new Error("symbol is required");
        const res = await engine.symbol(args.symbol, {
          budgetTokens: args.budgetTokens,
          debug: !!args.debug,
        });
        return formatResult(res, !!args.debug);
      },
    },
    {
      name: "dependency_trace",
      description: "Who calls / what does it call. Uses graph + exact fallback for dynamic registrations.",
      inputSchema: {
        type: "object",
        properties: {
          symbol: { type: "string" },
          direction: { type: "string", enum: ["callers", "callees", "both"], default: "callers" },
          budgetTokens: { type: "number" },
          debug: { type: "boolean" },
        },
        required: ["symbol"],
      },
      handler: async (args) => {
        if (!args.symbol) throw new Error("symbol is required");
        const dir = args.direction ?? "callers";
        const res = await engine.dependency(args.symbol, dir, {
          budgetTokens: args.budgetTokens,
          debug: !!args.debug,
        });
        return formatResult(res, !!args.debug);
      },
    },
    {
      name: "test_lookup",
      description: "Find tests covering feature/symbol.",
      inputSchema: {
        type: "object",
        properties: {
          query: { type: "string", description: "Feature or symbol" },
          budgetTokens: { type: "number" },
          debug: { type: "boolean" },
        },
        required: ["query"],
      },
      handler: async (args) => {
        if (!args.query) throw new Error("query is required");
        const res = await engine.tests(args.query, {
          budgetTokens: args.budgetTokens,
          debug: !!args.debug,
        });
        return formatResult(res, !!args.debug);
      },
    },
    {
      name: "context_status",
      description: "Diagnostics: version, branch, index, rg, node.",
      inputSchema: {
        type: "object",
        properties: {},
      },
      handler: async () => {
        const s = await engine.status();
        return {
          version: s.version,
          projectRoot: s.projectRoot,
          gitBranch: s.gitBranch,
          nodeVersion: s.nodeVersion,
          rgAvailable: s.rgAvailable,
          ociConnected: s.ociConnected,
          ociProvider: s.ociProvider,
          ociModel: s.ociModel,
          ociChunks: s.ociChunks,
          ociBranch: s.ociBranch,
          warnings: s.warnings,
        };
      },
    },
  ];
}

function formatResult(res: any, debug: boolean) {
  const evidence = (res.evidence ?? []).map((e: any) => ({
    file: e.file,
    lines: e.startLine ? `${e.startLine}-${e.endLine ?? e.startLine}` : undefined,
    symbol: e.symbol,
    relation: e.relation,
    source: e.source,
    provenance: debug ? e.provenance : undefined,
    score: debug ? e.score : undefined,
    authorityScore: debug ? e.authorityScore : undefined,
    finalScore: debug ? e.finalScore : undefined,
  }));
  const out: any = {
    query: res.query,
    type: res.type,
    context: res.packed?.markdown ?? "",
    evidence,
    stats: res.stats,
    warnings: res.stats?.warnings ?? [],
  };
  if (debug && res.debug) {
    out.debug = res.debug;
    out.evidence = (res.evidence ?? []).map((e: any) => ({
      file: e.file,
      lines: `${e.startLine}-${e.endLine}`,
      symbol: e.symbol,
      symbolKind: e.symbolKind,
      relation: e.relation,
      source: e.source,
      provenance: e.provenance,
      score: e.score,
      authorityScore: e.authorityScore,
      finalScore: e.finalScore,
      authorityReasons: e.authorityReasons,
      text: e.text?.slice(0, 120),
    }));
  }
  return out;
}
