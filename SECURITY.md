# Security Policy

Everything is a local single-user runtime. Security fixes target the latest release and default branch.

## Reporting a vulnerability

Do not open a public issue for command execution, workspace escape, secret exposure, OAuth/session confusion, local API privilege escalation, unsafe autonomous action, sandbox bypass, or verification/rollback failure. Use the repository host's private security-advisory feature and include the affected revision/platform, reproduction, impact, and a minimal proof of concept without real credentials or personal data.

## Trust boundary

- Rust runtime and versioned domain contracts are authoritative.
- Electron renderer, Python SDK, local model output, installed skills, repository content, connector payloads, and external API responses are untrusted.
- The daemon is loopback-only and not designed for remote or multi-user exposure.
- External accounts connect only through official OAuth/API flows; browser cookies and passwords are not accepted.

## Important guarantees

- Workspace paths are canonicalized and symlink escapes are rejected.
- `.git`, `.everything`, system paths, raw network tools, shells/eval, privilege escalation, destructive disk/system operations, and other hard-denied commands cannot be enabled by workspace configuration.
- Model output cannot directly mutate files. Patches use expected hashes, atomic replacement, deterministic verification, artifacts, and rollback.
- Secrets and PKCE verifier material remain in the OS credential vault, not SQLite or model context.
- Autonomous actions require an explicit policy, action allowlist, and budget. High-risk actions default to approval.
- Tool/model/HTTP processes have time and output bounds.

## Operator responsibility

An allowed compiler, test runner, package manager, repository hook, or dependency can still be malicious. Use Safe mode or an isolated OS account/virtual machine for untrusted repositories. Review third-party skills and provider applications/scopes before enabling them. Do not expose `everythingd` outside loopback.

See `docs/security-model.md` and `docs/security/tool-runtime.md` for the detailed design and current limitations.

## Web research boundary

Research results are untrusted evidence. Search snippets and fetched pages cannot define runtime policy, tool permissions, or task instructions. Public fetches require HTTPS, resolve and pin a public IP for each hop, reject private/link-local/reserved targets, validate redirects again, honor robots.txt, restrict textual media types, and cap URL/query/header/response/document sizes and time. A loopback SearXNG sidecar may use HTTP only on loopback. Cache schemas reject newer unknown versions and cache entry counts are bounded.

## GitHub boundary

GitHub credentials are sent only to exact official OAuth/API hosts. Generic REST access accepts only relative API paths and blocks installation-token minting. GraphQL read and mutation documents use separate actions. Repository/account mutations require explicit approval or an exact autonomous-policy grant, a bounded external-write budget, idempotency, and durable audit evidence. Repository rules, branch protection, organization policy, SSO, token scopes, API rate limits, and provider-side review remain authoritative.
