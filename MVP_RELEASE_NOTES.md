# Everything 0.3.0 Apex

Everything 0.3.0 turns the local-first coding and automation MVP into an impact-aware engineering runtime with first-class current web research and a comprehensive official GitHub control plane.

## Highlights

- Weighted, evidence-bearing change-impact analysis before every patch and line-aware postflight analysis after successful changes.
- Graph extraction for symbols, calls, references, imports, tests, routes, events, environment/configuration contracts, file I/O, and SQL objects.
- Model context ranked by centrality, fan-in, observed relations, multi-root convergence, source diversity, confidence, and token cost.
- Native web research with optional local SearXNG, keyless fallbacks, primary-source ranking, citation hashes, hardened HTTPS fetch, robots handling, SSRF protection, and bounded SQLite/FTS cache.
- Automatic research for current/external tasks, with explicit offline override and mode/model-aware budgets.
- GitHub OAuth/PAT connector with typed operations for repositories, branches, commits, contents, code search, issues, pull requests, releases, notifications, Actions, and security alerts.
- Approval/idempotency-gated generic GitHub REST mutations and query/mutation-separated GraphQL access.
- Research and GitHub built-in skills, Electron research console, impact reports in edit approval, and identical daemon/CLI/Python contracts.
- Optional hardened loopback SearXNG sidecar integrated into transactional setup without making Docker a mandatory dependency.

## Safety boundaries

Web pages, repository code/comments, connector payloads, memory, and skill instructions are untrusted evidence. They cannot change runtime policy, grant permissions, or bypass approval. Public web fetches are HTTPS-only, DNS-pinned, redirect-bounded, response-bounded, robots-aware, and blocked from private/special networks. GitHub credentials are sent only to exact official hosts; generic API paths cannot mint installation tokens or redirect to another host.

## Verification

The release workflow must pass Rust format/clippy/tests on Linux, macOS, and Windows; Python tests/package build; Electron typecheck/build/audit; installer/static smoke tests; deterministic source archive verification; and release provenance generation. A local setup does not report success until native build, daemon doctor, and configured Ollama smoke gates pass.
