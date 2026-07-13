# plonga-plongo-loop.md

> Orchestrator protocol for MORONIC sub-agents on a single git branch.

## Roles

- **Orchestrator** (this agent): defines `$stn`, spawns sub-agents in sequence, pushes at the end. Does NOT write code.
- **Sub-agents**: each is a distinct, context-isolated spawn. Reads `sub-agents-traits.md` first, then does one specific job, then appends to the shared worklog.

## Tools

- Markdown files (`MDs`) — trait docs, plans, critiques, hunt reports.
- One git branch — `$stn` (short-goal-name). Never create another.
- Diffs and commit logs — used to verify sub-agent work.

## The Loop

1. **L1 scaffolding** — `klemer-agents.md` L1: contracts only. Types, signatures, empty method bodies. NO logic.
2. **Devil's advocate** — critique the L1 output. Identify missed/skipped items + alignment with `docs/grafeo-loro.architecture.md`.
3. **Fixer + L2** — based on the critique, evolve or reduce the L1 scaffolds into L2 skeletons. Define internal state, wire the execution path. Complex algorithms stay as `// TODO`.
4. **L3 meat** — fill in all TODOs. Zero stubs, zero band-aids.
5. **plenger hunter** — scan the L3 output for the 8 anti-patterns in `plenger-traits.md`.
6. If plenger issues found → back to step 3 (Fixer + L2 evolves again).
7. Push `$stn`.

## Rules

1. Define `$stn : short-goal-name` before spawning anything.
2. On spawning, ask sub-agents to read & comply with `sub-agents-traits.md`.
3. Each step below must be run by a DISTINCT sub-agent for context isolation.
4. Orchestrator tools: MDs, one git branch, diffs, commit logs.
5. User decides to proceed to the next task for a new session loop.
