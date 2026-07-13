# Operator Interface Requirements

## Purpose

This document defines what the Electron-based operator surface must expose.

The UI is not a decorative shell.
It is the main human control plane for the runtime.

## Primary UX Goal

A user should be able to control, inspect, understand, and recover the system from the UI without dropping to the CLI for core workflows.

## Non-Negotiable UI Principle

If a capability is operationally important, it must be accessible from the interface.

That includes:

- execution
- debugging
- memory inspection
- graph exploration
- model selection
- installation diagnostics
- benchmark review
- skill management
- failure analysis

## Core UI Modules

### 1. App shell

Must provide:

- workspace switching
- global navigation
- panel layout persistence
- keyboard navigation
- window state persistence
- theme and accessibility controls

### 2. Runtime console

Must provide:

- start task
- stop task
- cancel task
- retry task
- resume task
- inspect active stage
- see current model
- see current run status
- see current budgets
- see current blockers

### 3. Run journal viewer

Must provide:

- run timeline
- tool actions
- observations
- decisions
- artifacts
- failures
- comparisons between runs
- filtering by severity, task, or status

### 4. Graph explorer

Must provide:

- symbol search
- file search
- path traversal
- dependency views
- impact views
- state-flow exploration
- related-node ranking

### 5. Memory and context inspector

Must provide:

- what was persisted
- what was retrieved
- why it was retrieved
- current context budget
- compression decisions
- stale memory warnings

### 6. Model control surface

Must provide:

- list local models
- show model capabilities
- show effective context budget
- show latency/throughput
- pick active model policy
- control fast/balanced/deep execution mode

### 7. Tool and skill manager

Must provide:

- installed tools
- installed skills
- enable/disable state
- permission surfaces
- health diagnostics
- version display
- compatibility display

### 8. Installer and environment diagnostics

Must provide:

- dependency readiness
- missing prerequisites
- toolchain readiness
- model runtime readiness
- graph readiness
- repair suggestions
- one-click or guided repair paths where safe

### 9. Benchmark and evaluation surface

Must provide:

- benchmark runs
- regressions
- latency history
- quality history
- comparison across model configurations
- comparison across runtime changes

## UI Accessibility Requirements

The interface must be genuinely usable.

### Required accessibility areas

- keyboard navigation
- visible focus states
- readable contrast
- scalable text
- reduced-motion support
- screen-size adaptation
- understandable status messaging

### Error messaging

The UI must avoid raw internal leakage as the primary user experience.

Requirements:

- show concise user-facing summaries
- allow drill-down into raw logs
- separate diagnosis view from top-level status
- classify failures as environment, policy, runtime, model, or task failures

## Full Feature Accessibility Requirement

The following categories must be operable from the interface:

- project selection
- run creation
- run pausing and resuming
- graph querying
- impact analysis
- model selection
- local context strategy settings
- memory review
- run comparison
- logs
- artifacts
- benchmark launch
- diagnostics
- installer repair
- skill enablement
- tool permission review

The UI may call into the same backend APIs as the CLI, but it must not permanently hide these capabilities behind terminal-only usage.

## Observability Requirements

Every important system action must have an inspectable UI representation.

### Required visible data

- current run stage
- current active model
- current retrieved files or graph nodes
- current tool invocation
- recent decisions
- evidence references
- retry count
- policy blocks
- failure cause
- timing and resource indicators

### Required inspection depth

The UI must support both:

- high-level summaries for normal operation
- deep drill-down for debugging

## Interaction Design Requirements

The product should feel like a systems console, not a chat clone.

### Desired characteristics

- dense but readable
- stateful
- timeline-aware
- graph-aware
- evidence-oriented
- low-friction for expert operators

### Avoid

- chat-only framing
- ambiguous loading states
- excessive hidden panels
- unclear causal relationships
- decorative dashboards with no operational use

## Performance Requirements for the UI

The desktop layer must stay responsive on large workloads.

### Required performance targets

- fast initial shell rendering
- low-latency route switching
- virtualized long journals
- incremental event rendering
- lazy loading of heavy graph views
- bounded memory growth during long sessions

### Design implications

- heavy computation stays out of the renderer
- stream structured updates instead of reloading whole pages
- avoid rerendering large collections unnecessarily

## Failure-State UX Requirements

Failure must be actionable.

### The UI must answer:

- what failed
- where it failed
- whether it is a user issue or system issue
- whether it is recoverable
- what the next best action is

### Example requirement

If backend startup fails because a compiler or toolchain is missing, the top-level status should present:

- a clear failure state
- a one-line explanation
- the affected component
- the suggested recovery
- a link to detailed logs

not only raw stderr output.

## Security and Privilege Boundaries

The UI must never become the hidden execution authority.

Requirements:

- renderer has no raw shell access
- typed preload APIs only
- privileged actions go through backend policy gates
- dangerous actions are attributable and logged

## Final Requirement

A user should be able to manage the full system from the desktop app with confidence, clarity, and low friction, even on large projects and even during failure or recovery scenarios.
