import type { ClassifiedQuery, QueryType } from "../core/types.js";

const DEPENDENCY_RE = /\b(who calls|what calls|callers|callees|depends on|what breaks if|impact of|used by|transitive|calls?)\b/i;
const TEST_RE = /\b(tests? for|where is .* tested|what tests cover|test coverage|specs? for)\b/i;
const SYMBOL_DEF_RE = /\b(where is|where's|defined|implementation|implemented|define)\b/i;

function isQuotedLiteral(q: string): boolean {
  return /^["'].*["']$/.test(q.trim());
}
function hasPathLike(q: string): boolean {
  return /[\/\\].+\.\w+/.test(q) || /\b\w+\/\w+/.test(q);
}
function hasFilename(q: string): boolean {
  return /\b[\w.-]+\.(py|ts|tsx|js|jsx|md|json|yml|yaml|toml|ini|env)\b/.test(q);
}
function isSnakeCase(q: string): boolean {
  return /\b[a-z]+_[a-z_0-9]+\b/.test(q);
}
function isCamelCase(q: string): boolean {
  return /\b[a-z]+[A-Z][a-zA-Z0-9]*\b/.test(q);
}
function isPascalCase(q: string): boolean {
  return /\b[A-Z][a-z]+[A-Z][a-zA-Z0-9]*\b/.test(q);
}
function looksLikeIdentifier(q: string): boolean {
  const t = q.trim();
  // single token snake/camel/pascal or with parentheses
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(t)) return isSnakeCase(t) || isCamelCase(t) || isPascalCase(t) || t.length > 2;
  // "symbol X" or "function X"
  const m = t.match(/\b(?:function|class|method|symbol)\s+([A-Za-z_][A-Za-z0-9_]*)\b/i);
  if (m) return true;
  return false;
}
function hasExactSignals(q: string): boolean {
  return (
    isQuotedLiteral(q) ||
    hasPathLike(q) ||
    hasFilename(q) ||
    /\b[A-Z_]+\b/.test(q) && /_/.test(q) // ENV_VAR
  );
}

export function classifyQuery(raw: string): ClassifiedQuery {
  const normalized = raw.trim().replace(/\s+/g, " ");
  const lower = normalized.toLowerCase();
  const hints: string[] = [];

  // Priority: TEST / DEPENDENCY often contain natural language but are specific intents
  if (TEST_RE.test(lower)) {
    hints.push("test");
    return { type: "TEST", raw, normalized, hints };
  }
  if (DEPENDENCY_RE.test(lower)) {
    hints.push("dependency");
    return { type: "DEPENDENCY", raw, normalized, hints };
  }

  // EXACT: quoted, path, filename, env var literal — check path/filename BEFORE snake check
  if (isQuotedLiteral(normalized)) {
    hints.push("exact-quoted");
    return { type: "EXACT", raw, normalized, hints };
  }
  if (hasPathLike(normalized) || hasFilename(normalized)) {
    hints.push("exact");
    if (normalized.split(/\s+/).length <= 3) return { type: "EXACT", raw, normalized, hints };
    hints.push("path-in-conceptual");
    return { type: "MIXED", raw, normalized, hints };
  }
  if (hasExactSignals(normalized)) {
    hints.push("exact");
    if (isSnakeCase(normalized) || isCamelCase(normalized) || isPascalCase(normalized)) {
      if (normalized.split(/\s+/).length === 1 && /^[A-Za-z_][A-Za-z0-9_]*$/.test(normalized)) {
        // single identifier -> fall through to SYMBOL
      } else {
        hints.push("mixed-exact-symbol");
        return { type: "MIXED", raw, normalized, hints };
      }
    }
  }

  // SYMBOL: bare identifier or "where is X defined"
  if (looksLikeIdentifier(normalized) || (SYMBOL_DEF_RE.test(lower) && (isSnakeCase(normalized) || isCamelCase(normalized) || isPascalCase(normalized)))) {
    // single token identifier
    const tokens = normalized.split(/\s+/);
    const hasId = tokens.some((t) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(t) && (isSnakeCase(t) || isCamelCase(t) || isPascalCase(t)));
    if (hasId) {
      // If query is short (<=6 words) and contains identifier + definition intent -> SYMBOL
      if (tokens.length <= 6) {
        hints.push("symbol");
        return { type: "SYMBOL", raw, normalized, hints };
      }
      hints.push("symbol-in-long-query");
      return { type: "MIXED", raw, normalized, hints };
    }
  }
  // "where is count_tokens implemented?" -> SYMBOL
  if (SYMBOL_DEF_RE.test(lower)) {
    const ids = normalized.match(/\b[A-Za-z_][A-Za-z0-9_]*\b/g) || [];
    const sym = ids.find((t) => isSnakeCase(t) || isCamelCase(t) || isPascalCase(t));
    if (sym) {
      hints.push("symbol-definition");
      return { type: "SYMBOL", raw, normalized, hints };
    }
  }

  // CONCEPTUAL: long natural language responsibility question
  const isQuestion = lower.startsWith("where") || lower.startsWith("what") || lower.startsWith("how") || normalized.includes("?");
  const wordCount = normalized.split(/\s+/).length;
  if (wordCount >= 4 && (isQuestion || /enforced|responsible|logic|handle|prevent|validate/.test(lower))) {
    hints.push("conceptual");
    return { type: "CONCEPTUAL", raw, normalized, hints };
  }

  // Default heuristics:
  // - very short single token with underscores/camel -> SYMBOL
  if (normalized.split(/\s+/).length === 1 && /^[A-Za-z_][A-Za-z0-9_]*$/.test(normalized)) {
    return { type: "SYMBOL", raw, normalized, hints: ["single-identifier"] };
  }
  // - contains conceptual keywords but also identifier -> MIXED
  if (wordCount > 6 && (isSnakeCase(normalized) || isCamelCase(normalized))) {
    return { type: "MIXED", raw, normalized, hints: ["mixed-concept-symbol"] };
  }
  if (wordCount >= 6) return { type: "CONCEPTUAL", raw, normalized, hints: ["long-natural"] };
  if (wordCount >= 1) return { type: "MIXED", raw, normalized, hints: ["default-mixed"] };

  return { type: "CONCEPTUAL", raw, normalized, hints };
}
