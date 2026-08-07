import { describe, it, expect } from "vitest";
import { exactSearch } from "../src/retrieval/exactSearch.js";

describe("exactSearch", () => {
  it("finds bundle literal", async () => {
    const ev = await exactSearch("bundle", { literal: true, limit: 5 });
    expect(ev.length).toBeGreaterThan(0);
    expect(ev[0].file).toBeDefined();
  });
  it("finds count_tokens", async () => {
    const ev = await exactSearch("count_tokens", { literal: true, limit: 20 });
    expect(ev.some((e) => e.file.includes("utils.py"))).toBe(true);
  });
  it("returns empty for nonsense", async () => {
    // build token dynamically so test file doesn't contain literal
    const token = ["__THIS","TOKEN","DOES","NOT","EXIST","987654321"].join("_");
    const ev = await exactSearch(token, { literal: true, limit: 5 });
    expect(ev.length).toBe(0);
  });
});
