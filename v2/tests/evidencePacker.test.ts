import { describe, it, expect } from "vitest";
import { packEvidence } from "../src/packing/evidencePacker.js";

describe("evidencePacker", () => {
  it("respects budget", () => {
    const ranked: any[] = Array.from({length:5}, (_,i)=>({ file:`a${i}.py`, startLine:1+i*10, endLine:5+i*10, symbol:"foo", score:0.9, authorityScore:10, finalScore:28, source:"symbol", provenance:"test"}));
    const pack = packEvidence(ranked as any, "foo", "SYMBOL", { budget: 100 });
    expect(pack.tokenEstimate).toBeLessThanOrEqual(150);
  });
});
