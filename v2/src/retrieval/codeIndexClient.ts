/**
 * Minimal MCP stdio client for open-codebase-index
 * Reuses one child per CE process, graceful shutdown, per-request timeout.
 * Only wraps verified tools: status, lookupImplementation, peek, search, callGraph, callGraphPath
 */
import { spawn, ChildProcess } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { Evidence } from "../core/types.js";

type JsonRpcResponse = { jsonrpc: "2.0"; id: number; result?: any; error?: any };

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
// v2/dist/retrieval -> v2 -> Context-Engine root
const CONTEXT_ENGINE_ROOT = path.resolve(__dirname, "../../..");

function defaultProjectRoot(): string {
  return process.env.CONTEXT_ENGINE_PROJECT_ROOT || process.cwd();
}

function cliPath(): string {
  // Always use Context-Engine's installed binary, not the target repo's
  return path.join(CONTEXT_ENGINE_ROOT, "node_modules/open-codebase-index/dist/cli.js");
}

let activeProjectRoot: string = defaultProjectRoot();
export function setActiveProjectRoot(root: string) {
  activeProjectRoot = path.resolve(root);
}
export function getActiveProjectRoot(): string {
  return activeProjectRoot;
}

class McpClient {
  private proc: ChildProcess | null = null;
  private nextId = 1;
  private pending = new Map<number, { resolve: (v: any) => void; reject: (e: any) => void; timer: NodeJS.Timeout }>();
  private buffer = "";
  private ready = false;
  private projectRoot: string;

  constructor(projectRoot?: string) {
    this.projectRoot = projectRoot ? path.resolve(projectRoot) : getActiveProjectRoot();
  }

  async start(): Promise<void> {
    if (this.proc) return;
    // Spawn OCI MCP with cwd = target project so it indexes that repo
    const proc = spawn("node", [cliPath()], { stdio: ["pipe", "pipe", "pipe"], cwd: this.projectRoot });
    this.proc = proc;
    proc.stdout.on("data", (d) => this.onData(d));
    proc.stderr.on("data", () => {});
    proc.on("exit", () => this.failAll(new Error("MCP exited")));
    // initialize
    const res = await this.request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "context-engine-v2", version: "0.1.0" },
    }, 15000);
    void res;
    this.sendNotification("notifications/initialized", {});
    await new Promise((r) => setTimeout(r, 100));
    this.ready = true;
  }

  private onData(d: Buffer): void {
    this.buffer += d.toString();
    let idx: number;
    while ((idx = this.buffer.indexOf("\n")) !== -1) {
      const line = this.buffer.slice(0, idx).trim();
      this.buffer = this.buffer.slice(idx + 1);
      if (!line) continue;
      try {
        const msg = JSON.parse(line) as JsonRpcResponse;
        if (msg.id && this.pending.has(msg.id)) {
          const p = this.pending.get(msg.id)!;
          clearTimeout(p.timer);
          this.pending.delete(msg.id);
          if (msg.error) p.reject(new Error(JSON.stringify(msg.error)));
          else p.resolve(msg.result);
        }
      } catch {}
    }
  }

  private failAll(err: Error): void {
    for (const [, p] of this.pending) {
      clearTimeout(p.timer);
      p.reject(err);
    }
    this.pending.clear();
  }

  private sendNotification(method: string, params: any): void {
    this.proc?.stdin?.write(JSON.stringify({ jsonrpc: "2.0", method, params }) + "\n");
  }

  request(method: string, params: any, timeoutMs = 15000): Promise<any> {
    const id = this.nextId++;
    const payload = JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n";
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`MCP timeout ${method}`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.proc?.stdin?.write(payload, (err) => {
        if (err) {
          clearTimeout(timer);
          this.pending.delete(id);
          reject(err);
        }
      });
    });
  }

  async callTool(name: string, args: any, timeoutMs = 15000): Promise<string> {
    await this.start();
    const res = await this.request("tools/call", { name, arguments: args }, timeoutMs);
    // result.content[0].text
    const text = res?.content?.[0]?.text ?? "";
    return text;
  }

  async close(): Promise<void> {
    for (const [, p] of this.pending) {
      clearTimeout(p.timer);
      p.reject(new Error("client closed"));
    }
    this.pending.clear();
    if (this.proc) {
      try { this.proc.kill(); } catch {}
      this.proc = null;
    }
  }
}

