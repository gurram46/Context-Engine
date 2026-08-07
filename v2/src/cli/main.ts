#!/usr/bin/env node
import { routeQuery } from "../router/router.js";
import { fuseEvidence } from "../ranking/fuse.js";
import { packEvidence, countTokens } from "../packing/evidencePacker.js";
import { createCodeIndexClient } from "../retrieval/codeIndexClient.js";
import { classifyQuery } from "../router/classifyQuery.js";
import { exactSearch } from "../retrieval/exactSearch.js";

async function main() {
  const args = process.argv.slice(2);
  if (args.length === 0 || args[0] === "--help" || args[0] === "-h") {
    console.log(`Context Engine V2 CLI

  npx tsx src/cli/main.ts search "where is bundle generation implemented?"
  npx tsx src/cli/main.ts symbol count_tokens
  npx tsx src/cli/main.ts callers bundle
  npx tsx src/cli/main.ts callees _manual_fixed_bundle
  npx tsx src/cli/main.ts tests bundle
  npx tsx src/cli/main.ts debug-search "Trace the Bundle Generation Flow context bundle --no-ai to .context/context_for_ai.md"
  npx tsx src/cli/main.ts eval
  npx tsx src/cli/main.ts status
`);
    process.exit(0);
  }

  const cmd = args[0];

  if (cmd === "status") {
    const c = createCodeIndexClient();
    const s = await c.status();
    console.log(s);
    await c.close();
    return;
  }

  if (cmd === "eval") {
    const { runEval } = await import("../../eval/runner.js");
    await runEval();
    return;
  }

  if (cmd === "symbol") {
    const q = args.slice(1).join(" ");
    const c = createCodeIndexClient();
    const ev = await c.lookupImplementation(q);
    console.log(JSON.stringify(ev, null, 2));
    await c.close();
    return;
  }

  if (cmd === "callers" || cmd === "callees") {
    const sym = args[1];
    const dir = cmd === "callers" ? "callers" : "callees";
    const c = createCodeIndexClient();
    const ev = await c.callGraph(sym, dir as any);
    console.log(JSON.stringify(ev, null, 2));
    await c.close();
    return;
  }

  if (cmd === "tests") {
    const q = args.slice(1).join(" ");
    const res = await routeQuery(`tests for ${q}`);
    const fused = fuseEvidence(res.evidence, { queryType: res.classified.type, rawQuery: res.classified.raw });
    console.log(JSON.stringify({ classified: res.classified, fused: fused.ranked }, null, 2));
    return;
  }

  // search / debug-search
  let query = "";
  let debug = false;
  if (cmd === "debug-search") {
    debug = true;
    query = args.slice(1).join(" ");
  } else if (cmd === "search") {
    query = args.slice(1).join(" ");
  } else {
    query = args.join(" ");
    if (query.startsWith("debug-search ")) {
      debug = true;
      query = query.slice("debug-search ".length);
    }
  }

  if (!query) {
    console.error("Missing query");
    process.exit(1);
  }

  const t0 = Date.now();
  const res = await routeQuery(query);
  const fused = fuseEvidence(res.evidence, { queryType: res.classified.type, rawQuery: query, topN: 10 });
  const pack = packEvidence(fused.ranked as any, query, res.classified.type, { budget: 10000 });
  const elapsed = Date.now() - t0;

  console.log(`\n=== Classified: ${res.classified.type} hints:${res.classified.hints.join(",")} ===`);
  console.log(`Retrievers: ${res.timings.map(t => `${t.retriever}(${t.count}@${t.ms}ms)`).join(" | ")}`);
  console.log(`Decisions: ${res.decisions.join(" | ")}`);
  console.log(`Fused: deduped=${fused.deduped} collapsed=${fused.collapsed} topN=${fused.ranked.length} elapsed=${elapsed}ms`);

  if (debug) {
    console.log(`\n--- Raw evidence (${res.evidence.length}) ---`);
    for (const e of res.evidence.slice(0, 20)) {
      console.log(`${e.source.padEnd(8)} ${(e.score ?? 0).toFixed(2)} ${e.file}:${e.startLine ?? "?"} ${e.symbol ?? ""} ${e.symbolKind ?? ""} ${e.text?.slice(0, 60) ?? ""}`);
    }
    console.log(`\n--- Ranked (authority) ---`);
    for (const e of fused.ranked) {
      const r = (e as any).authorityReasons?.join("; ") ?? "";
      console.log(`final:${e.finalScore.toFixed(1)} (base:${(e.score ?? 0).toFixed(2)} auth:${e.authorityScore}) ${e.file}:${e.startLine} ${e.symbol ?? ""} [${e.source}] ${r}`);
    }
  } else {
    console.log(`\n--- Ranked evidence ---`);
    for (let i = 0; i < fused.ranked.length; i++) {
      const e = fused.ranked[i];
      console.log(`[${i+1}] ${e.file}:${e.startLine}-${e.endLine} ${e.symbol ?? ""} (${e.symbolKind ?? ""}) score:${(e.score ?? 0).toFixed(2)} auth:${e.authorityScore} final:${e.finalScore.toFixed(1)} [${e.source}]`);
    }
  }

  console.log(`\n--- Packed (est ${pack.tokenEstimate} tokens, budget 10000) ---`);
  console.log(pack.markdown.slice(0, 6000));

  // quick exact fallback debug for dependency
  if (res.classified.type === "DEPENDENCY" && fused.ranked.length < 3) {
    console.log(`\n[debug] dependency had low results, showing exact fallback first 3:`);
    const rg = await exactSearch(query.split(/\s+/).pop() || "", { literal: true, limit: 5 });
    console.log(rg.slice(0,3));
  }

  // ensure exit
  setTimeout(() => process.exit(0), 500);
}

main().catch((e) => { console.error(e); process.exit(1); });
