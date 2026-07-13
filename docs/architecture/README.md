# Architecture

## Authority and dependency direction

`everything-domain` defines stable serializable contracts. `everything-adapters` implements typed ports for external effects. `everything-graph` owns deterministic repository extraction, graph metrics, retrieval, and change-impact analysis. `everything-research` owns hardened web discovery/fetch/cache. `everything-connectors` owns provider APIs and credentials. `everything-runtime` composes these layers and owns orchestration. CLI, daemon, Python, and Electron call narrow runtime boundaries and must not become alternate sources of truth.

```text
operator surfaces
  CLI | everythingd | Python SDK | Electron
                    |
             everything-runtime
          /      /      |       \
     domain  adapters  graph   research/connectors
```

## Runtime data

Workspace-local runtime data lives under `.everything/`. Persistent code-graph data and run/metrics data are distinct concerns and should remain independently migratable. Serialized contracts require explicit versions before long-lived compatibility is promised.

## Canonical code graph

`PersistentCodeGraph` is the canonical retrieval and impact substrate. It stores symbols and runtime contracts in SQLite, maintains directional indexed relations, computes centrality/fan-in/test-risk metrics, and answers bounded change-impact queries. Compatibility APIs may project simpler views but must not build an independent product graph.

Every mutation path performs graph preflight before applying a patch. A successful transaction refreshes the graph and persists a line-aware postflight report. Planner and edit context use the same graph revision, provenance, observed/inferred evidence, and mode-aware traversal budgets.

## Research substrate

`everything-research` is intentionally separate from browser UI and connector credentials. It performs bounded provider fan-out, safe HTTPS fetch, citation construction, and TTL/size-bounded SQLite/FTS caching. Runtime context labels all web material as untrusted evidence. The optional SearXNG sidecar is loopback-only and is not an authority boundary.

## Control planes

- `everythingd` exposes loopback HTTP v1 and SSE events.
- `everything-cli` is a synchronous native operator interface.
- `everything_control` calls the daemon HTTP API and never executes runtime work itself.
- Electron starts/owns a daemon process and proxies requests through isolated IPC.

## Non-negotiable safety rules

Model output never defines permissions. Irreversible actions, evaluators, policy, command allow/deny rules, and verification remain outside model-editable surfaces. External effects must be bounded, observable, attributable, and eventually recoverable.
