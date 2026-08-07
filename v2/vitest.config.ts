import { defineConfig } from "vitest/config";
export default defineConfig({
  test: {
    include: ["v2/tests/**/*.test.ts"],
  },
});
