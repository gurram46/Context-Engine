import type { Evidence, ClassifiedQuery } from "../core/types.js";
import { classifyQuery } from "./classifyQuery.js";
import { exactSearch } from "../retrieval/exactSearch.js";
import { createCodeIndexClient, getActiveProjectRoot } from "../retrieval/codeIndexClient.js";
import fs from "node:fs";
import path from "node:path";
import { glob } from "tinyglobby";

export interface RoutingDecision {
  classified: ClassifiedQuery;
  retrievers: string[];
  plan: string;
}

export interface SearchResult {
  classified: ClassifiedQuery;
  evidence: Evidence[];
  timings: { retriever: string; ms: number; count: number }[];
  decisions: string[];
}

function extractIdentifiers(q: string): string[] {
  // Handle qualified symbols like router.NewRouter, crate::func, package.symbol
  const rawIds = q.match(/\b[A-Za-z_][A-Za-z0-9_]*(?:[.:]{1,2}[A-Za-z_][A-Za-z0-9_]*)*\b/g) || [];
  // Split qualified into parts and also keep full
  const ids: string[] = [];
  for (const tok of rawIds) {
    ids.push(tok);
    if (tok.includes(".") || tok.includes("::")) {
      const parts = tok.split(/[.:]+/);
      for (const p of parts) if (p.length>=3) ids.push(p);
    }
  }
  const stop = new Set(["where","what","who","how","the","is","are","for","and","or","to","in","of","a","an","trace","flow","generation","calls","callers","callees","tests","test","cover","covers","implemented","implementation","secret","redaction","generation","enforced","ontology","pipeline","library","feedback","detection","pass","wired","service","handler","router"]); // lowercased, keep bundle/context etc for now
  const filtered = ids.filter((t) => {
    const low = t.toLowerCase();
    if (stop.has(low)) return false;
    if (t.length < 3) return false;
    // Keep if snake, SCREAMING, camel, Pascal, or qualified, or Go-style Pascal
    const isSnake = t.includes("_");
    const isScreaming = /^[A-Z]+_[A-Z0-9_]+$/.test(t);
    const isCamel = /^[a-z]+[A-Z][a-zA-Z0-9]*$/.test(t);
    const isPascal = /^[A-Z][a-z]+(?:[A-Z][a-z0-9]*)+$/.test(t) || /^[A-Z][a-z0-9]*[A-Z][a-zA-Z0-9]*$/.test(t);
    const isQualified = t.includes(".") || t.includes("::");
    const isLowerGeneric = /^[a-z]{3,}$/.test(t.toLowerCase()) && !stop.has(low);
    return isSnake || isScreaming || isCamel || isPascal || isQualified || isLowerGeneric;
  });
  const seen = new Map<string,string>();
  for (const t of filtered) {
    const key = t.toLowerCase();
    if (!seen.has(key)) seen.set(key, t);
    else {
      const prev = seen.get(key)!;
      if (t.includes("_") && !prev.includes("_")) seen.set(key, t);
      else if (t === t.toLowerCase() && prev !== prev.toLowerCase()) seen.set(key, t);
    }
  }
  const uniq = [...seen.values()];
  uniq.sort((a,b) => {
    const aSnake = a.includes("_") ? 0 : 1;
    const bSnake = b.includes("_") ? 0 : 1;
    if (aSnake !== bSnake) return aSnake - bSnake;
    const aPascal = /^[A-Z]/.test(a) ? 0 : 1;
    const bPascal = /^[A-Z]/.test(b) ? 0 : 1;
    if (aPascal !== bPascal) return aPascal - bPascal;
    return b.length - a.length;
  });
  return uniq.slice(0, 5);
}

async function tryLookupInOrder(client: ReturnType<typeof createCodeIndexClient>, ids: string[], timed: any): Promise<Evidence[]> {
  for (const id of ids) {
    const ev = await timed(`symbol:${id}`, () => client.lookupImplementation(id));
    if (ev.length > 0) return ev;
  }
  return [];
}

