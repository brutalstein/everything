# Changelog

All notable user-visible changes are recorded here. Everything follows semantic versioning while the persisted runtime schemas remain migration-aware.

## 0.3.0 — 2026-07-12

### Added

- Weighted code-graph change-impact analysis with callers, public API surfaces, tests, dependencies, verifier targets, centrality, fan-in, and multi-root convergence.
- Mandatory preflight impact evidence for every patch path plus line-aware postflight impact after successful graph refresh.
- Native web-research runtime with optional loopback SearXNG, keyless fallbacks, citations, primary-source ranking, domain diversity, bounded parallel fetch, and SQLite/FTS cache.
- Automatic current-information research in planner/edit context with explicit offline override and Fast/Balanced/Deep budgets.
- Comprehensive official GitHub connector covering typed repository, issue, pull-request, Actions, release, notification, content, branch, commit, search, and security-alert operations.
- Approval-gated generic GitHub REST and GraphQL mutation surfaces, plus read-only REST/GraphQL access.
- Electron research console and edit-approval blast-radius presentation.
- Built-in web-research and GitHub-operator skills.

### Changed

- Graph extraction now records tests, routes, events, environment variables, configuration keys, file reads/writes, and SQL query/mutation contracts.
- Impact traversal uses a deterministic priority heap rather than repeated frontier scans.
- Research provider fan-out is parallel, while page fetches are capped at six workers to preserve local-model CPU/RAM budget.
- Research ranking favors primary documentation and standards sources and prevents a single domain from monopolizing context.
- Smart setup optionally starts a hardened loopback SearXNG sidecar, but remains functional without Docker/Podman.

### Security

- Research fetches enforce public-IP DNS resolution, per-hop redirect validation, HTTPS-only public transport, robots.txt, bounded output/time, MIME restrictions, and cache schema/size limits.
- Web evidence is structurally isolated as untrusted context and cannot override system/user policy.
- GitHub tokens are restricted to exact official API hosts; mutating actions require approval, policy, idempotency, and durable audit evidence.

## 0.2.0 — 2026-07-12

### Added

- Durable local automation scheduler with DST-aware schedules, atomic leases, heartbeat renewal, retries, missed-run policy, dead-letter state, daily budgets, exact approval replay, and native subsystem doctor routines.
- Official OAuth/API connectors for Gmail, Spotify, TikTok, and eligible Instagram professional accounts.
- Smart local-model briefings that treat external content as untrusted data and avoid automatically persisting raw account payloads.
- Structured SQLite/FTS memory with evidence, confidence, validity, supersession, deduplication, source diversity, contradiction blocking, quotas, and forgettability controls.
- Installable `SKILL.md` plugin packages with compatibility, permission, enable/disable, and workflow-policy boundaries.
- One-command transactional installers with dependency setup, self-healing ports, user background service, rollback, native doctor evidence, model smoke, and install manifests.
- GitHub CI, dependency security checks, deterministic source releases, checksums, and provenance attestations across Linux, macOS, and Windows.

### Changed

- Connector OAuth defaults are conservative: Gmail is read-only; Spotify omits playback mutation; TikTok omits `video.publish`; Instagram omits content publishing. The desktop blocks under-scoped actions and routines before execution.
- Repository, memory, skill, tool, and connector content are treated as untrusted evidence rather than model instructions.
- Graph retrieval uses indexed directional traversal, observed/inferred weighting, per-file diversity, and model/mode-aware context budgets.
- Connections and routines use generated forms, with raw JSON moved behind advanced controls.
- Heavy Electron settings screens are loaded lazily to reduce initial renderer work.

### Security

- Workspace-trusted mode remains bounded by non-overridable denials for destructive system, disk, remote-access, and unrestricted network tooling.
- OAuth state is one-time and provider-bound; PKCE is used where supported; secrets remain in the operating-system vault; provider hosts, redirects, payloads, outputs, retries, and idempotency keys are bounded.
- External writes require explicit approval unless an exact provider/action pair is granted inside a narrow `ActWithinPolicy` budget.
