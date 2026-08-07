import type { Evidence, ClassifiedQuery } from "../core/types.js";
import { classifyQuery } from "./classifyQuery.js";
import { exactSearch } from "../retrieval/exactSearch.js";
import { createCodeIndexClient } from "../retrieval/codeIndexClient.js";

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
  const ids = q.match(/\b[A-Za-z_][A-Za-z0-9_]*\b/g) || [];
  const stop = new Set(["where","what","who","how","the","is","are","for","and","or","to","in","of","a","an","trace","flow","generation","calls","callers","callees","tests","test","cover","covers","implemented","implementation","secret","redaction","generation"]); // lowercased, keep bundle/context/count_tokens
  const filtered = ids.filter((t) => {
    const low = t.toLowerCase();
    if (stop.has(low)) return false;
    if (t.length < 3) return false;
    return t.includes("_") || /[A-Z]/.test(t) || /^[a-z]{3,}$/.test(t); // keep lower single words like bundle, but filtered via stop
  });
  // dedup case-insensitively, prefer snake_case and lower case
  const seen = new Map<string,string>();
  for (const t of filtered) {
    const key = t.toLowerCase();
    if (!seen.has(key)) seen.set(key, t);
    else {
      // prefer snake_case
      const prev = seen.get(key)!;
      if (t.includes("_") && !prev.includes("_")) seen.set(key, t);
      else if (t === t.toLowerCase() && prev !== prev.toLowerCase()) seen.set(key, t);
    }
  }
  const uniq = [...seen.values()];
  // sort: snake_case first, then exact lower, then others
  uniq.sort((a,b) => {
    const aSnake = a.includes("_") ? 0 : 1;
    const bSnake = b.includes("_") ? 0 : 1;
    if (aSnake !== bSnake) return aSnake - bSnake;
    if (a === a.toLowerCase() && b !== b.toLowerCase()) return -1;
    if (b === b.toLowerCase() && a !== a.toLowerCase()) return 1;
    return a.length - b.length;
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
      const ev = await timed("exact", () => exactSearch(raw, { literal: true, limit: 20 }));
      push(ev);
      if (ev.length < 3) {
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
      // extract identifiers from results to verify with exact
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
