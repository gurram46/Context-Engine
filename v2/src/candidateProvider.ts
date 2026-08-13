#!/usr/bin/env node
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import { createCodeIndexClient, setActiveProjectRoot } from "./retrieval/codeIndexClient.js";
import path from "node:path";

const projectRoot = process.env.CONTEXT_ENGINE_PROJECT_ROOT || process.cwd();
setActiveProjectRoot(path.resolve(projectRoot));
const client = createCodeIndexClient();

const server = new Server(
  { name: "contextd-candidate-provider", version: "0.1.0" },
  { capabilities: { tools: {} } }
);

server.setRequestHandler(ListToolsRequestSchema, async () => {
  return {
    tools: [
      {
        name: "symbol_candidates",
        description: "Raw symbol candidates via OCI implementation_lookup",
        inputSchema: {
          type: "object",
          properties: { symbol: { type: "string" } },
          required: ["symbol"],
        },
      },
      {
        name: "semantic_candidates",
        description: "Raw semantic candidates via OCI peek/search",
        inputSchema: {
          type: "object",
          properties: { query: { type: "string" }, limit: { type: "number" } },
          required: ["query"],
        },
      },
      {
        name: "graph_candidates",
        description: "Raw graph candidates via OCI call_graph",
        inputSchema: {
          type: "object",
          properties: { symbol: { type: "string" }, direction: { type: "string", enum: ["callers", "callees"] } },
          required: ["symbol"],
        },
      },
      {
        name: "test_candidates",
        description: "Raw test candidates via OCI peek for test queries",
        inputSchema: {
          type: "object",
          properties: { query: { type: "string" } },
          required: ["query"],
        },
      },
      {
        name: "index_status",
        description: "Index status",
        inputSchema: { type: "object", properties: {} },
      },
    ],
  };
});

server.setRequestHandler(CallToolRequestSchema, async (request) => {
  const { name, arguments: args } = request.params;
  try {
    if (name === "symbol_candidates") {
      const symbol = (args as any).symbol as string;
      const ev = await client.lookupImplementation(symbol);
      return { content: [{ type: "text", text: JSON.stringify({ candidates: ev }, null, 2) }] };
    }
    if (name === "semantic_candidates") {
      const query = (args as any).query as string;
      const limit = (args as any).limit as number | undefined;
      // Use peek for low-token, search for full
      const peek = await client.peek(query, limit ?? 10);
      // Also try search for more coverage, but return peek as primary
      // For now, return peek only to keep raw
      return { content: [{ type: "text", text: JSON.stringify({ candidates: peek }, null, 2) }] };
    }
    if (name === "graph_candidates") {
      const symbol = (args as any).symbol as string;
      const direction = ((args as any).direction as string) || "callers";
      const ev = await client.callGraph(symbol, direction as any);
      return { content: [{ type: "text", text: JSON.stringify({ candidates: ev }, null, 2) }] };
    }
    if (name === "test_candidates") {
      const query = (args as any).query as string;
      const ev = await client.peek(query, 10);
      // Filter to test-like? Keep raw for Rust to decide
      return { content: [{ type: "text", text: JSON.stringify({ candidates: ev }, null, 2) }] };
    }
    if (name === "index_status") {
      const s = await client.status();
      return { content: [{ type: "text", text: s }] };
    }
    return { content: [{ type: "text", text: JSON.stringify({ error: `Unknown tool ${name}` }) }], isError: true };
  } catch (e: any) {
    return { content: [{ type: "text", text: JSON.stringify({ error: e?.message ?? String(e) }) }], isError: true };
  }
});

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  const shutdown = async () => {
    try { await client.close(); } catch {}
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}
main().catch((e) => { console.error(e); process.exit(1); });
