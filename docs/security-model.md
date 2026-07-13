# Security Model

Everything is a local single-user runtime. The Rust daemon is the source of authority. Electron, the Python SDK, local model output, installed skills, connector payloads, and repository content are untrusted inputs.

## Network boundary

- The daemon binds to explicit loopback addresses only.
- Desktop API calls are restricted to versioned local `/v1/` routes.
- The renderer runs with context isolation, sandboxing, no Node integration, and a narrow preload bridge.
- External links open in the system browser; renderer navigation is denied.
- Official connectors use HTTPS, exact provider host allowlists, bounded responses, timeouts, and HTTPS-only redirects.
- Custom HTTP connectors are disabled by default.

Do not expose `everythingd` through a reverse proxy or non-loopback socket. Multi-user remote operation is outside the threat model.

## Workspace permissions

`Safe` mode permits a conservative executable allowlist. `TrustedWorkspace` can run a broader set of developer tools and perform approved workspace writes, but it never disables the hard denylist.

The hard denylist covers shells/eval entrypoints, privilege escalation, system/package management from agent tools, raw network clients, remote login, disk/partition tools, destructive recursive deletion, service manipulation, and commands that can escape through arbitrary script evaluation. Paths are canonicalized, symlink escapes are rejected, `.git` and `.everything` internals are protected, file reads are bounded, and writes use expected hashes plus atomic replacement.

On Linux, Bubblewrap is used when available. On macOS, `sandbox-exec` is used when available. These profiles restrict filesystem writes to the workspace and remove general network access for tool processes. When an OS sandbox is unavailable, Everything falls back to the strict executable allowlist rather than silently granting unrestricted command execution.

## Mutation transaction

1. The model proposes a bounded change; it cannot directly write files.
2. Everything reads the current file and records its content hash.
3. The operator sees a diff unless a narrow autonomous policy already grants the operation.
4. The patch applies only if the base hash still matches.
5. Verification commands run through the typed process policy with time/output limits.
6. Required verification failure triggers a hash-controlled rollback of only the change Everything applied.
7. Diff, patch, verification, invocation, and rollback evidence is stored as content-addressed artifacts.

A failed patch fingerprint is blocked from blind replay unless the operator supplies an explicit override.

## Secrets and OAuth

Access tokens, refresh tokens, client secrets, and temporary PKCE verifiers live in macOS Keychain, Linux Secret Service/libsecret, or Windows Credential Manager. Secret keys are validated and helper processes have bounded input/output and timeouts. Tokens are not stored in SQLite, logs, artifacts, notifications, or model context.

OAuth state is random, provider-bound, expiring, and one-time. PKCE verifier material is state-bound in the vault. Token refresh uses a single-flight lock to avoid concurrent refresh races for the same account.

## Durable execution

Runs, events, checkpoints, artifacts, tool/model invocations, automations, leases, connector audits, and memory use SQLite WAL mode. Long operations are preceded by durable state. Schema versions reject newer unknown databases. Corrupt individual JSON/event rows are isolated. In-progress tool invocations are deterministically marked interrupted after restart.

## Memory controls

Memory entries carry scope, source, workspace key, version, confidence, validity interval, evidence IDs, tags, supersession, editability, and forgettability. Entry size, tag/evidence count, identifier length, workspace-key length, search length, and result limits are bounded. Non-forgettable entries are not removed by automatic expiry compaction.

External-account content is not retained automatically. Users can inspect and delete forgettable entries from the desktop UI or API.

## Remaining limitations

Everything reduces risk but cannot prove that every allowed compiler, package manager, test runner, or repository hook is benign. Treat untrusted repositories as hostile, use Safe mode, inspect project scripts, and prefer an OS account or virtual machine with limited privileges. Provider applications and OAuth consent configuration remain the operator's responsibility.
