# Tool Runtime Security Model

Everything executes mutations through a typed, policy-gated tool registry. Model output is never interpreted as an unrestricted shell command.

## Default policy

| Permission | Default decision |
| --- | --- |
| `workspace_read` | Allow |
| `workspace_write` | Require explicit approval |
| `process_execute` | Require explicit approval |
| `git_read` | Allow |
| `git_write` | Require explicit approval |
| `network_local` | Deny |
| `network_external` | Deny |
| `system_install` | Deny |

Approval is scoped to the submitted invocation. The model cannot modify `everything.toml`, `.everything/`, or `.git/` through workspace tools.

## Process boundary

`process.run` accepts only an executable name from `[tools].allowed_programs`. Program paths, shell interpolation, and workspace escape are rejected. The runner clears the environment, restores only a small operational allowlist, confines the working directory to the workspace, bounds captured output, records exit status/signal, enforces a timeout, and can terminate the process tree.

## Patch transaction

`workspace.apply_patch` performs these steps:

1. Resolve and confine the target to the workspace.
2. Reject protected runtime/Git metadata and `everything.toml`.
3. Compare the current BLAKE3 content hash with `expected_content_hash`.
4. Produce a dry-run diff.
5. Atomically replace the file through temporary and backup paths.
6. Validate the persisted post-write hash.
7. Run configured verification commands.
8. Roll back only when the current file still matches the hash produced by this transaction.

Diffs, verification reports, rollback reports, checkpoints, and invocation records are persisted under `.everything/` and referenced by the run journal.

## API surfaces

- `GET /v1/tools`
- `POST /v1/tools/invocations/{invocation_id}`
- `GET /v1/tools/invocations/{invocation_id}`
- `POST /v1/tools/invocations/{invocation_id}/cancel`
- `GET /v1/runs/{run_id}/tool-invocations`
- `POST /v1/executions/patch`

A mutating request without `approval_granted: true` is recorded as `AwaitingApproval` without changing the workspace.
