#!/usr/bin/env node
import { getContextEngine } from "../core/contextEngine.js";

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

  const engine = getContextEngine();

  if (cmd === "status") {
    const s = await engine.status();
    console.log(JSON.stringify(s, null, 2));
    await engine.close();
    process.exit(0);
  }

  if (cmd === "eval") {
    // @ts-ignore - eval is outside src rootDir, loaded dynamically at runtime
    const { runEval } = await import("../../eval/runner.js");
    await runEval();
    await engine.close();
    process.exit(0);
  }

  if (cmd === "symbol") {
    const q = args.slice(1).join(" ");
    const res = await engine.symbol(q, { debug: false });
    console.log(JSON.stringify({ query: res.query, type: res.type, evidence: res.evidence, stats: res.stats }, null, 2));
    await engine.close();
    process.exit(0);
  }

  if (cmd === "callers" || cmd === "callees") {
    const sym = args[1];
    if (!sym) { console.error("Missing symbol"); process.exit(1); }
    const r = cmd === "callers" ? await engine.callers(sym) : await engine.callees(sym);
    console.log(JSON.stringify({ query: r.query, type: r.type, evidence: r.evidence, stats: r.stats, packed: r.packed.markdown.slice(0, 2000) }, null, 2));
    await engine.close();
    process.exit(0);
  }

  if (cmd === "tests") {
    const q = args.slice(1).join(" ");
    const res = await engine.tests(q);
    console.log(JSON.stringify({ query: res.query, type: res.type, evidence: res.evidence, stats: res.stats }, null, 2));
    await engine.close();
    process.exit(0);
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

  const res = await engine.search(query, { budgetTokens: 10000, maxResults: 10, debug });
  console.log(`\n=== Classified: ${res.type} ===`);
  console.log(`Retrievers: ${res.stats.retrievers.join(" | ")}`);
  console.log(`Elapsed: ${res.stats.elapsedMs}ms tokens:${res.stats.tokenEstimate}`);
  if (res.stats.warnings.length) console.log(`Warnings: ${res.stats.warnings.join("; ")}`);

  if (debug && res.debug) {
    console.log(`\n--- Raw evidence (${res.debug.rawEvidenceCount}) ---`);
    // debug already contains evidence with authority, so show top
    console.log(`Decisions: ${res.debug.decisions.join(" | ")}`);
    console.log(`\n--- Ranked (authority) ---`);
    for (const e of res.evidence.slice(0, 10) as any[]) {
      const r = (e as any).authorityReasons?.join("; ") ?? "";
      console.log(`final:${e.finalScore.toFixed(1)} (base:${(e.score ?? 0).toFixed(2)} auth:${e.authorityScore}) ${e.file}:${e.startLine} ${e.symbol ?? ""} [${e.source}] ${r}`);
    }
  } else {
    console.log(`\n--- Ranked evidence ---`);
    for (let i = 0; i < res.evidence.length; i++) {
      const e = res.evidence[i] as any;
      console.log(`[${i+1}] ${e.file}:${e.startLine}-${e.endLine} ${e.symbol ?? ""} (${e.symbolKind ?? ""}) score:${(e.score ?? 0).toFixed(2)} auth:${e.authorityScore} final:${e.finalScore.toFixed(1)} [${e.source}]`);
    }
  }

  console.log(`\n--- Packed (est ${res.packed.tokenEstimate} tokens) ---`);
  console.log(res.packed.markdown.slice(0, 6000));

  await engine.close();
  process.exit(0);
}

main().catch((e) => { console.error(e); process.exit(1); });
