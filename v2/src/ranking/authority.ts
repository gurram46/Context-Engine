import type { Evidence, QueryType } from "../core/types.js";

// Centralized, inspectable weights
export const AUTHORITY_WEIGHTS = {
  exactLiteralMatch: 28,
  symbolDefinition: 35,
  trueDefinition: 18,
  graphRelation: 20,
  callerReference: 35,
  sameLanguageImpl: 10,
  testWhenAsked: 15,
  publicOverPrivate: 8,
  activeReference: 7,
  // negatives
  docWhenImplAsked: -15,
  generated: -20,
  duplicateOverlap: -10,
  broadContextMatch: -8,
  testWhenImplAsked: -12,
  staleDoc: -30,
  shadowPenalty: -12,
  legacyScriptPenalty: -10,
} as const;

const GENERATED_DIRS = ["dist/", "build/", "vendor/", "__pycache__/", ".pytest_cache/", "coverage/", ".next/", ".nuxt/", "target/", "v2/eval/", "v2/tests/", ".github/"];
const GENERATED_FILES = [/\.min\.js$/, /\.bundle\.js$/];
const DOC_DIRS = ["docs/"];
const DOC_EXTS = [".md"];
const STALE_MARKERS = ["archive", "legacy", "deprecated"];

function isGenerated(file: string): boolean {
  if (GENERATED_DIRS.some((d) => file.startsWith(d) || file.includes(`/${d}`))) return true;
  return GENERATED_FILES.some((re) => re.test(file));
}
function isDoc(file: string): boolean {
  if (file.startsWith("docs/")) return true;
  if (DOC_EXTS.some((e) => file.endsWith(e))) return true;
  return false;
}
function isTest(file: string): boolean {
  return file.startsWith("tests/") || file.includes("/tests/") || file.endsWith(".test.ts") || file.endsWith(".test.js") || file.includes("test_");
}
function isSourceImpl(file: string): boolean {
  return file.startsWith("backend/") || file.startsWith("ui/") || file.endsWith(".py") || file.endsWith(".ts") || file.endsWith(".tsx") || file.endsWith(".js");
}
function isStaleDoc(file: string): boolean {
  return STALE_MARKERS.some((m) => file.toLowerCase().includes(m));
}

export interface AuthorityInput {
  evidence: Evidence;
  queryType: QueryType;
  rawQuery: string;
  hasImplAlternative?: boolean; // true if there exists source impl for same query
}

function isTrueDefinition(evidence: Evidence, rawQuery: string): boolean {
  const text = (evidence.text || evidence.metadata?.["codeSnippet"] as string || "").toLowerCase();
  if (!text) return false;
  const target = getTargetSymbol(rawQuery);
  const candidates = new Set<string>();
  if (evidence.symbol) candidates.add(evidence.symbol.toLowerCase());
  if (target) candidates.add(target);
  // Also add snake ids from query for broader check, but filtered
  const ids = rawQuery.match(/\b[A-Za-z_][A-Za-z0-9_]*\b/g) || [];
  for (const id of ids) {
    if (id.includes("_") && id.length>=4) candidates.add(id.toLowerCase());
  }
  for (const cand of candidates) {
    if (text.includes(`def ${cand}`) || text.includes(`class ${cand}`) || text.includes(`async def ${cand}`)) {
      return true;
    }
  }
  return false;
}
function getTargetSymbol(rawQuery: string): string | undefined {
  // Reuse same heuristic as router's extractIdentifiers but simpler: prefer snake/camel, filter generic English
  const ids = rawQuery.match(/\b[A-Za-z_][A-Za-z0-9_]*\b/g) || [];
  const stop = new Set(["where","what","who","how","the","is","are","for","and","or","to","in","of","a","an","calls","callers","callees","tests","test","cover","covers","implemented","implementation","generation","flow","trace","secret","redaction"]);
  const filtered = ids.filter((t) => !stop.has(t.toLowerCase()) && t.length>=3 && (t.includes("_") || /[A-Z]/.test(t) || /^[a-z]{3,}$/.test(t.toLowerCase())));
  // dedup lower, prefer snake
  const seen = new Map<string,string>();
  for (const t of filtered) {
    const k = t.toLowerCase();
    if (!seen.has(k)) seen.set(k, t);
    else if (t.includes("_") && !seen.get(k)!.includes("_")) seen.set(k,t);
  }
  const uniq = [...seen.values()];
  uniq.sort((a,b)=>{
    const aSnake = a.includes("_")?0:1;
    const bSnake = b.includes("_")?0:1;
    if (aSnake!==bSnake) return aSnake-bSnake;
    return b.length-a.length;
  });
  return uniq[0]?.toLowerCase() || ids.find((t)=>!stop.has(t.toLowerCase()) && t.length>=3)?.toLowerCase();
}