let singleton: McpClient | null = null;
let singletonRoot: string | null = null;
function client(): McpClient {
  const root = getActiveProjectRoot();
  if (!singleton || singletonRoot !== root) {
    if (singleton) { try { singleton.close(); } catch {} }
    singleton = new McpClient(root);
    singletonRoot = root;
  }
  return singleton;
}
export function createCodeIndexClientForRoot(root: string): CodeIndexClient {
  setActiveProjectRoot(root);
  // Force new singleton for that root
  if (singleton) { try { singleton.close(); } catch {} singleton = null; singletonRoot = null; }
  return createCodeIndexClient();
}

// ---- parsers: OCI returns formatted text, parse back to Evidence ----

function parsePeekLike(text: string, source: Evidence["source"], relation: Evidence["relation"] = "unknown"): Evidence[] {
  const out: Evidence[] = [];
  // Header may be followed by ``` code block (for implementation_lookup/search)
  // Split into chunks starting with [n]
  const chunks = text.split(/(?=\[\d+\])/g);
  const reAt = /\[(\d+)\]\s+(\S+)\s+(?:"([^"]+)"\s+)?at\s+(.+):(\d+)-(\d+)\s+\(score:\s*([0-9.]+)\)/;
  const reIn = /\[(\d+)\]\s+(\S+)\s+(?:"([^"]+)"\s+)?in\s+(.+):(\d+)-(\d+)\s+\(score:\s*([0-9.]+)\)/;
  for (const chunk of chunks) {
    const header = chunk.split("\n")[0] ?? "";
    let m = header.match(reAt) || header.match(reIn);
    if (!m) {
      // fallback line-by-line for peek without code blocks
      for (const l of chunk.split("\n")) {
        m = l.match(reAt) || l.match(reIn);
        if (!m) continue;
        const [, , kind, symbol, fileRaw, sLine, eLine, score] = m;
        const rel = normalizeFile(fileRaw.trim());
        out.push({
          source,
          file: rel,
          startLine: Number(sLine),
          endLine: Number(eLine),
          symbol: symbol || undefined,
          symbolKind: kind,
          score: Number(score),
          relation,
          provenance: `oci:${source}`,
          metadata: { raw: l.slice(0, 500) },
        });
        break;
      }
      continue;
    }
    const [, , kind, symbol, fileRaw, sLine, eLine, score] = m;
    const rel = normalizeFile(fileRaw.trim());
    // Extract code block if present
    const codeMatch = chunk.match(/```[\s\S]*?```/);
    const code = codeMatch ? codeMatch[0].slice(3, -3).trim().slice(0, 800) : undefined;
    const firstLine = header.slice(0, 500);
    out.push({
      source,
      file: rel,
      startLine: Number(sLine),
      endLine: Number(eLine),
      symbol: symbol || undefined,
      symbolKind: kind,
      text: code,
      score: Number(score),
      relation,
      provenance: `oci:${source}`,
      metadata: { raw: firstLine, codeSnippet: code?.slice(0, 200) },
    });
  }
  // Fallback if chunk split failed (e.g., no [n] at start)
  if (out.length === 0) {
    const re = /\[(\d+)\]\s+(\S+)\s+(?:"([^"]+)"\s+)?(?:at|in)\s+(.+):(\d+)-(\d+)\s+\(score:\s*([0-9.]+)\)/g;
    let mm: RegExpExecArray | null;
    while ((mm = re.exec(text)) !== null) {
      const kind = mm[2], symbol = mm[3], fileRaw = mm[4], sLine = mm[5], eLine = mm[6], score = mm[7];
      out.push({
        source,
        file: normalizeFile(fileRaw.trim()),
        startLine: Number(sLine),
        endLine: Number(eLine),
        symbol: symbol || undefined,
        symbolKind: kind,
        score: Number(score),
        relation,
        provenance: `oci:${source}`,
        metadata: { raw: mm[0].slice(0, 500) },
      });
    }
  }
  return out;
}

function normalizeFile(p: string): string {
  // handle Windows C:\... -> relative to active project root
  const root = getActiveProjectRoot();
  const abs = path.isAbsolute(p) ? p : path.join(root, p);
  const rel = path.relative(root, abs);
  const posix = rel.split(path.sep).join("/");
  if (posix.startsWith("..")) {
    const base = p.split(/[\\/]/).slice(-3).join("/");
    return base;
  }
  return posix || p.split(/[\\/]/).slice(-2).join("/");
}

