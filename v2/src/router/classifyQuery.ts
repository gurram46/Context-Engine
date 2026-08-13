import type { ClassifiedQuery, QueryType } from "../core/types.js";
import { classifyFile } from "../core/fileClassifier.js";
import path from "node:path";

const DEPENDENCY_RE = /\b(who calls|what calls|callers|callees|depends on|what breaks if|impact of|used by|transitive|calls?)\b/i;
const TEST_RE = /\b(tests? for|where is .* tested|what tests cover|test coverage|specs? for)\b/i;
const SYMBOL_DEF_RE = /\b(where is|where's|defined|implementation|implemented|define)\b/i;

function isQuotedLiteral(q: string): boolean {
  return /^["'].*["']$/.test(q.trim());
}
function hasPathLike(q: string): boolean {
  // Any token with slash and extension, or path with at least two segments
  return /[\/\\].+\.\w+/.test(q) || /\b\w+\/\w+/.test(q);
}
function hasFilename(q: string): boolean {
  const tokens = q.trim().split(/\s+/);
  for (const tok of tokens) {
    let clean = tok.replace(/^["']|["']$/g, "").replace(/^[?.!,;:()]+|[?.!,;:()]+$/g, "");
    if (!clean) continue;
    if (/^(Dockerfile|Makefile|Procfile|Justfile|Brewfile|Gemfile|Rakefile|go\.mod|go\.sum)$/i.test(clean)) return true;
    const ext = path.extname(clean).toLowerCase();
    if (!ext) continue;
    const kind = classifyFile(clean);
    if (kind === "SOURCE" || kind === "CONFIG" || kind === "BUILD" || kind === "DOC") return true;
    if (clean.includes(".") && clean.length < 40 && !clean.includes(" ")) {
      if (/\.[a-z0-9]{1,5}$/i.test(clean)) return true;
    }
  }
  return false;
}
function isSnakeCase(q: string): boolean {
  return /\b[a-z]+_[a-z_0-9]*\b/.test(q) || /\b[A-Z]+_[A-Z_0-9]*\b/.test(q); // also SCREAMING
}
function isCamelCase(q: string): boolean {
  return /\b[a-z]+[A-Z][a-zA-Z0-9]*\b/.test(q);
}
function isPascalCase(q: string): boolean {
  return /\b[A-Z][a-z]+[A-Z][a-zA-Z0-9]*\b/.test(q) || /\b[A-Z][a-z]{2,}[A-Z][a-zA-Z]*\b/.test(q) || /^[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]+)+$/.test(q) || /^\b[A-Z][a-z]*[A-Z][a-zA-Z0-9]*\b/.test(q);
}
function isScreamingSnake(q: string): boolean {
  return /\b[A-Z]+_[A-Z0-9_]+\b/.test(q);
}
function hasQualifiedSymbol(q: string): boolean {
  // package.symbol, Receiver.Method, namespace::symbol, crate::module
  return /[a-zA-Z_][\w]*[.:]{1,2}[a-zA-Z_][\w]*/.test(q) || /::/.test(q);
}
function looksLikeIdentifier(q: string): boolean {
  const t = q.trim();
  if (/^[A-Za-z_][A-Za-z0-9_:]*(\.[A-Za-z_][A-Za-z0-9_]*)*$/.test(t) && (isSnakeCase(t) || isCamelCase(t) || isPascalCase(t) || isScreamingSnake(t) || hasQualifiedSymbol(t) || t.length > 2)) return true;
  const m = t.match(/\b(?:function|class|method|symbol|struct|interface|type|func)\s+([A-Za-z_][A-Za-z0-9_:]*)\b/i);
  if (m) return true;
  return false;
}
function isIdentifierToken(t: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(t) && (isSnakeCase(t) || isCamelCase(t) || isPascalCase(t) || isScreamingSnake(t));
}
function hasIdentifier(q: string): boolean {
  const tokens = q.match(/\b[A-Za-z_][A-Za-z0-9_:]*\b/g) || [];
  return tokens.some((tok) => {
    const base = tok.split(/[.:]/).pop() || tok;
    return isIdentifierToken(base) || hasQualifiedSymbol(tok);
  });
}

export function classifyQuery(raw: string): ClassifiedQuery {
  const normalized = raw.trim().replace(/\s+/g, " ");
  const lower = normalized.toLowerCase();
  const hints: string[] = [];

  if (TEST_RE.test(lower)) {
    hints.push("test");
    return { type: "TEST", raw, normalized, hints };
  }
  if (DEPENDENCY_RE.test(lower)) {
    hints.push("dependency");
    return { type: "DEPENDENCY", raw, normalized, hints };
  }

  if (isQuotedLiteral(normalized)) {
    hints.push("exact-quoted");
    return { type: "EXACT", raw, normalized, hints };
  }
  // Single token filename like go.mod, Dockerfile, package.json
  const single = normalized.split(/\s+/).length === 1;
  if (single && hasFilename(normalized)) {
    hints.push("exact-filename");
    return { type: "EXACT", raw, normalized, hints };
  }
  if (hasPathLike(normalized) || hasFilename(normalized)) {
    hints.push("exact");
    if (normalized.split(/\s+/).length <= 3) return { type: "EXACT", raw, normalized, hints };
    hints.push("path-in-conceptual");
    return { type: "MIXED", raw, normalized, hints };
  }

  if (looksLikeIdentifier(normalized) || (SYMBOL_DEF_RE.test(lower) && hasIdentifier(normalized))) {
    const tokens = normalized.split(/\s+/);
    const hasId = tokens.some((t) => {
      const base = t.split(/[.:]/).pop() || t;
      return isIdentifierToken(base) || hasQualifiedSymbol(t);
    });
    if (hasId) {
      if (tokens.length <= 6) {
        hints.push("symbol");
        return { type: "SYMBOL", raw, normalized, hints };
      }
      hints.push("symbol-in-long-query");
      return { type: "MIXED", raw, normalized, hints };
    }
  }
  if (SYMBOL_DEF_RE.test(lower)) {
    const ids = normalized.match(/\b[A-Za-z_][A-Za-z0-9_]*\b/g) || [];
    const sym = ids.find((t) => isIdentifierToken(t) || hasQualifiedSymbol(t));
    if (sym) {
      hints.push("symbol-definition");
      return { type: "SYMBOL", raw, normalized, hints };
    }
  }

  const isQuestion = lower.startsWith("where") || lower.startsWith("what") || lower.startsWith("how") || normalized.includes("?");
  const wordCount = normalized.split(/\s+/).length;
  if (wordCount >= 4 && (isQuestion || /enforced|responsible|logic|handle|prevent|validate|implemented|ontology|pipeline|wired/.test(lower))) {
    hints.push("conceptual");
    return { type: "CONCEPTUAL", raw, normalized, hints };
  }

  if (single && /^[A-Za-z_][A-Za-z0-9_:]*$/.test(normalized) && hasIdentifier(normalized)) {
    return { type: "SYMBOL", raw, normalized, hints: ["single-identifier"] };
  }
  if (wordCount > 6 && hasIdentifier(normalized)) {
    return { type: "MIXED", raw, normalized, hints: ["mixed-concept-symbol"] };
  }
  if (wordCount >= 6) return { type: "CONCEPTUAL", raw, normalized, hints: ["long-natural"] };
  if (wordCount >= 1) return { type: "MIXED", raw, normalized, hints: ["default-mixed"] };

  return { type: "CONCEPTUAL", raw, normalized, hints };
}
