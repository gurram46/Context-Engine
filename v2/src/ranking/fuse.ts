import type { Evidence, QueryType } from "../core/types.js";
import { applyAuthority, AUTHORITY_WEIGHTS } from "./authority.js";

export interface FuseOptions {
  topN?: number;
  queryType: QueryType;
  rawQuery: string;
}

function normalizePath(p: string): string {
  return p.replace(/\\/g, "/").replace(/^\.\//, "").toLowerCase();
}

function overlap(a: Evidence, b: Evidence): boolean {
  if (normalizePath(a.file) !== normalizePath(b.file)) return false;
  if (!a.startLine || !b.startLine) return false;
  // overlapping line ranges
  const s1 = a.startLine, e1 = a.endLine ?? a.startLine;
  const s2 = b.startLine, e2 = b.endLine ?? b.startLine;
  return Math.max(s1, s2) <= Math.min(e1, e2) + 2; // allow 2-line gap
}

export function fuseEvidence(evidence: Evidence[], opts: FuseOptions) {
  const topN = opts.topN ?? 10;
  if (evidence.length === 0) return { ranked: [] as any[], deduped: 0, collapsed: 0 };

  // 1. Authority scoring
  const scored = applyAuthority(evidence, opts.queryType, opts.rawQuery);

  // 2. Sort by finalScore descending, then retrieval score
  scored.sort((a, b) => b.finalScore - a.finalScore || (b.score ?? 0) - (a.score ?? 0));

  // 3. Deduplicate exact same file+symbol+line+source (keep highest)
  const seen = new Map<string, typeof scored[0]>();
  let dedupedCount = 0;
  for (const e of scored) {
    const key = `${normalizePath(e.file)}:${e.symbol ?? ""}:${e.startLine ?? 0}:${e.endLine ?? 0}:${e.source}`;
    if (!seen.has(key)) seen.set(key, e);
    else {
      dedupedCount++;
      const existing = seen.get(key)!;
      // keep higher finalScore
      if (e.finalScore > existing.finalScore) seen.set(key, e);
    }
  }
  let dedupedList = [...seen.values()];

  // 4. Collapse heavily overlapping chunks from SAME file (same symbol family)
  // If same file has many adjacent chunks, keep top 2 per file unless they are from different sources with high authority
  // This prevents 5x bundle_command.py slices from crowding out other files, but MUST NOT remove exact definition.
  const byFile = new Map<string, typeof dedupedList>();
  for (const e of dedupedList) {
    const f = normalizePath(e.file);
    if (!byFile.has(f)) byFile.set(f, []);
    byFile.get(f)!.push(e);
  }

  let collapsedList: typeof dedupedList = [];
  let collapsedCount = 0;

  for (const [file, list] of byFile) {
    // Sort file-internal by finalScore
    list.sort((a, b) => b.finalScore - a.finalScore);
    // Keep first 3 if file has >3 entries and the extras are overlapping low-authority
    const kept: typeof list = [];
    for (const e of list) {
      // Check overlap with already kept
      const hasOverlap = kept.some((k) => overlap(k, e));
      if (hasOverlap) {
        // If overlapping and current is exact definition from symbol source, keep it (correctness over diversity)
        const isDef = e.relation === "definition" && e.source === "symbol";
        const keptHasDef = kept.some((k) => k.relation === "definition");
        if (isDef && !keptHasDef) {
          kept.push(e);
        } else if (kept.length < 2) {
          // allow up to 2 overlapping if we have <2
          kept.push(e);
        } else {
          collapsedCount++;
          continue;
        }
      } else {
        if (kept.length < 4 || e.finalScore > 15) {
          kept.push(e);
        } else {
          collapsedCount++;
        }
      }
    }
    // Cap per-file to 3 unless high authority
    const finalKept = kept.length > 3 ? kept.filter((e, i) => i < 3 || e.authorityScore > 10) : kept;
    collapsedCount += Math.max(0, kept.length - finalKept.length);
    collapsedList.push(...finalKept);
  }

  // Re-sort collapsed list by finalScore
  collapsedList.sort((a, b) => b.finalScore - a.finalScore);

  // 5. Authority must not be penalized for file-diversity: ensure top exact definition survives
  // If an exact symbol definition was removed, reinsert it
  const defExists = collapsedList.some((e) => e.relation === "definition" && e.source === "symbol");
  if (!defExists) {
    const bestDef = scored.find((e) => e.relation === "definition" && e.source === "symbol");
    if (bestDef) {
      collapsedList.unshift(bestDef);
      collapsedList = collapsedList.slice(0, topN);
    }
  }

  const ranked = collapsedList.slice(0, topN);
  return { ranked, deduped: dedupedCount, collapsed: collapsedCount, weights: AUTHORITY_WEIGHTS };
}