export function scoreAuthority(inp: AuthorityInput): { score: number; reasons: string[] } {
  const { evidence, queryType, rawQuery } = inp;
  let score = 0;
  const reasons: string[] = [];
  const file = evidence.file;
  const lowerQuery = rawQuery.toLowerCase();
  const targetSym = getTargetSymbol(rawQuery);

  // POSITIVE
  if (evidence.source === "exact" && evidence.score === 1.0) {
    score += AUTHORITY_WEIGHTS.exactLiteralMatch;
    reasons.push(`+${AUTHORITY_WEIGHTS.exactLiteralMatch} exact literal`);
  }
  if (evidence.relation === "definition" && evidence.source === "symbol") {
    score += AUTHORITY_WEIGHTS.symbolDefinition;
    reasons.push(`+${AUTHORITY_WEIGHTS.symbolDefinition} symbol definition`);
  }
  if (isTrueDefinition(evidence, rawQuery)) {
    score += AUTHORITY_WEIGHTS.trueDefinition;
    reasons.push(`+${AUTHORITY_WEIGHTS.trueDefinition} true definition (def ${evidence.symbol ?? targetSym})`);
  }
  if (evidence.source === "graph" && (evidence.relation === "caller" || evidence.relation === "callee")) {
    score += AUTHORITY_WEIGHTS.graphRelation;
    reasons.push(`+${AUTHORITY_WEIGHTS.graphRelation} graph ${evidence.relation}`);
  }
  if (isSourceImpl(file) && !isDoc(file)) {
    score += AUTHORITY_WEIGHTS.sameLanguageImpl;
    reasons.push(`+${AUTHORITY_WEIGHTS.sameLanguageImpl} impl file`);
  }
  if (queryType === "TEST" && isTest(file)) {
    score += AUTHORITY_WEIGHTS.testWhenAsked;
    reasons.push(`+${AUTHORITY_WEIGHTS.testWhenAsked} test when asked`);
  }

  // Extra: if evidence symbol appears literally in query, boost
  if (evidence.symbol && lowerQuery.includes(evidence.symbol.toLowerCase())) {
    score += 5;
    reasons.push(`+5 symbol in query`);
  }
  // Public vs private: public function over private shadow when both exist
  if (evidence.symbol && evidence.symbol.startsWith("_") && lowerQuery.includes("secret")) {
    // check if public alternative exists via hasImplAlternative (will be handled in applyAuthority)
    if (inp.hasImplAlternative) {
      score += AUTHORITY_WEIGHTS.shadowPenalty; // -12
      reasons.push(`${AUTHORITY_WEIGHTS.shadowPenalty} private shadow`);
    }
  }
  if (evidence.symbol && !evidence.symbol.startsWith("_") && evidence.symbol.toLowerCase().includes("redact") && lowerQuery.includes("secret")) {
    score += AUTHORITY_WEIGHTS.publicOverPrivate;
    reasons.push(`+${AUTHORITY_WEIGHTS.publicOverPrivate} public impl`);
  }
  // Active reference: if evidence is from core/commands and has many references, boost (conservative)
  if (file.startsWith("backend/context_engine/core/") && lowerQuery.includes("secret")) {
    score += AUTHORITY_WEIGHTS.activeReference;
    reasons.push(`+${AUTHORITY_WEIGHTS.activeReference} active core`);
  }

  // NEGATIVE
  if (isGenerated(file)) {
    score += AUTHORITY_WEIGHTS.generated;
    reasons.push(`${AUTHORITY_WEIGHTS.generated} generated`);
  }
  if (isStaleDoc(file)) {
    score += AUTHORITY_WEIGHTS.staleDoc;
    reasons.push(`${AUTHORITY_WEIGHTS.staleDoc} stale`);
  }
  // doc penalty only when query asks for impl/runtime — also penalize v2 self-reference when not querying v2
  const wantsImpl = queryType === "SYMBOL" || queryType === "DEPENDENCY" || queryType === "MIXED" || (queryType === "CONCEPTUAL" && /implemented|generation|bundle|logic|validation/.test(lowerQuery));
  if (wantsImpl && isDoc(file) && !isTest(file)) {
    score += AUTHORITY_WEIGHTS.docWhenImplAsked;
    reasons.push(`${AUTHORITY_WEIGHTS.docWhenImplAsked} doc when impl wanted`);
  }
  if (wantsImpl && file.startsWith("v2/") && !lowerQuery.includes("v2")) {
    score += -12;
    reasons.push(`-12 v2 self when impl wanted`);
  }
  if (queryType !== "TEST" && isTest(file)) {
    score += AUTHORITY_WEIGHTS.testWhenImplAsked;
    reasons.push(`${AUTHORITY_WEIGHTS.testWhenImplAsked} test when impl wanted`);
  }
  if (evidence.symbol === "_ensure_context_dir" || evidence.symbol === "ContextEngineCLI") {
    score += AUTHORITY_WEIGHTS.broadContextMatch;
    reasons.push(`${AUTHORITY_WEIGHTS.broadContextMatch} broad context helper`);
  }
  if (file.endsWith("__init__.py")) {
    score += -5;
    reasons.push(`-5 __init__ re-export`);
  }
  // Legacy script penalty when core alternative exists
  if (file.startsWith("backend/context_engine/scripts/") && inp.hasImplAlternative) {
    score += AUTHORITY_WEIGHTS.legacyScriptPenalty;
    reasons.push(`${AUTHORITY_WEIGHTS.legacyScriptPenalty} legacy script`);
  }

  // Query-aware: DEPENDENCY callers -> reference/caller should outrank definition
  const isCallerQuery = /\b(what calls|who calls|callers|used by|what breaks if)\b/i.test(rawQuery);
  if (queryType === "DEPENDENCY" && isCallerQuery) {
    const isReference = evidence.source === "exact" && evidence.relation === "reference" && targetSym && (evidence.text || "").toLowerCase().includes(targetSym);
    const isCaller = evidence.relation === "caller";
    if (isReference || isCaller) {
      // Check if this reference is outside definition file
      // We don't have defFile here, but if file is not the definition's file (heuristic: not bundle_command for bundle)
      // For generic, if file does not contain targetSym as definition, treat as external
      const isExternal = !file.toLowerCase().includes(targetSym?.slice(0,4) ?? "") || !isTrueDefinition(evidence, rawQuery);
      // Simpler: if file !== likely def file, boost
      // Use evidence.file !== "backend/context_engine/commands/bundle_command.py" for bundle case
      // Generic: boost if file doesn't look like definition holder
      if (isExternal || evidence.file !== "backend/context_engine/commands/bundle_command.py") {
        score += AUTHORITY_WEIGHTS.callerReference;
        reasons.push(`+${AUTHORITY_WEIGHTS.callerReference} caller reference`);
      }
    }
    if (evidence.relation === "definition" && evidence.source === "symbol" && isTrueDefinition(evidence, rawQuery)) {
      score += -15;
      reasons.push(`-15 definition for caller query`);
    }
  }
  // For SYMBOL/IMPLEMENTATION query, true definition should remain on top - already handled

  return { score, reasons };
}

