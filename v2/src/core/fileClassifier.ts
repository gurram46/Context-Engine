import path from "node:path";

export type FileKind = "SOURCE" | "TEST" | "DOC" | "CONFIG" | "BUILD" | "GENERATED" | "UNKNOWN";

const SOURCE_EXTS = new Set([
  ".py", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs",
  ".go", ".rs", ".java", ".kt", ".c", ".cpp", ".cc", ".cxx", ".h", ".hpp", ".hh", ".hxx",
  ".cs", ".rb", ".php", ".swift", ".sql", ".sh", ".ps1", ".zig", ".dart", ".scala", ".clj",
]);

const CONFIG_EXTS = new Set([
  ".json", ".yaml", ".yml", ".toml", ".ini", ".env", ".xml", ".proto", ".conf", ".cfg", ".properties",
]);

const BUILD_FILES = new Set([
  "dockerfile", "makefile", "procfile", "justfile", "brewfile", "gemfile", "rakefile",
]);

const SPECIAL_SOURCELESS = new Set([
  "go.mod", "go.sum", "cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml",
]);

const DOC_EXTS = new Set([".md", ".mdx", ".rst", ".adoc", ".txt"]);

const GENERATED_DIRS = ["dist", "build", "target", "out", "bin", "obj", "vendor", "__pycache__", ".pytest_cache", "coverage", ".next", ".nuxt", "node_modules", ".opencode", ".git", "tmp", "temp"];
const GENERATED_FILES = [/\.min\.js$/, /\.bundle\.js$/, /\.min\.css$/, /\.pyc$/];

export function classifyFile(filePath: string): FileKind {
  const lower = filePath.toLowerCase();
  const base = path.basename(lower);
  const ext = path.extname(lower);
  const dir = path.dirname(lower);

  // GENERATED first (highest priority for filtering)
  if (GENERATED_DIRS.some((d) => lower === d || lower.startsWith(d + "/") || lower.includes(`/${d}/`) || dir.split("/").includes(d))) {
    // But don't classify test files as generated just because they are under tests
    // Check if it's actually a generated dir, not just name collision
    for (const g of GENERATED_DIRS) {
      if (lower.startsWith(g + "/") || lower.includes(`/${g}/`)) return "GENERATED";
      if (base === g) return "GENERATED";
    }
  }
  if (GENERATED_FILES.some((re) => re.test(lower))) return "GENERATED";

  // TEST detection: must be before SOURCE/CONFIG
  if (isTestFile(lower, base, ext)) return "TEST";

  // Special extensionless / well-known
  if (SPECIAL_SOURCELESS.has(base)) return "CONFIG"; // go.mod etc are CONFIG/BUILD
  if (BUILD_FILES.has(base)) return "BUILD";
  if (base === "dockerfile" || base.startsWith("dockerfile.")) return "BUILD";
  if (base === "makefile" || base.startsWith("makefile.")) return "BUILD";

  // DOC
  if (DOC_EXTS.has(ext)) return "DOC";
  // CONFIG
  if (CONFIG_EXTS.has(ext)) return "CONFIG";
  // SOURCE
  if (SOURCE_EXTS.has(ext)) return "SOURCE";
  // Also no ext but known source like "Makefile" already handled; unknown
  return "UNKNOWN";
}

function isTestFile(lower: string, base: string, ext: string): boolean {
  // Common conventions
  if (lower.startsWith("tests/") || lower.includes("/tests/") || lower.startsWith("test/") || lower.includes("/test/")) return true;
  if (lower.startsWith("__tests__/") || lower.includes("/__tests__/")) return true;
  if (lower.includes("/__tests__/")) return true;
  // Go: *_test.go
  if (lower.endsWith("_test.go")) return true;
  // Python: test_*.py, *_test.py
  if (ext === ".py" && (base.startsWith("test_") || base.endsWith("_test.py"))) return true;
  // JS/TS: *.test.ts, *.spec.ts, *.test.js, *.spec.js, *.test.tsx etc
  if (/\.(test|spec)\.(ts|tsx|js|jsx|mjs|cjs)$/.test(lower)) return true;
  // Java: *Test.java
  if (ext === ".java" && /test\.java$/.test(lower)) return true;
  // Also files under scoring/tests etc? Already via /tests/
  return false;
}

export function isSourceFile(p: string): boolean { return classifyFile(p) === "SOURCE"; }
export function isTestFilePath(p: string): boolean { return classifyFile(p) === "TEST"; }
export function isDocFile(p: string): boolean { return classifyFile(p) === "DOC"; }
export function isConfigFile(p: string): boolean { return classifyFile(p) === "CONFIG" || classifyFile(p) === "BUILD"; }
export function isGeneratedFile(p: string): boolean { return classifyFile(p) === "GENERATED"; }