function parseImplementationLookup(text: string): Evidence[] {
  // format: [1] function_definition "count_tokens" in C:\...:54-60 (score: 0.99) ```def...
  return parsePeekLike(text, "symbol", "definition");
}

function parseCallGraph(text: string, direction: "callers" | "callees"): Evidence[] {
  if (text.includes("No callers") || text.includes("No indexed symbol")) return [];
  const out: Evidence[] = [];
  // "bundle" at backend/.../bundle_command.py:22 calls 52 function(s):
  // [1] → Config (Call) at line 24 [unresolved]
  // We lack file for many; try to extract file from header if present
  const header = text.match(/"([^"]+)"\s+at\s+([^\s]+):(\d+)/);
  const baseFile = header ? normalizeFile(header[2]) : "";
  const lines = text.split("\n");
  for (const l of lines) {
    const m = l.match(/\[(?:\d+)\]\s+→\s+(\S+)\s+\((\w+)\)\s+at line (\d+)\s+\[(resolved|unresolved)\]/);
    if (!m) continue;
    const [, name, rel] = m;
    // Without file, attribute to baseFile with that line (best effort)
    out.push({
      source: "graph",
      file: baseFile || "unknown",
      startLine: Number(m[3]),
      symbol: name,
      symbolKind: rel,
      score: m[4] === "resolved" ? 0.9 : 0.5,
      relation: direction === "callers" ? "caller" : "callee",
      provenance: `oci:call_graph:${direction}`,
      metadata: { raw: l.slice(0, 500) },
    });
  }
  // If no bracket list but header exists, still return header as definition edge
  return out;
}

export interface CodeIndexClient {
  status(): Promise<string>;
  lookupImplementation(symbol: string): Promise<Evidence[]>;
  peek(query: string, limit?: number): Promise<Evidence[]>;
  search(query: string, limit?: number): Promise<Evidence[]>;
  callGraph(symbol: string, direction?: "callers" | "callees"): Promise<Evidence[]>;
  callGraphPath(from: string, to: string): Promise<Evidence[]>;
  close(): Promise<void>;
}

export function createCodeIndexClient(): CodeIndexClient {
  const c = client();
  return {
    async status(): Promise<string> {
      return c.callTool("index_status", {});
    },
    async lookupImplementation(symbol: string): Promise<Evidence[]> {
      const text = await c.callTool("implementation_lookup", { query: symbol });
      return parseImplementationLookup(text);
    },
    async peek(query: string, limit = 10): Promise<Evidence[]> {
      const text = await c.callTool("codebase_peek", { query, limit });
      if (text.startsWith("No matching")) return [];
      return parsePeekLike(text, "semantic");
    },
    async search(query: string, limit = 5): Promise<Evidence[]> {
      const text = await c.callTool("codebase_search", { query, limit });
      if (text.startsWith("No matching")) return [];
      // codebase_search has ``` content blocks; still parse header lines
      return parsePeekLike(text, "semantic");
    },
    async callGraph(symbol: string, direction: "callers" | "callees" = "callers"): Promise<Evidence[]> {
      const text = await c.callTool("call_graph", { name: symbol, direction });
      return parseCallGraph(text, direction);
    },
    async callGraphPath(from: string, to: string): Promise<Evidence[]> {
      const text = await c.callTool("call_graph_path", { from, to });
      if (text.includes("No path") || text.includes("No indexed")) return [];
      // Path (2 hops): [start] bundle (...) --Call--> _manual_fixed_bundle (...)
      const re = /(\S+)\s+\([^)]+\)\s+--\w+-->\s+(\S+)\s+\([^)]+\)/g;
      const out: Evidence[] = [];
      let m: RegExpExecArray | null;
      while ((m = re.exec(text)) !== null) {
        out.push({
          source: "graph",
          file: "path",
          symbol: `${m[1]}->${m[2]}`,
          score: 0.8,
          relation: "callee",
          provenance: "oci:call_graph_path",
        });
      }
      if (out.length === 0 && text.includes("-->")) {
        out.push({ source: "graph", file: "path", symbol: text.slice(0, 200), score: 0.6, relation: "callee", provenance: "oci:call_graph_path" });
      }
      return out;
    },
    async close(): Promise<void> {
      await c.close();
      singleton = null;
    },
  };
}

// For tests: ensure process exit cleans up
if (typeof process !== "undefined") {
  process.on("exit", () => { try { singleton?.close(); } catch {} });
}