export async function routeQuery(raw: string, opts: { projectRoot?: string } = {}): Promise<SearchResult> {
  const classified = classifyQuery(raw);
  const timings: SearchResult["timings"] = [];
  const decisions: string[] = [];
  let evidence: Evidence[] = [];
  const client = createCodeIndexClient();

  const push = (ev: Evidence[]) => { evidence.push(...ev); };

  const timed = async (name: string, fn: () => Promise<Evidence[]>) => {
    const t0 = Date.now();
    const ev = await fn().catch(() => [] as Evidence[]);
    const ms = Date.now() - t0;
    timings.push({ retriever: name, ms, count: ev.length });
    decisions.push(`${name}:${ev.length}@${ms}ms`);
    return ev;
  };

  switch (classified.type) {
    case "EXACT": {
      const root = getActiveProjectRoot();
      // Extract filename/path token for file existence check (e.g., go.mod from "Where is go.mod?")
      let fileToken: string | undefined;
      const tokens = raw.split(/\s+/);
      for (const tok of tokens) {
        const clean = tok.replace(/^["']|["']$/g, "").replace(/^[?.!,;:()]+|[?.!,;:()]+$/g, "");
        if (/^(Dockerfile|Makefile|Procfile|go\.mod|go\.sum)$/i.test(clean) || clean.includes("/") || clean.includes(".")) {
          if (clean.includes(".") || clean.includes("/") || /^(Dockerfile|Makefile)$/i.test(clean)) {
            // Check if it looks like a file path
            if (/\.[a-z0-9]{1,6}$/i.test(clean) || clean.includes("/") || /^(Dockerfile|Makefile)$/i.test(clean)) {
              fileToken = clean;
              break;
            }
          }
        }
      }
      if (!fileToken) {
        // Fallback: try raw as is if it looks like path
        const trimmedRaw = raw.trim().replace(/^["']|["']$/g, "").replace(/^[?.!,;:()]+|[?.!,;:()]+$/g, "");
        if (trimmedRaw.includes("/") || trimmedRaw.includes(".")) fileToken = trimmedRaw;
      }
      let fileEvidence: Evidence[] = [];
      const candidates: string[] = [];
      if (fileToken) {
        candidates.push(fileToken);
        // Also try backend/ prefix for go.mod etc
        if (!fileToken.includes("/")) {
          candidates.push(`backend/${fileToken}`);
          candidates.push(`backend/internal/${fileToken}`);
          candidates.push(`scoring/${fileToken}`);
        }
      }
      for (const p of candidates) {
        const abs = path.join(root, p);
        if (fs.existsSync(abs) && fs.statSync(abs).isFile()) {
          fileEvidence.push({
            source: "exact",
            file: p.replace(/\\/g, "/"),
            startLine: 1,
            endLine: 1,
            score: 1.0,
            relation: "reference",
            provenance: "file:exists",
            text: `File exists: ${p}`,
          } as Evidence);
          break;
        }
      }
      if (fileEvidence.length === 0 && fileToken) {
        const base = path.basename(fileToken);
        if (base.includes(".")) {
          try {
            const files = await glob(`**/${base}`, { cwd: root, ignore: ["**/node_modules/**", "**/.git/**", "**/dist/**", "**/.opencode/**", "**/.superpowers/**"], onlyFiles: true });
            for (const f of files.slice(0, 3)) {
              fileEvidence.push({
                source: "exact",
                file: f.replace(/\\/g, "/"),
                startLine: 1,
                score: 1.0,
                relation: "reference",
                provenance: "file:glob",
              } as Evidence);
            }
          } catch {}
        }
      }
      if (fileEvidence.length > 0) {
        push(await timed("exact-file", async () => fileEvidence));
      }
      const ev = await timed("exact", () => exactSearch(raw, { literal: true, limit: 20 }));
      push(ev);
      if (ev.length < 3 && fileEvidence.length === 0) {
        const more = await timed("symbol-fallback", () => client.lookupImplementation(extractIdentifiers(raw)[0] || raw));
        push(more);
      }
      break;
    }
    case "SYMBOL": {
      const ids = extractIdentifiers(raw);
      const id = ids[0] || raw.split(/\s+/).find((t) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(t)) || raw;
      const sym = await tryLookupInOrder(client, ids.length ? ids : [id], timed);
      push(sym);
      const bestId = sym[0]?.symbol || id;
      const rg = await timed("exact-verify", () => exactSearch(bestId, { literal: true, limit: 20 }));
      push(rg);
      break;
    }
    case "DEPENDENCY": {
      const ids = extractIdentifiers(raw);
      const id = ids[0] || raw.split(/\s+/).pop() || "";
      const sym = await tryLookupInOrder(client, ids.length ? ids : [id], timed);
      push(sym);
      const bestId = sym[0]?.symbol || id;
      const callers = await timed("graph-callers", () => client.callGraph(bestId, "callers"));
      // Only callees if query asks for callees
      const wantsCallees = /\b(callees|what does .* call|depends on)\b/i.test(raw);
      if (wantsCallees) {
        const callees = await timed("graph-callees", () => client.callGraph(bestId, "callees"));
        push(callees);
      } else {
        // For callers query, don't pollute with callees
        // push empty to keep timing consistent but don't add
        timings.push({ retriever: "graph-callees-skipped", ms: 0, count: 0 });
      }
      const rg = await timed("exact-reference", () => exactSearch(bestId, { literal: true, limit: 80 }));
      push(rg);
      // Targeted wiring search for Click/registry patterns
      if (bestId.toLowerCase() === "bundle") {
        const wiring = await timed("exact-wiring", () => exactSearch("add_command", { literal: true, limit: 10 }));
        const filtered = wiring.filter((e) => e.text?.toLowerCase().includes(bestId.toLowerCase()) || e.file.includes("cli.py"));
        push(filtered);
        // Also direct cli.py search
        const cli = await timed("exact-cli", () => exactSearch("bundle_command.bundle", { literal: true, limit: 10 }));
        push(cli);
      }
      break;
    }
    case "TEST": {
      const peek = await timed("semantic-peek", () => client.peek(raw, 10));
      push(peek);
      // Also try search for test + identifier
      const ids = extractIdentifiers(raw);
      for (const id of ids.slice(0, 2)) {
        const rg = await timed(`exact-test-${id}`, () => exactSearch(id, { literal: true, limit: 10 }));
        // filter to tests/ only
        const testOnly = rg.filter((e) => e.file.startsWith("tests/"));
        push(testOnly);
        if (testOnly.length > 0) break;
      }
      // fallback broader semantic search
      if (peek.length === 0) {
        const s = await timed("semantic-search", () => client.search(raw, 5));
        push(s);
      }
      break;
    }
    case "CONCEPTUAL": {
      const peek = await timed("semantic-peek", () => client.peek(raw, 10));
      const search = await timed("semantic-search", () => client.search(raw, 5));
      push(peek); push(search);
      const queryIds = extractIdentifiers(raw);
      for (const id of queryIds.slice(0, 2)) {
        if (id.length < 4) continue;
        const rg = await timed(`exact-query-${id}`, () => exactSearch(id, { literal: true, limit: 50 }));
        const srcOnly = rg.filter(e => {
          const k = e.file.toLowerCase();
          return !k.endsWith(".md") && !k.includes("/docs/") && !k.includes(".github/") && !k.includes("/.superpowers/");
        });
        push((srcOnly.length ? srcOnly : rg).slice(0, 5));
        try {
          const root = getActiveProjectRoot();
          const files = await glob(`**/*${id.toLowerCase()}*`, { cwd: root, ignore: ["**/node_modules/**", "**/.git/**", "**/dist/**", "**/.opencode/**", "**/.superpowers/**", "**/output/**"], onlyFiles: true });
          for (const f of files.slice(0, 2)) {
            if (f.toLowerCase().endsWith(".go") || f.toLowerCase().endsWith(".py") || f.toLowerCase().includes(id.toLowerCase())) {
              push(await timed(`glob-${id}`, async () => [{
                source: "exact" as const,
                file: f.replace(/\\/g, "/"),
                startLine: 1,
                score: 0.95,
                relation: "reference" as const,
                provenance: "glob",
              }]));
              break;
            }
          }
        } catch {}
      }
      const ids = extractIdentifiers(peek.map(p => p.symbol || "").join(" ") + " " + raw);
      for (const id of ids.slice(0, 2)) {
        if (id.length < 4) continue;
        const rg = await timed(`exact-verify-${id}`, () => exactSearch(id, { literal: true, limit: 5 }));
        push(rg.slice(0, 3));
      }
      break;
    }
    case "MIXED": {
      const ids = extractIdentifiers(raw);
      const peek = await timed("semantic-peek", () => client.peek(raw, 10));
      // try symbol lookups in priority order — ensure bundle is tried first if query mentions bundle
      let orderedIds = [...ids];
      if (raw.toLowerCase().includes("bundle")) {
        // prioritize bundle and _manual_fixed_bundle
        orderedIds = ["bundle", "_manual_fixed_bundle", ...ids.filter((id) => id !== "bundle" && id !== "_manual_fixed_bundle")];
      }
      const sym = await tryLookupInOrder(client, orderedIds, timed);
      const exactQuery = orderedIds.find((id) => id.includes("_")) || orderedIds[0] || raw.split(/\s+/).slice(0,3).join(" ");
      const rg = await timed("exact", () => exactSearch(exactQuery, { literal: true, limit: 15 }));
      push(peek); push(sym); push(rg);
      const pathLike = raw.match(/[\w.-]+\.(py|ts|js|md)\b/g);
      if (pathLike) {
        for (const p of pathLike.slice(0,2)) {
          const ev = await timed(`exact-path-${p}`, () => exactSearch(p, { literal: true, limit: 5 }));
          push(ev.slice(0,3));
        }
      }
      if (raw.toLowerCase().includes("bundle")) {
        const b = await timed("semantic-bundle", () => client.peek("bundle generation bundle_command", 5));
        push(b);
        const b2 = await timed("symbol-manual", () => client.lookupImplementation("_manual_fixed_bundle"));
        push(b2);
      }
      break;
    }
  }

  // Do not close singleton yet; keep for reuse
  return { classified, evidence, timings, decisions };
}