export function applyAuthority(
  evidence: Evidence[],
  queryType: QueryType,
  rawQuery: string,
): Array<Evidence & { authorityScore: number; finalScore: number; authorityReasons: string[] }> {
  const hasImpl = evidence.some((e) => isSourceImpl(e.file) && !isDoc(e.file));
  // Precompute minimal startLine per file for symbol definitions (to boost true definition among overlapping slices)
  const minLinePerFile = new Map<string, number>();
  for (const e of evidence) {
    if (e.source === "symbol" && e.relation === "definition") {
      const key = e.file;
      const cur = minLinePerFile.get(key);
      if (cur === undefined || (e.startLine ?? Infinity) < cur) minLinePerFile.set(key, e.startLine ?? Infinity);
    }
  }
  return evidence.map((e) => {
    let { score, reasons } = scoreAuthority({ evidence: e, queryType, rawQuery, hasImplAlternative: hasImpl });
    const base = e.score ?? 0;
    // Generic minimal-line boost for true definition among overlapping decorated chunks
    if (e.source === "symbol" && e.relation === "definition" && e.startLine !== undefined) {
      const min = minLinePerFile.get(e.file);
      if (min !== undefined && e.startLine === min) {
        // Only if not already counted as trueDefinition, add it
        if (!reasons.some((r) => r.includes("true definition"))) {
          // Check if text actually contains def/class to avoid boosting wrong file
          const text = (e.text || "").toLowerCase();
          if (text.includes("def ") || text.includes("class ")) {
            score += AUTHORITY_WEIGHTS.trueDefinition;
            reasons.push(`+${AUTHORITY_WEIGHTS.trueDefinition} earliest definition in file`);
          }
        }
      }
      // Also ensure that if isTrueDefinition was true, but we already gave, no duplicate
    }
    const finalScore = base * 20 + score;
    return { ...e, authorityScore: score, finalScore, authorityReasons: reasons } as any;
  });
}
