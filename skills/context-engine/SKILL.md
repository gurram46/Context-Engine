# Context Engine Skill

> Local repository intelligence — retrieve context, don't search manually.

**When to use:** Before broad repository exploration (unknown implementation location, cross-file behavior, architecture flow, dependency tracing, tests covering behavior, conceptual questions).

**How:**
```bash
contextd search "<natural language repository question>" --json
```

Examples:
```bash
contextd search "Where is count_tokens implemented?" --json
contextd search "Where is payment retry enforced?" --json
contextd symbol count_tokens --json
contextd dependency bundle_command --direction callers --json
contextd tests "bundle generation" --json
contextd status --json
```

**Policy:**
- Prefer one high-quality Context Engine request over repeated grep/read cycles.
- If returned evidence is sufficient, stop searching.
- For known exact path/string where shell tools are obviously cheaper, normal exact tools are acceptable.
- Context Engine retrieves context only. Continue editing/testing using the agent's normal tools.

**Output:** `--json` gives stable JSON `{query, type, evidence, context, stats}`; without `--json` gives concise readable evidence. Results are ranked compact evidence (~500-2000 tokens).

**Notes:** Project root auto-detected; use `--root <path>` to override, `--budget <tokens>` and `--max-results <n>` to tune. Tracing to stderr, stdout is result only.
