import { describe, it, expect } from "vitest";
import { fuseEvidence } from "../src/ranking/fuse.js";
import type { Evidence } from "../src/core/types.js";

function ev(over: Partial<Evidence>): Evidence {
  return {
    source: "symbol",
    file: "a.py",
    startLine: 1,
    endLine: 5,
    score: 0.9,
    relation: "unknown",
    ...over,
  } as Evidence;
}

describe("ranking_rules", () => {
  it("1. true definition beats overlapping interior slices", () => {
    const def = ev({
      source: "symbol",
      file: "backend/context_engine/commands/bundle_command.py",
      startLine: 20,
      endLine: 31,
      symbol: "foo",
      symbolKind: "decorated_definition",
      relation: "definition",
      score: 0.96,
      text: "@click.command()\ndef foo():\n  pass",
    });
    const interior = ev({
      source: "symbol",
      file: "backend/context_engine/commands/bundle_command.py",
      startLine: 74,
      endLine: 85,
      symbolKind: "decorated_definition",
      relation: "definition",
      score: 1.0, // even higher retrieval score, but interior
      text: "x = deduplicate_content(y)",
    });
    const interior2 = ev({
      source: "symbol",
      file: "backend/context_engine/commands/bundle_command.py",
      startLine: 110,
      endLine: 120,
      symbolKind: "decorated_definition",
      relation: "definition",
      score: 0.96,
      text: "if tokens > 1000: warn()",
    });
    const { ranked } = fuseEvidence([interior, interior2, def] as any, {
      queryType: "MIXED",
      rawQuery: "Trace the foo generation flow",
      topN: 10,
    });
    expect(ranked[0].file).toBe(def.file);
    expect(ranked[0].startLine).toBe(20);
  });

  it("2. caller/reference beats definition for callers query", () => {
    const def = ev({
      source: "symbol",
      file: "backend/context_engine/commands/bundle_command.py",
      startLine: 20,
      symbol: "foo",
      symbolKind: "decorated_definition",
      relation: "definition",
      score: 0.96,
      text: "def foo():",
    });
    const caller = ev({
      source: "exact",
      file: "backend/context_engine/cli.py",
      startLine: 45,
      score: 1.0,
      relation: "reference",
      text: "cli.add_command(foo)",
    });
    const { ranked } = fuseEvidence([def, caller] as any, {
      queryType: "DEPENDENCY",
      rawQuery: "What calls foo?",
      topN: 10,
    });
    expect(ranked[0].file).toBe("backend/context_engine/cli.py");
  });

  it("3. definition remains first for implementation query", () => {
    const def = ev({
      source: "symbol",
      file: "backend/context_engine/core/utils.py",
      startLine: 54,
      symbol: "foo",
      symbolKind: "function_definition",
      relation: "definition",
      score: 0.96,
      text: "def foo():",
    });
    const ref = ev({
      source: "exact",
      file: "backend/context_engine/cli.py",
      startLine: 45,
      score: 1.0,
      relation: "reference",
      text: "cli.add_command(foo)",
    });
    const { ranked } = fuseEvidence([ref, def] as any, {
      queryType: "SYMBOL",
      rawQuery: "Where is foo implemented?",
      topN: 10,
    });
    expect(ranked[0].relation).toBe("definition");
    expect(ranked[0].file).toContain("utils.py");
  });

  it("4. active referenced public beats private shadow", () => {
    const pub = ev({
      source: "symbol",
      file: "backend/context_engine/core/utils.py",
      startLine: 77,
      symbol: "redact_secrets",
      symbolKind: "function_definition",
      relation: "definition",
      score: 0.96,
      text: "def redact_secrets(text):",
    });
    const shadow = ev({
      source: "symbol",
      file: "backend/context_engine/scripts/embedder.py",
      startLine: 10,
      symbol: "_redact_secrets",
      symbolKind: "function_definition",
      relation: "definition",
      score: 0.96,
      text: "def _redact_secrets(text):",
    });
    const { ranked } = fuseEvidence([shadow, pub] as any, {
      queryType: "CONCEPTUAL",
      rawQuery: "Where is secret redaction implemented?",
      topN: 10,
    });
    expect(ranked[0].file).toBe("backend/context_engine/core/utils.py");
    expect(ranked[0].symbol).toBe("redact_secrets");
  });
});
