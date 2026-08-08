import { spawn } from "node:child_process";
import path from "node:path";
import type { Evidence } from "../core/types.js";
import { getActiveProjectRoot } from "./codeIndexClient.js";

export interface ExactSearchOptions {
  projectRoot?: string;
  literal?: boolean; // true -> -F fixed strings, false -> regex
  caseSensitive?: boolean;
  limit?: number; // max results
  timeoutMs?: number;
  cwd?: string;
}

const DEFAULT_IGNORES = [
  ".git",
  ".context",
  "node_modules",
  ".opencode/index",
  "dist",
  "build",
  "__pycache__",
  ".pytest_cache",
  "coverage",
  ".next",
  ".nuxt",
];

function normalizeProjectRoot(root?: string): string {
  if (root) return path.resolve(root);
  try { return getActiveProjectRoot(); } catch { return process.cwd(); }
}

function normalizeFile(file: string, root: string): string {
  const abs = path.isAbsolute(file) ? file : path.join(root, file);
  const rel = path.relative(root, abs);
  return rel.split(path.sep).join("/");
}

function isIgnored(rel: string): boolean {
  const lower = rel.toLowerCase();
  return DEFAULT_IGNORES.some((ig) => lower === ig || lower.startsWith(ig + "/") || lower.includes(`/${ig}/`));
}

export async function exactSearch(
  query: string,
  options: ExactSearchOptions = {},
): Promise<Evidence[]> {
  if (!query || !query.trim()) return [];
  const raw = query.trim();
  // detect quoted literal: "foo" or 'foo' -> strip quotes
  let literal = options.literal ?? true;
  let searchTerm = raw;
  const quoted = raw.match(/^["'](.+)["']$/);
  if (quoted) {
    searchTerm = quoted[1];
    literal = true;
  }
  const limit = Math.min(Math.max(options.limit ?? 50, 1), 200);
  const timeoutMs = options.timeoutMs ?? 5000;
  const projectRoot = normalizeProjectRoot(options.cwd ?? options.projectRoot);

  // Build rg args without shell
  const args: string[] = [
    "--line-number",
    "--column",
    "--no-heading",
    "--color",
    "never",
    "--max-count",
    String(limit),
    "--hidden",
    "--glob",
    "!.git/**",
  ];
  // respects .gitignore by default for rg; --hidden includes but we filter
  if (literal) args.push("--fixed-strings");
  if (options.caseSensitive === false) args.push("--ignore-case");
  // ignore dirs via -g
  for (const ig of DEFAULT_IGNORES) {
    args.push("-g", `!${ig}/**`);
    args.push("-g", `!**/${ig}/**`);
  }
  args.push("--", searchTerm, ".");

  const rg = spawn("rg", args, { cwd: projectRoot, stdio: ["ignore", "pipe", "pipe"] });

  let stdout = "";
  let stderr = "";
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    try { rg.kill(); } catch {}
  }, timeoutMs);

  rg.stdout.on("data", (d) => (stdout += d.toString()));
  rg.stderr.on("data", (d) => (stderr += d.toString()));

  const exitCode: number = await new Promise((resolve) => {
    rg.on("close", (code) => resolve(code ?? 1));
    rg.on("error", () => resolve(127));
  });
  clearTimeout(timer);

  if (timedOut) return [];
  // rg exit 0 = matches, 1 = no matches, 2 = error
  if (exitCode === 1 && !stdout) return [];
  if (exitCode !== 0 && exitCode !== 1) {
    // rg not found -> fallback to error empty (do not throw)
    if (exitCode === 127 || stderr.includes("not found")) return [];
  }

  const lines = stdout.split("\n").filter(Boolean);
  const evidence: Evidence[] = [];
  for (const rawLine of lines.slice(0, limit)) {
    const line = rawLine.replace(/\r$/, ""); // handle Windows CRLF from rg
    // format: path:line:column:text  (text may contain colons)
    const m = line.match(/^([^:]+):(\d+):(\d+):(.*)$/);
    if (!m) continue;
    const [, fileRaw, lineStr, , text] = m;
    const rel = normalizeFile(fileRaw, projectRoot);
    if (isIgnored(rel)) continue;
    const lineNum = Number(lineStr);
    evidence.push({
      source: "exact",
      file: rel,
      startLine: lineNum,
      endLine: lineNum,
      text: text.slice(0, 400),
      score: 1.0, // exact matches get high retrieval score
      relation: "reference",
      symbolKind: "reference",
      provenance: `rg:${literal ? "literal" : "regex"}`,
      metadata: { column: m[3], rawLine: line.slice(0, 500) },
    });
  }
  // Deduplicate exact same file+line
  const seen = new Set<string>();
  const deduped: Evidence[] = [];
  for (const e of evidence) {
    const k = `${e.file}:${e.startLine}:${e.text?.slice(0, 100)}`;
    if (seen.has(k)) continue;
    seen.add(k);
    deduped.push(e);
  }
  return deduped;
}
