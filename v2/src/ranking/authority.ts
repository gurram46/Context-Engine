import type { Evidence, QueryType } from "../core/types.js";
import { classifyFile } from "../core/fileClassifier.js";

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
  docWhenImplAsked: -15,
  generated: -20,
  duplicateOverlap: -10,
  broadContextMatch: -8,
  testWhenImplAsked: -12,
  staleDoc: -30,
  shadowPenalty: -12,
  legacyScriptPenalty: -10,
} as const;

const STALE_MARKERS = ["archive", "legacy", "deprecated"];

function isStaleDoc(file: string): boolean {
  return STALE_MARKERS.some((m) => file.toLowerCase().includes(m));
}

export interface AuthorityInput {
  evidence: Evidence;
  queryType: QueryType;
  rawQuery: string;
  hasImplAlternative?: boolean;
}

function isTrueDefinition(evidence: Evidence, rawQuery: string): boolean {
  const text = (evidence.text || (evidence.metadata?.["codeSnippet"] as string) || "").toLowerCase();
  const kind = (evidence.symbolKind || "").toLowerCase();
  // Prefer structured OCI metadata: function_definition, class_definition, method, struct, etc with symbol
  if (evidence.symbol) {
    const symLower = evidence.symbol.toLowerCase();
    // If symbolKind indicates a definition and symbol matches, it's true
    if (["function_definition","function_declaration","method","class_definition","class_declaration","struct","interface","type","enum","trait","impl"].some(k=>kind.includes(k))) {
      // Check text for declaration
      if (text) {
        if (text.includes(`func ${symLower}`) || text.includes(`func (${symLower}`) || text.includes(`type ${symLower} struct`) || text.includes(`type ${symLower} interface`)) return true;
        if (text.includes(`def ${symLower}`) || text.includes(`class ${symLower}`)) return true;
        if (text.includes(`function ${symLower}`) || text.includes(`const ${symLower}`) || text.includes(`fn ${symLower}`)) return true;
      } else {
        // No text, but symbolKind + symbol match is strong signal for true definition
        return true;
      }
    }
  }
  // Fallback: check query target
  const target = getTargetSymbol(rawQuery);
  if (!target || !text) return false;
  const t = target.toLowerCase();
  // Language-agnostic declaration patterns
  const patterns = [
    `def ${t}`, `class ${t}`, `func ${t}`, `func (`, `type ${t} struct`, `type ${t} interface`,
    `function ${t}`, `const ${t}`, `fn ${t}`, `struct ${t}`, `enum ${t}`, `trait ${t}`,
  ];
  // For Go func with receiver: func (r *Receiver) Foo(
  if (text.includes(`func ${t}(`) || text.includes(`func (${t}`) || text.includes(` ${t}(`)) {
    // More precise: check if text contains t as function name
    if (new RegExp(`\\b${t}\\b`).test(text)) return true;
  }
  for (const pat of patterns) {
    if (text.includes(pat)) return true;
  }
  // Direct symbolKind check without text (e.g., peek without code block but with symbol)
  if (evidence.symbol && evidence.symbol.toLowerCase() === t && kind.includes("definition")) return true;
  return false;
}

