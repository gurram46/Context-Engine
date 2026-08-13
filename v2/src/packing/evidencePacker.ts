import type { Evidence, QueryType } from "../core/types.js";
import { encoding_for_model } from "tiktoken";

let enc: ReturnType<typeof encoding_for_model> | null = null;
function getEnc() {
  if (!enc) {
    try {
      enc = encoding_for_model("gpt-4");
    } catch {
      enc = encoding_for_model("cl100k_base" as any);
    }
  }
  return enc!;
}

export function countTokens(text: string): number {
  try {
    return getEnc().encode(text).length;
  } catch {
    return Math.ceil(text.length / 4);
  }
}

export interface PackOptions {
  budget?: number; // tokens
  maxFiles?: number;
}

export function packEvidence(
  ranked: Array<Evidence & { finalScore: number; authorityScore: number }>,
  query: string,
  queryType: QueryType,
  opts: PackOptions = {},
): { markdown: string; tokenEstimate: number; files: string[] } {
  const budget = opts.budget ?? 10000;
  const maxFiles = opts.maxFiles ?? 10;
  // Collapse adjacent line ranges per file
  const byFile = new Map<string, typeof ranked>();
  for (const e of ranked.slice(0, maxFiles * 2)) {
    const k = e.file;
    if (!byFile.has(k)) byFile.set(k, []);
    byFile.get(k)!.push(e);
  }

  let lines: string[] = [];
  lines.push(`# Evidence Pack — ${queryType}`);
  lines.push(`> Query: ${query}`);
  lines.push(``);

  let totalTokens = countTokens(lines.join("\n"));
  const files: string[] = [];
  let added = 0;

  for (const [file, items] of byFile) {
    if (added >= maxFiles) break;
    // Merge line ranges for this file
    const sorted = [...items].sort((a, b) => (a.startLine ?? 0) - (b.startLine ?? 0));
    const ranges = sorted.map((e) => `${e.startLine ?? "?"}-${e.endLine ?? "?"}`).join(", ");
    const symbols = [...new Set(sorted.map((e) => e.symbol).filter(Boolean))].join(", ");
    const sources = [...new Set(sorted.map((e) => e.source))].join("+");
    const header = `## ${file} ${symbols ? `(${symbols})` : ""} [${sources}] lines ${ranges}`;
    const bodyLines: string[] = [header];
    for (const e of sorted) {
      const loc = e.startLine ? `${e.file}:${e.startLine}-${e.endLine}` : e.file;
      const score = `score:${(e.score ?? 0).toFixed(2)} authority:${e.authorityScore} final:${e.finalScore.toFixed(1)}`;
      bodyLines.push(`- ${loc} ${e.symbolKind ?? ""} ${e.symbol ?? ""} (${score}) ${e.text ? `— ${e.text.slice(0, 120)}` : ""} [${e.provenance ?? e.source}]`);
    }
    bodyLines.push("");
    const chunk = bodyLines.join("\n");
    const chunkTokens = countTokens(chunk);
    if (totalTokens + chunkTokens > budget) break;
    lines.push(chunk);
    totalTokens += chunkTokens;
    files.push(file);
    added++;
  }

  const markdown = lines.join("\n");
  return { markdown, tokenEstimate: totalTokens, files };
}
