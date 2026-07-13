# Everything AI Runtime

Local-first, performance-first AI operating-system runtime. Rust owns execution and source-of-truth state; Python is an operator SDK; desktop/API shells remain thin control planes.

## Read First

Before modifying a subdirectory, read its nearest `AGENTS.md` if one exists. Use `graphify query "<question>"` before broad source searches; use `graphify path` for relationships and `graphify explain` for a symbol. After code changes, run `graphify update .`.

## Architecture Map

- `crates/everything-domain/` — stable serializable contracts: configuration, runs, planning, workspace and API DTOs. No I/O or orchestration.
- `crates/everything-adapters/` — filesystem, model and command port implementations. External effects stay behind typed traits.
- `crates/everything-graph/` — deterministic project/symbol graph, indexed lookup and impact traversal. Keep extraction cheap and evidence-backed.
- `crates/everything-runtime/` — orchestration, bootstrap/cache, planning, journals, metrics and composition root.
- `apps/everythingd/` — Axum control-plane API and SSE events; blocking native/model work goes through `spawn_blocking`.
- `apps/everything-cli/` — thin synchronous operator CLI over `ModularRuntime`.
- `python/everything_control/` — typed client/operator tooling; never move runtime authority here.
- `graphify-out/` — generated cross-project code graph, reports, query memory and visualizations. Do not hand-edit.

## Global Invariants

- Optimize measured hot paths; preserve correctness and deterministic fallbacks.
- Persist explicit run state before long or failure-prone work. Completed and failed outcomes must remain inspectable.
- Keep evaluators, safety policy and irreversible actions outside model-editable surfaces.
- Route repository understanding through the graph before wide reads; cite source locations for consequential claims.
- Keep Rust as the execution source of truth. API, CLI, Python and future desktop layers call narrow versioned boundaries.
- Prefer bounded fast paths. Add deeper planning, verification or model escalation only when task difficulty requires it.
- New graph edges must be structurally extracted or clearly marked inferred; never present guesses as observed relationships.

## Verification

- Rust: `cargo fmt --all -- --check`, `cargo test --workspace`, then the relevant CLI benchmark.
- Graph: `graphify update .`, `graphify diagnose multigraph`, and regenerate visual exports after structural changes.
- Intent layer: keep this file under 4k tokens and update the architecture map when ownership moves.