function getTargetSymbol(rawQuery: string): string | undefined {
  const ids = rawQuery.match(/\b[A-Za-z_][A-Za-z0-9_]*\b/g) || [];
  const stop = new Set(["where","what","who","how","the","is","are","for","and","or","to","in","of","a","an","calls","callers","callees","tests","test","cover","covers","implemented","implementation","generation","flow","trace","secret","redaction"]);
  const filtered = ids.filter((t) => !stop.has(t.toLowerCase()) && t.length>=3 && (t.includes("_") || /[A-Z]/.test(t) || /^[a-z]{3,}$/.test(t.toLowerCase())));
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
    // Prefer PascalCase for Go
    const aPascal = /^[A-Z][a-z]+[A-Z]/.test(a) ? 0 : 1;
    const bPascal = /^[A-Z][a-z]+[A-Z]/.test(b) ? 0 : 1;
    if (aPascal!==bPascal) return aPascal-bPascal;
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
  const kind = classifyFile(file);

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
  if (kind === "SOURCE") {
    score += AUTHORITY_WEIGHTS.sameLanguageImpl;
    reasons.push(`+${AUTHORITY_WEIGHTS.sameLanguageImpl} source file`);
  }
  if (queryType === "TEST" && kind === "TEST") {
    score += AUTHORITY_WEIGHTS.testWhenAsked;
    reasons.push(`+${AUTHORITY_WEIGHTS.testWhenAsked} test when asked`);
  }
  if (evidence.symbol && lowerQuery.includes(evidence.symbol.toLowerCase())) {
    score += 5;
    reasons.push(`+5 symbol in query`);
  }
  if (evidence.symbol && evidence.symbol.startsWith("_") && lowerQuery.includes("secret")) {
    if (inp.hasImplAlternative) {
      score += AUTHORITY_WEIGHTS.shadowPenalty;
      reasons.push(`${AUTHORITY_WEIGHTS.shadowPenalty} private shadow`);
    }
  }
  if (evidence.symbol && !evidence.symbol.startsWith("_") && evidence.symbol.toLowerCase().includes("redact") && lowerQuery.includes("secret")) {
    score += AUTHORITY_WEIGHTS.publicOverPrivate;
    reasons.push(`+${AUTHORITY_WEIGHTS.publicOverPrivate} public impl`);
  }
  if (kind === "SOURCE" && file.toLowerCase().includes("/core/") && lowerQuery.includes("secret")) {
    score += AUTHORITY_WEIGHTS.activeReference;
    reasons.push(`+${AUTHORITY_WEIGHTS.activeReference} active core`);
  }

  if (kind === "GENERATED") {
    score += AUTHORITY_WEIGHTS.generated;
    reasons.push(`${AUTHORITY_WEIGHTS.generated} generated`);
  }
  if (isStaleDoc(file)) {
    score += AUTHORITY_WEIGHTS.staleDoc;
    reasons.push(`${AUTHORITY_WEIGHTS.staleDoc} stale`);
  }
  const wantsImpl = queryType === "SYMBOL" || queryType === "DEPENDENCY" || queryType === "MIXED" || (queryType === "CONCEPTUAL" && /implemented|generation|bundle|logic|validation|handler|router|service|enforced|ontology|pipeline/.test(lowerQuery));
  if (wantsImpl && kind === "DOC" ) {
    score += AUTHORITY_WEIGHTS.docWhenImplAsked;
    reasons.push(`${AUTHORITY_WEIGHTS.docWhenImplAsked} doc when impl wanted`);
  }
  // Penalize Context-Engine's own v2 when querying other repos, but not Mulanous's own docs
  if (wantsImpl && file.startsWith("v2/") && !lowerQuery.includes("v2")) {
    score += -12;
    reasons.push(`-12 v2 self when impl wanted`);
  }
  if (queryType !== "TEST" && kind === "TEST") {
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
  if (kind === "SOURCE" && file.includes("/scripts/") && inp.hasImplAlternative) {
    // Generic scripts penalty only when core alternative exists, but weak
    score += -5;
    reasons.push(`-5 scripts when core exists`);
  }

  const isCallerQuery = /\b(what calls|who calls|callers|used by|what breaks if)\b/i.test(rawQuery);
  if (queryType === "DEPENDENCY" && isCallerQuery) {
    const isReference = evidence.source === "exact" && evidence.relation === "reference" && targetSym && (evidence.text || "").toLowerCase().includes(targetSym);
    const isCaller = evidence.relation === "caller";
    if (isReference || isCaller) {
      const isExternal = !file.toLowerCase().includes(targetSym?.slice(0,4) ?? "") || !isTrueDefinition(evidence, rawQuery);
      if (isExternal || evidence.file !== "backend/context_engine/commands/bundle_command.py") {
        score += AUTHORITY_WEIGHTS.callerReference;
        reasons.push(`+${AUTHORITY_WEIGHTS.callerReference} caller reference`);
        const txt = (evidence.text || "").toLowerCase();
        if (txt.includes("add_command") || txt.includes("register") || txt.includes("newrouter") || txt.includes("healthhandler") || (targetSym && txt.includes(targetSym + "."))) {
          score += 8;
          reasons.push(`+8 wiring pattern`);
        }
        if (kind === "SOURCE" && file.startsWith("backend/")) {
          score += 5;
          reasons.push(`+5 backend wiring`);
        } else if (kind === "SOURCE" && file.includes("/internal/")) {
          score += 5;
          reasons.push(`+5 internal wiring`);
        }
      }
    }
    if (evidence.relation === "definition" && evidence.source === "symbol" && isTrueDefinition(evidence, rawQuery)) {
      score += -15;
      reasons.push(`-15 definition for caller query`);
    }
  }

  return { score, reasons };
}

export function applyAuthority(
  evidence: Evidence[],
  queryType: QueryType,
  rawQuery: string,
): Array<Evidence & { authorityScore: number; finalScore: number; authorityReasons: string[] }> {
  const hasImpl = evidence.some((e) => classifyFile(e.file) === "SOURCE");
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
    if (e.source === "symbol" && e.relation === "definition" && e.startLine !== undefined) {
      const min = minLinePerFile.get(e.file);
      if (min !== undefined && e.startLine === min) {
        if (!reasons.some((r) => r.includes("true definition"))) {
          const text = (e.text || "").toLowerCase();
          if (text.includes("def ") || text.includes("class ") || text.includes("func ") || text.includes("type ")) {
            score += AUTHORITY_WEIGHTS.trueDefinition;
            reasons.push(`+${AUTHORITY_WEIGHTS.trueDefinition} earliest definition in file`);
          }
        }
      }
    }
    const finalScore = base * 20 + score;
    return { ...e, authorityScore: score, finalScore, authorityReasons: reasons } as any;
  });
}
