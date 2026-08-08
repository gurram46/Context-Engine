import { routeQuery } from "../router/router.js";
import { fuseEvidence } from "../ranking/fuse.js";
import { packEvidence } from "../packing/evidencePacker.js";
import { createCodeIndexClient } from "../retrieval/codeIndexClient.js";
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import type { Evidence, QueryType } from "./types.js";

export interface ContextResult {
  query: string;
  type: QueryType;
  evidence: Array<Evidence & { authorityScore: number; finalScore: number }>;
  packed: { markdown: string; tokenEstimate: number; files: string[] };
  stats: { retrievers: string[]; elapsedMs: number; tokenEstimate: number; warnings: string[] };
  debug?: {
    rawEvidenceCount: number;
    timings: Array<{ retriever: string; ms: number; count: number }>;
    decisions: string[];
    authorityWeights: Record<string, number>;
  };
}

export interface ContextStatus {
  version: string;
  projectRoot: string;
  gitBranch?: string;
  nodeVersion: string;
  rgAvailable: boolean;
  ociConnected: boolean;
  ociProvider?: string;
  ociModel?: string;
  ociChunks?: number;
  ociBranch?: string;
  warnings: string[];
}

function getVersion(): string {
  try {
    const pkg = JSON.parse(readFileSync(path.resolve("v2/package.json"), "utf8"));
    return pkg.version ?? "0.1.0";
  } catch { return "0.1.0"; }
}

function getGitBranch(): string | undefined {
  try {
    return execSync("git branch --show-current", { encoding: "utf8" }).trim() || undefined;
  } catch { return undefined; }
}

async function checkRg(): Promise<boolean> {
  try {
    execSync("rg --version", { stdio: "ignore" });
    return true;
  } catch { return false; }
}

export class ContextEngine {
  private client = createCodeIndexClient();
  private closed = false;

  async search(query: string, opts: { budgetTokens?: number; maxResults?: number; debug?: boolean } = {}): Promise<ContextResult> {
    return this.execute(query, opts);
  }

  async symbol(name: string, opts: { budgetTokens?: number; debug?: boolean } = {}): Promise<ContextResult> {
    // Ensure SYMBOL classification by passing bare symbol
    return this.execute(name.trim(), opts);
  }

  async callers(symbol: string, opts: { budgetTokens?: number; debug?: boolean } = {}): Promise<ContextResult> {
    return this.execute(`What calls ${symbol}?`, opts);
  }

  async callees(symbol: string, opts: { budgetTokens?: number; debug?: boolean } = {}): Promise<ContextResult> {
    return this.execute(`What does ${symbol} call?`, opts);
  }

  async dependency(symbol: string, direction: "callers" | "callees" | "both" = "callers", opts: { budgetTokens?: number; debug?: boolean } = {}): Promise<ContextResult> {
    if (direction === "callers") return this.callers(symbol, opts);
    if (direction === "callees") return this.callees(symbol, opts);
    // both: run callers and combine
    const a = await this.callers(symbol, opts);
    const b = await this.callees(symbol, opts);
    // Merge evidence already handled; for both we just do a mixed query
    return this.execute(`dependency of ${symbol}`, opts);
  }

  async tests(queryOrSymbol: string, opts: { budgetTokens?: number; debug?: boolean } = {}): Promise<ContextResult> {
    // Force TEST classification
    const q = queryOrSymbol.toLowerCase().includes("test") ? queryOrSymbol : `What tests cover ${queryOrSymbol}?`;
    return this.execute(q, opts);
  }

  async status(): Promise<ContextStatus> {
    const warnings: string[] = [];
    const rgAvailable = await checkRg();
    if (!rgAvailable) warnings.push("rg not available");
    let ociConnected = false;
    let ociProvider: string | undefined;
    let ociModel: string | undefined;
    let ociChunks: number | undefined;
    let ociBranch: string | undefined;
    try {
      const raw = await this.client.status();
      ociConnected = true;
      // Parse "Provider: ollama" etc
      const prov = raw.match(/Provider:\s*(\S+)/);
      const model = raw.match(/Model:\s*(\S+)/);
      const chunks = raw.match(/Indexed chunks:\s*([\d,]+)/);
      const branch = raw.match(/Current branch:\s*(\S+)/);
      if (prov) ociProvider = prov[1];
      if (model) ociModel = model[1];
      if (chunks) ociChunks = Number(chunks[1].replace(/,/g, ""));
      if (branch) ociBranch = branch[1];
      if (raw.includes("not indexed") || raw.includes("not indexed")) warnings.push("index not ready");
    } catch (e: any) {
      warnings.push(`oci status failed: ${e.message}`);
    }
    return {
      version: getVersion(),
      projectRoot: process.cwd(),
      gitBranch: getGitBranch(),
      nodeVersion: process.version,
      rgAvailable,
      ociConnected,
      ociProvider,
      ociModel,
      ociChunks,
      ociBranch,
      warnings,
    };
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    await this.client.close().catch(() => {});
  }

  private async execute(rawQuery: string, opts: { budgetTokens?: number; maxResults?: number; debug?: boolean }): Promise<ContextResult> {
    const t0 = Date.now();
    const warnings: string[] = [];
    let result: Awaited<ReturnType<typeof routeQuery>> | null = null;
    try {
      result = await routeQuery(rawQuery);
    } catch (e: any) {
      // Fallback to exact only if OCI fails
      warnings.push(`retrieval failed: ${e.message}, falling back to exact`);
      const { exactSearch } = await import("../retrieval/exactSearch.js");
      const ev = await exactSearch(rawQuery, { literal: true, limit: opts.maxResults ?? 10 }).catch(() => []);
      result = {
        classified: { type: "MIXED" as QueryType, raw: rawQuery, normalized: rawQuery, hints: ["fallback-exact"] },
        evidence: ev,
        timings: [{ retriever: "exact-fallback", ms: Date.now() - t0, count: ev.length }],
        decisions: ["fallback"],
      };
    }
    if (!result) throw new Error("empty result");
    const fused = fuseEvidence(result.evidence, {
      queryType: result.classified.type,
      rawQuery,
      topN: opts.maxResults ?? 10,
    });
    const packed = packEvidence(fused.ranked as any, rawQuery, result.classified.type, {
      budget: opts.budgetTokens ?? 8000,
      maxFiles: opts.maxResults ?? 10,
    });
    const elapsed = Date.now() - t0;
    if (fused.ranked.length === 0) warnings.push("no evidence found");
    return {
      query: rawQuery,
      type: result.classified.type,
      evidence: fused.ranked as any,
      packed,
      stats: {
        retrievers: result.timings.map((t) => `${t.retriever}:${t.count}`),
        elapsedMs: elapsed,
        tokenEstimate: packed.tokenEstimate,
        warnings,
      },
      debug: opts.debug
        ? {
            rawEvidenceCount: result.evidence.length,
            timings: result.timings,
            decisions: result.decisions,
            authorityWeights: fused.weights as any,
          }
        : undefined,
    };
  }
}

let singleton: ContextEngine | null = null;
export function getContextEngine(): ContextEngine {
  if (!singleton) singleton = new ContextEngine();
  return singleton;
}
export async function closeContextEngine(): Promise<void> {
  if (singleton) {
    await singleton.close();
    singleton = null;
  }
}
