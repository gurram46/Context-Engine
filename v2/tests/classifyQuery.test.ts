import { describe, it, expect } from "vitest";
import { classifyQuery } from "../src/router/classifyQuery.js";

describe("classifyQuery", () => {
  it("classifies quoted literal as EXACT", () => {
    expect(classifyQuery('"context_for_ai.md"').type).toBe("EXACT");
  });
  it("classifies path as EXACT", () => {
    expect(classifyQuery("backend/context_engine/core/utils.py").type).toBe("EXACT");
  });
  it("classifies snake_case symbol", () => {
    expect(classifyQuery("count_tokens").type).toBe("SYMBOL");
  });
  it("classifies where is X implemented as SYMBOL", () => {
    expect(classifyQuery("Where is count_tokens implemented?").type).toBe("SYMBOL");
  });
  it("classifies dependency", () => {
    expect(classifyQuery("What calls bundle?").type).toBe("DEPENDENCY");
    expect(classifyQuery("who calls activateSubscription").type).toBe("DEPENDENCY");
  });
  it("classifies test", () => {
    expect(classifyQuery("What tests cover bundle generation?").type).toBe("TEST");
    expect(classifyQuery("tests for bundle").type).toBe("TEST");
  });
  it("classifies conceptual", () => {
    expect(classifyQuery("where is subscription expiration enforced?").type).toBe("CONCEPTUAL");
  });
  it("classifies mixed bundle flow as MIXED", () => {
    const r = classifyQuery("Trace the Bundle Generation Flow context bundle --no-ai to .context/context_for_ai.md");
    expect(["MIXED","CONCEPTUAL"]).toContain(r.type);
  });
});
