# iQOO Hackathon 2026 — Context Engine Pitch: Speaker Notes

Total: ~3 minutes (20–30s per slide).

---

## Slide 1 — Title (20s)

> Context Engine is a local repository intelligence layer for AI coding agents.
> Index the repo once — and let every coding agent continue with the right context.
> One shared engine underneath Codex, Claude, OpenCode, and any MCP-compatible agent.

## Slide 2 — Problem (30s)

> Every new agent session starts from zero. Agent A explores the repo — symbols, dependencies, tests — then the session ends.
> Agent B repeats the same discovery, burning context and tokens.
> And if the repo changed in between, even copied-over context may be stale.
> Three costs: wasted context, lost continuity, stale evidence.

## Slide 3 — Solution (25s)

> Context Engine indexes the repository once — structural, lexical BM25, and semantic retrieval —
> packs compact evidence-backed context, and serves it over an agent-neutral MCP interface.
> Any compatible coding agent asks; the engine answers from one shared, verified index.

## Slide 4 — Differentiator (30s)

> Search is only step one — continuity is the real problem.
> Context Proof records exactly what Agent A saw: evidence hashes plus the indexed repo state.
> Context Delta checks whether that evidence is still valid: unchanged, changed, removed — or unverifiable, because we prefer honesty over guessing.
> Cross-Agent Handoff gives Agent B the delta, fresh current context, and a new proof.
> No chat-history replay required — the handoff reference is tiny, and the intelligence stays local.
> Note the status tags: Proof and Delta are already built — Handoff is what we complete at iQOO.

## Slide 5 — Demo (30s)

> Watch it live. Agent A asks where retry policy is enforced, and gets evidence plus Proof A.
> A developer edits an unrelated file. Agent B starts fresh with only the query and Proof A — no chat history.
> The engine reports the retry evidence UNCHANGED, returns fresh context, and mints Proof B.
> Then we edit retry.rs itself: Agent C continues from Proof B, Delta reports CHANGED, and fresh implementation context comes back.
> Both paths — untouched and edited — proven in one workflow.

## Slide 6 — Credibility (20s)

> Why does this matter when developers use multiple AI agents? Three outcomes:
> less rediscovery through one shared index, safer continuity through verified evidence,
> and agent independence — switch agents without moving entire chats.
> Underneath: local-first, deterministic Proof-to-Delta-to-Handoff, one shared daemon,
> and conservative by design — UNVERIFIABLE instead of guessed.
> Search finds code. Context Engine makes repository context survive safely across independent agent sessions.

## Slide 7 — Plan + Team (25s)

> Proof and Delta already work. At iQOO we turn those foundations into the cross-agent handoff workflow
> and measure it in a controlled A/B demo: task completion, repo exploration, tool calls, context usage.
> The result we want: Agent B solves the task without rediscovering the repository.
> Team Mulanous, Sandeep Gurram — production systems, backend architecture, retrieval infrastructure,
> Google hackathon participant.
> Pre-existing work is disclosed on the slide. One codebase. Many agents. Continuous context.
