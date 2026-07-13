# GitHub Control Plane

Everything uses GitHub's official REST and GraphQL APIs through the connector runtime.

## Typed operations

Read operations cover profile/rate limits, repository metadata, branches, commits, contents, code search, issues, pull requests, notifications, workflow runs, releases, and Dependabot/code-scanning/secret-scanning alerts. Mutating operations cover issues/comments, pull requests/merge, workflow dispatch/rerun/cancel, releases, notification state, generic REST mutations, and GraphQL mutations.

## Complete API escape hatch

`rest_read` and approval-gated `rest_write` accept bounded relative API paths and scalar query values. `graphql_read` rejects mutation documents; `graphql_mutation` requires a mutation document and the mutation approval path. This provides broad official API coverage without allowing arbitrary hosts or raw credential-bearing network commands.

## Performance and reliability

- Exact official host allowlist and current API-version header.
- Typed schemas reject malformed payloads before network I/O.
- Bounded request/response/time limits and disabled redirects.
- Durable connector audit, sanitized errors, idempotency keys, retry classification, and routine budgets.
- Profile operations include provider rate-limit metadata so operators and autonomous policies can avoid wasteful retries.

## Permissions

Connections start conservatively. GitHub PAT/OAuth scopes, repository permissions, organization SSO, branch protection, rulesets, and Actions policies remain provider-enforced. Everything never bypasses them. External writes require explicit approval unless an exact provider/action pair is granted to a bounded `ActWithinPolicy` routine.
