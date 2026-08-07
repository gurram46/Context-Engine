import fs from "node:fs";
import path from "node:path";
import { routeQuery } from "../src/router/router.js";
import { fuseEvidence } from "../src/ranking/fuse.js";

interface Case {
  id: string;
  query: string;
  type: string;
  expectedFiles: string[];
  expectedSymbols?: string[];
  mustNotTop?: string[];
}

export async function runEval() {
  const cases: Case[] = JSON.parse(fs.readFileSync(path.resolve("v2/eval/retrieval-cases.json"), "utf8"));
  console.log(`\n=== V2 Retrieval Eval — ${cases.length} cases ===\n`);
  let rows: any[] = [];
  for (const c of cases) {
    const t0 = Date.now();
    const res = await routeQuery(c.query);
    const fused = fuseEvidence(res.evidence, { queryType: res.classified.type as any, rawQuery: c.query, topN: 10 });
    const elapsed = Date.now() - t0;
    const top1 = fused.ranked[0]?.file ?? "";
    const top5 = fused.ranked.slice(0, 5).map((e) => e.file);
    const top10 = fused.ranked.slice(0, 10).map((e) => e.file);
    const syms = fused.ranked.map((e) => e.symbol).filter(Boolean) as string[];
    const calls = res.timings.length;
    const ms = res.timings.reduce((a, b) => a + b.ms, 0);

    const top1Hit = c.expectedFiles.some((f) => top1.includes(path.basename(f)) || top1 === f);
    const top5Hits = c.expectedFiles.filter((f) => top5.some((t) => t.includes(path.basename(f)) || t === f)).length;
    const symHit = !c.expectedSymbols || c.expectedSymbols.some((s) => syms.includes(s) || fused.ranked.some((e) => e.file.includes("bundle") && syms.includes(s)));
    // More precise: check any expected symbol appears in ranked symbols
    const symFound = !c.expectedSymbols ? true : c.expectedSymbols.some((s) => fused.ranked.some((e) => e.symbol === s));
    const mustNotViolation = c.mustNotTop ? fused.ranked.slice(0, 2).some((e) => c.mustNotTop!.some((m) => e.file.includes(m))) : false;

    rows.push({
      id: c.id,
      classified: res.classified.type,
      top1: top1 || "(none)",
      top1Hit: top1Hit ? "YES" : "NO",
      top5: `${top5Hits}/${c.expectedFiles.length}`,
      sym: symFound ? "YES" : "NO",
      mustNotTop: mustNotViolation ? "VIOLATION" : "ok",
      calls,
      ms,
      elapsed,
    });

    console.log(`Case ${c.id}: "${c.query.slice(0, 60)}"`);
    console.log(`  classified: ${res.classified.type} hints:${res.classified.hints.join(",")}`);
    console.log(`  top1: ${top1} -> ${top1Hit ? "HIT" : "MISS"}`);
    console.log(`  top5: ${top5.join(" | ")}`);
    console.log(`  top10: ${top10.join(" | ")}`);
    console.log(`  symbols: ${syms.slice(0,5).join(", ")} -> symFound:${symFound}`);
    if (c.mustNotTop) console.log(`  mustNotTop (ui/index.js etc in top2): ${mustNotViolation ? "FAIL" : "PASS"}`);
    console.log(`  timings: ${res.timings.map((t) => `${t.retriever}:${t.count}@${t.ms}ms`).join(" | ")} elapsed ${elapsed}ms`);
    console.log("");

    if (c.id === "bundle-flow") {
      console.log(`  --- DEBUG bundle-flow ranked ---`);
      for (let i=0;i<fused.ranked.length;i++) {
        const e = fused.ranked[i];
        console.log(`  [${i+1}] final:${e.finalScore.toFixed(1)} auth:${e.authorityScore} src:${e.source} ${e.file}:${e.startLine} ${e.symbol ?? ""} (${e.symbolKind ?? ""}) score:${(e.score ?? 0).toFixed(2)}`);
      }
      console.log(`  --- raw evidence sample ---`);
      for (const e of res.evidence.slice(0,8)) {
        console.log(`    ${e.source} ${e.file}:${e.startLine} ${e.symbol ?? ""} ${e.score?.toFixed(2)}`);
      }
      console.log("");
    }
  }

  console.log(`\n=== SUMMARY TABLE ===`);
  console.table(rows);
  const top1Rate = rows.filter((r) => r.top1Hit === "YES").length / rows.length;
  const symRate = rows.filter((r) => r.sym === "YES").length / rows.length;
  const violations = rows.filter((r) => r.mustNotTop === "VIOLATION").length;
  console.log(`Top1 hit rate: ${(top1Rate*100).toFixed(0)}% (${rows.filter(r=>r.top1Hit==="YES").length}/${rows.length})`);
  console.log(`Symbol found: ${(symRate*100).toFixed(0)}%`);
  console.log(`mustNotTop violations: ${violations}`);
  if (violations > 0 || top1Rate < 0.6) {
    console.log(`Verdict: NEEDS_ROUTER_FIXES`);
  } else {
    console.log(`Verdict: READY_FOR_PHASE_2_MCP`);
  }
}

// allow direct run
if (import.meta.url.endsWith("runner.ts") || process.argv[1]?.includes("runner")) {
  runEval().then(() => setTimeout(()=>process.exit(0), 800)).catch((e)=>{ console.error(e); process.exit(1); });
}
