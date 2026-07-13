# Autonomous Routines

Everything's scheduler is a durable local state machine, not an in-memory timer. It runs inside `everythingd`, so routines continue while the desktop window is closed when the installer-created user service is active.

## Schedules

Supported schedules:

- one-time timestamp;
- fixed interval, minimum 60 seconds;
- daily local wall-clock time resolved from the operating system on every occurrence;
- selected local weekdays at a wall-clock time;
- fixed-offset daily/weekly schedules for API compatibility and deliberately offset-bound jobs.

The desktop app uses DST-aware local schedules by default, so a `09:00` routine remains at local 09:00 across daylight-saving transitions. Fixed-offset variants remain available through the API for workloads that intentionally follow UTC offsets instead of civil time.

## Actions

A routine can run:

- a graph-grounded repository plan;
- an enabled, runtime-compatible Everything skill;
- a native Everything doctor report that checks all runtime subsystems without a model call;
- a privacy-bounded local briefing assembled from up to eight read-only official connector sources;
- an official connector action.

Plans and skills reuse the native graph, memory retrieval, model budgets, typed tools, artifacts, verifier, and rollback engine. Connector actions reuse OAuth, scope checks, host allowlists, audit, idempotency, and approval policy.

## Autonomy levels

- `Observe` — preview or read-only behavior; no mutations.
- `Assist` — performs safe reads and stops before a mutation.
- `ActWithApproval` — a mutation can proceed only when an operator explicitly runs/approves it.
- `ActWithinPolicy` — permits only the exact workspace/connector operations granted in that routine's policy.

`ActWithinPolicy` is not an unrestricted agent switch. It still enforces action allowlists, model/tool/write budgets, process restrictions, workspace containment, verification, and failure suspension.

## Reliability model

For each due routine the runtime:

1. atomically acquires an owner-token SQLite lease sized to the routine runtime budget;
2. writes a claimed execution record before performing work;
3. enforces per-run and per-day budgets;
4. executes the typed action;
5. writes status, run/artifact links, sanitized output metadata, and error classification;
6. renews and releases the lease only when the same owner still holds it;
7. calculates the next occurrence and releases the lease.

Concurrent workers cannot claim the same occurrence, and an expired worker cannot release a replacement worker's lease. Scheduled occurrence buckets provide deterministic deduplication. Transient failures use bounded exponential retry while preserving the original occurrence idempotency key. Exhausted retries enter a visible dead-letter state. Missed occurrences follow an explicit `RunOnce` or `Skip` grace policy. A routine is automatically disabled after its configured consecutive-failure threshold and remains inspectable. One-time routines disable themselves after execution. The desktop history shows the last 50 executions, retries, approvals, dead letters, and evidence.

Approval-waiting executions are durable. Approving from the desktop replays the exact pending occurrence instead of creating a semantically different job. Preview/approval bookkeeping does not consume the daily execution budget twice.

## Privacy and memory

Routine outcomes may create a short workspace-memory record containing the routine name, status, time, execution ID, and run link. Raw email bodies, social-media payloads, OAuth tokens, and account secrets are not automatically copied into long-term memory. Smart briefing source payloads remain ephemeral; only the locally generated summary and model invocation evidence are retained.

Memory retrieval is scoped by workspace, validity, confidence, supersession state, provenance, and token budget. Duplicate content is fingerprinted, contradictory active memories are withheld as explicit blockers, source diversity is enforced, FTS queries are bounded, sensitive paths are excluded, and low-priority segments are deterministically dropped before the user objective or runtime policy.

Repository text, comments, filenames, memory, connector payloads, and skill instructions are always marked as untrusted data. They cannot override runtime policy or the user's explicit objective, which limits prompt-injection from code, email, and social content.

## Suggested routines

The desktop app includes starting points for:

- weekday morning unread-mail summary;
- read-only Spotify playback snapshot;
- daily native Everything subsystem doctor;
- daily repository health review;
- weekly dependency, supply-chain, and license review.

Installed skills can also be selected as routine actions. A skill that requests workspace mutation waits for approval unless the routine explicitly grants workspace mutation under `ActWithinPolicy`.

## Notifications

When the desktop app is open, completed, failed, and approval-waiting routines generate privacy-preserving operating-system notifications. Notifications contain only generic status text; details stay in the local Routines screen and SQLite audit records.
