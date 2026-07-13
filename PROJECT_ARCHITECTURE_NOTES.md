# Project Architecture Notes

## Core Product Direction

Goal: build a local-first, very fast, modular agent runtime that can handle real project work, especially software and automation tasks, while improving on common agent weaknesses.

Principles:
- Local-first execution
- Modular and replaceable components
- Explicit state instead of fragile hidden context
- Verification before trust
- Fast path for simple tasks, deeper path for hard tasks
- Architecture that improves smaller local models instead of assuming frontier-model behavior

## Known Weaknesses To Design Around

These are not side notes. The runtime should be built to compensate for them.

### Model-level weaknesses

- Small local models lose coherence on long-horizon tasks
- Tool selection and tool arguments are less reliable than strong cloud models
- Long context degrades focus and retrieval quality
- They often fail to notice their own mistakes early
- Open-ended planning quality is weaker under ambiguity
- They are more sensitive to prompt shape and state noise

### Agent-system weaknesses

- Context poisoning over long sessions
- Hidden state and unclear reasoning traces
- Retry loops without learning
- Weak grounding between code, tools, and memory
- Bad tool contracts causing unnecessary reasoning load
- No clear escalation path when a task is above the active model's ability

## Design Implications

- Use explicit run state, task state, and artifact state
- Keep prompts narrow and task-bounded
- Prefer structured tool schemas and constrained outputs
- Add validators, checkers, and replayable logs
- Split work into stages instead of one giant agent loop
- Support model escalation by difficulty, not by default

## Proposed Runtime Layers

### 1. Orchestrator

Responsible for:
- task intake
- routing
- budget control
- retries
- escalation
- stopping conditions

It should behave like a deterministic controller, not like another chat agent.

### 2. Worker Models

Suggested separation:
- fast worker: small local model for routing, extraction, summaries, simple code lookup, simple tool use
- main worker: stronger local model for execution and synthesis
- verifier: separate pass or separate model for checking claims, diffs, tests, and tool outputs
- escalator: optional larger local model for tasks that exceed the main worker

### 3. Tool Runtime

Tools should be:
- typed
- narrow
- observable
- replayable
- cheap to call

The model should never search the whole world if a scoped retrieval tool can answer first.

### 4. Memory and State

Separate:
- session state
- project state
- task state
- artifact state
- graph state

Do not let all of this live only inside prompt history.

## Language Strategy

The language split should be intentional.

### Rust

Owns:
- runtime core
- graph engine
- cache layers
- execution control
- performance-critical indexing and traversal
- security-sensitive operations

### Python

Owns:
- typed control-plane client SDK
- automation scripts
- eval harnesses
- offline analysis tools
- experiment runners
- operator tooling where iteration speed matters more than raw execution speed

### Electron + TypeScript

Owns:
- desktop operator interface
- visualization
- workflow control
- human-facing control plane

Rule:
- Python should never become the hidden execution core.
- Rust remains the source of truth for runtime behavior.
- Python is allowed to move fast around the core.

## Graph Layer Evaluation

Short answer: yes, a graph layer is worth building.

Not as a fancy visualization feature first, but as a retrieval and reasoning substrate for large projects.

### Why it makes sense

In large codebases, the real problem is usually not generation. It is navigation:
- what exists
- what calls what
- what depends on what
- what state changes where
- what this file can break
- where a concept appears across layers

Small local models are especially weak at reconstructing this on demand from raw files. A graph reduces search cost and cuts reasoning noise.

### What the graph should represent

#### Static code entities

- repositories
- packages/modules
- files
- classes/structs
- functions/methods
- interfaces/types
- constants/enums
- variables
- imports/exports

#### Dynamic or semantic entities

- state containers
- config values
- environment variables
- events/messages
- API routes
- database tables and queries
- caches
- background jobs
- feature flags

#### Relationship types

- calls
- imports
- defines
- reads
- writes
- mutates
- emits
- subscribes_to
- instantiates
- extends/implements
- depends_on
- routes_to
- persists_to
- derived_from

### Highest-value graph metrics

Prioritize these first:
- call graph
- import/dependency graph
- read/write edges for important state
- symbol-to-file mapping
- API route to handler to service to data-store path
- config/env usage graph
- blast radius / impact radius

Second wave:
- variable mutation lineage
- cross-layer feature graph
- test coverage to symbol graph
- runtime trace overlays
- confidence scores on inferred edges

## Important Constraint

Do not try to model every local variable and every token from day one.

That becomes noisy, slow, and expensive. Start with a layered graph:

### Layer A: high-signal structural graph

- file
- module
- symbol
- import
- call
- type

This should be deterministic and cheap.

### Layer B: state and behavior graph

- reads
- writes
- mutations
- event flows
- request flows

This can be partly inferred and should carry confidence metadata.

### Layer C: runtime evidence graph

- actual execution traces
- test results
- stack traces
- profiler spans
- observed state transitions

This should be optional but very valuable for debugging and refactors.

## How the Agent Should Use the Graph

The graph should not just exist. It should drive workflow.

Use cases:
- "Find where this capability lives"
- "Show all paths from route X to database Y"
- "What mutates this state?"
- "If I change this function, what breaks?"
- "Which files matter for this task?"
- "What symbols are closest to this concept?"
- "Which modules are highly coupled?"

The model should retrieve a scoped subgraph first, then read only the relevant files.

## Recommended Queries

The graph runtime should support:
- entity lookup
- path query between two nodes
- neighborhood query
- impact analysis
- state mutation query
- ranked retrieval by task intent
- filtered traversal by edge type

Example:
- path(function:A -> table:orders)
- impact(symbol:AuthService)
- writes(state:session)
- neighborhood(route:/checkout, depth=2)

## Risks

### If you under-build it

- The agent keeps falling back to raw grep and broad file reading
- Small models waste context on navigation
- Refactor safety stays weak

### If you over-build it

- Graph extraction becomes slow and brittle
- Too many inferred edges lower trust
- Update costs hurt local iteration speed
- The graph becomes a product of its own instead of enabling the runtime

## Decision

Graph architecture is a strong fit for this project.

Reason:
- It directly compensates for a major weakness of small local models: large-project navigation and dependency understanding.
- It improves speed, grounding, and change safety.
- It can become the default context substrate for coding tasks.

But it should be built incrementally:
1. structural graph
2. impact analysis
3. state mutation graph
4. runtime evidence overlay

## Immediate Build Priorities

### Track as core requirements

- small-model weakness compensation
- explicit state machine
- verifier layer
- model escalation layer
- graph-backed code retrieval
- impact analysis
- read/write state tracking
- replayable execution logs

### First graph MVP

- parse project into symbols and files
- build import and call edges
- expose query API
- expose impact API
- expose path API
- rank relevant files before raw reading

## Working Thesis

The project should not aim to make a small local model "magically as smart as the cloud."

It should aim to make a local model systemically efficient:
- less blind search
- less wasted context
- better task decomposition
- better grounding
- better verification

The graph layer is one of the highest-leverage components for that goal.

## Industry Gaps To Target

As of 2026-07-08, the biggest market gap is not raw model intelligence alone.

The gap is dependable execution under real constraints:
- limited latency
- limited memory
- noisy tools
- large codebases
- changing external state
- security risk

This is especially relevant when basing the system on a local 9B-class model.

### 1. Reliability is still weak in real workflows

State-of-the-art agents still fail too often once tasks become multi-step, tool-driven, and rule-constrained.

What this means for us:
- do not trust single-shot outputs
- build retry with state awareness, not blind repetition
- add end-state validators
- treat consistency as a first-class metric

### 2. Persistence and deep search are still missing

Current systems are much better at answering than at persistently finding hard-to-find information.

What this means for us:
- build search loops with budget control
- separate exploration from synthesis
- rank evidence before final answer
- keep provenance for every important claim

### 3. Tool use is still brittle

The sector still has weak performance on domain rules, multi-turn API use, and consistent function calling.

What this means for us:
- make tools narrow and strongly typed
- validate tool arguments before execution
- validate tool effects after execution
- track policy compliance explicitly

### 4. Long-horizon execution is too slow

Computer-use agents remain far too slow for practical day-to-day use. Planning and reflection dominate latency.

What this means for us:
- avoid reflective loops by default
- cache intermediate reasoning products
- reduce step count aggressively
- prefer direct symbolic tools over GUI automation whenever possible
- make "fast path vs deep path" a core runtime feature

### 5. Security architecture is behind capability growth

Prompt injection, indirect prompt injection, and agent hijacking are still fundamental problems.

What this means for us:
- separate trusted instructions from untrusted data
- isolate tool permissions
- treat every web page, file, and tool output as untrusted
- add permission boundaries and action confirmation policies
- log provenance of instructions and actions

### 6. Hallucination is now an evaluation design problem too

The sector still rewards guessing too much. Models often answer confidently when they should express uncertainty.

What this means for us:
- support abstain / unsure outputs
- reward calibrated uncertainty in evals
- require evidence-backed answers for critical tasks
- split "answer" from "confidence" from "evidence"

### 7. Memory is still mostly fake memory

Many systems market "memory" but really store loose summaries or chat history. That is not enough for reliable project work.

What this means for us:
- use explicit state, not implicit memory
- store decisions, artifacts, evidence, and constraints separately
- make memory queryable and versioned
- connect memory to the graph layer

### 8. Large-project code understanding is still under-built

Most agents are still too dependent on raw grep, wide context dumps, or shallow embeddings.

What this means for us:
- graph-backed retrieval should be core, not optional
- use structural and state-aware code navigation
- surface dependency paths and mutation paths directly
- optimize for impact analysis before code generation

### 9. Observability and replay are missing

Many AI systems still cannot answer basic operational questions:
- what did the model read
- why did it choose this tool
- which step introduced the error
- what changed in external state

What this means for us:
- event log everything important
- make runs replayable
- persist intermediate artifacts
- compare runs across prompts, models, and tool policies

### 10. Local-first developer experience is still fragmented

Open models are improving quickly, but local-first orchestration is still scattered across model runners, tool layers, vector stores, eval scripts, and custom glue.

What this means for us:
- one runtime
- one state model
- one graph substrate
- one eval harness
- pluggable model backends

## Strategic Conclusion For 9B Local Base

Using a 9B local model is reasonable if the product is designed around compensation, not denial.

The system should assume:
- the model is fast enough
- the model is good enough for bounded subtasks
- the model is not trustworthy enough for unchecked autonomy
- the model needs structure to compete with stronger cloud systems

So the opportunity is not:
- "make a 9B think like a frontier cloud model"

The opportunity is:
- "make a 9B operate inside a runtime that removes as much unnecessary thinking as possible"

## High-Priority Product Bets

These are the most promising places to differentiate:

### A. Graph-native coding and project navigation

Why:
- directly offsets small-model weakness
- high practical value
- local privacy-friendly
- strong leverage for software workflows

### B. Verifier-first agent runtime

Why:
- reliability gap is still large across the industry
- easier to trust local models when outputs are checked
- good fit for code, automation, and document workflows

### C. Budget-aware execution

Why:
- current agents overspend latency on reflection
- local models need adaptive depth more than raw scale
- small models benefit greatly from controlled escalation

### D. Secure local action layer

Why:
- tool and action security is still immature industry-wide
- local deployment raises the value of trustworthy autonomy
- permission-aware execution can be a product advantage

### E. Real memory and replay

Why:
- most systems still fake continuity
- project work needs durable state
- replay makes debugging and evaluation tractable

## Recommended Focus Order

1. graph-native retrieval and impact analysis
2. state machine and run logging
3. verifier and end-state checking
4. budget-aware planner / executor split
5. secure tool sandbox and trust boundaries
6. durable memory connected to graph and artifacts

## Current Evidence References

These references informed the gaps above:
- BrowseComp (OpenAI, 2025-04-10): browsing agents still need persistence, depth, and creative search.
- τ-bench (arXiv:2406.12045, 2024): even top function-calling agents were below 50% on many realistic tool-and-rule tasks and inconsistent across runs.
- OSWorld-Human (arXiv:2506.16042, 2025): leading computer-use agents still take far too many steps and spend most latency in planning and reflection.
- NIST agent hijacking blog (2025): current agents remain vulnerable to indirect prompt injection and agent hijacking.
- OWASP LLM01:2025 Prompt Injection: RAG and fine-tuning do not fully mitigate prompt injection.
- OpenAI "Why language models hallucinate" (2025): evaluation systems often reward guessing instead of calibrated uncertainty.

## Desktop Control Plane Direction

The project should also have a professional desktop control plane.

This is not a side utility.
It should be treated as the main operator surface for understanding, controlling, and debugging the runtime.

Target shape:
- Electron + TypeScript desktop app
- modular frontend architecture
- strict IPC boundaries
- Rust runtime remains the execution core
- desktop app acts as control plane, inspector, and workflow shell

## Why The Desktop App Matters

The runtime will become too complex to manage well through CLI alone.

The desktop app should solve these problems:
- observe the full lifecycle of runs
- inspect graph structure and impact paths
- watch model behavior and latency
- compare runs and artifacts
- manage tools, models, policies, and settings
- make debugging and operator workflows significantly faster

This should feel closer to a systems console than a chat wrapper.

## Architectural Position

Use a split architecture:

### Native core

Rust owns:
- runtime orchestration
- graph building and querying
- run journals
- cache layers
- model adapter execution
- security policy enforcement
- tool execution
- performance-sensitive indexing and retrieval

### Desktop shell

Electron owns:
- operator UX
- windowing
- local app shell
- navigation
- visualization
- session browsing
- settings management
- workflow composition UI

### Boundary

The app should talk to the Rust core through a narrow, versioned API boundary.

Preferred options:
1. local HTTP + SSE / WebSocket
2. stdio child process RPC
3. native Node binding later only if profiling proves it necessary

Default recommendation:
- start with local HTTP + streaming events
- keep protocol explicit and language-neutral

## Control Plane Modules

The Electron app should be modular from day one.

### 1. App Shell

Responsibilities:
- workspace selection
- navigation
- window state
- layout persistence
- theme and accessibility settings

### 2. Runtime Console

Responsibilities:
- run start / stop / retry
- active task view
- live stage transitions
- budget and latency indicators
- current model/backend visibility

### 3. Graph Explorer

Responsibilities:
- symbol search
- package/module browsing
- impact visualization
- path tracing
- dependency drill-down
- state mutation views later

### 4. Run Journal Viewer

Responsibilities:
- inspect run events
- compare runs
- show artifacts
- show timings
- show cache hits/misses
- show error provenance

### 5. Model Control Surface

Responsibilities:
- choose active local model
- see backend health
- see token / latency / throughput stats
- switch fast/balanced/deep behavior
- control keep-alive and resource policy

### 6. Tool and Policy Manager

Responsibilities:
- list tools
- show permissions
- show blocked actions
- adjust approval policy
- inspect tool execution logs

### 7. Workspace and Config Manager

Responsibilities:
- manage `everything.toml`
- expose runtime config safely
- manage cache locations
- manage graph settings
- manage model settings

### 8. Eval and Benchmark Surface

Responsibilities:
- run benchmark suites
- compare runtime changes
- compare model configs
- surface regressions
- track quality vs latency vs cost

## UI Principles

The app should not mimic generic chat products.

Desired UX qualities:
- dense but readable
- operator-first
- keyboard-centric
- graph-aware
- timeline-aware
- low-latency
- clear provenance everywhere

Avoid:
- chat-only UX
- hidden system state
- vague loading states
- giant unstructured logs
- decorative dashboards without operational value

## Technical App Constraints

### Electron process model

- main process should remain thin
- renderer should never hold privileged execution authority
- all sensitive operations go through preload + typed IPC
- no direct shell access from renderer

### IPC rules

- typed request/response contracts
- streaming events for long tasks
- no arbitrary eval-style bridges
- request IDs and correlation IDs required
- cancellation support required

### State management

Use a predictable client state model.

Recommended split:
- server state: TanStack Query style cache
- UI state: lightweight local store
- event stream store: append-only run/event layer

### Rendering strategy

- virtualize long lists and journals
- lazy-load graph-heavy views
- avoid full graph rerenders on every event
- incremental update patches preferred over full snapshots

### Error handling

- user-safe error messages in UI
- structured error objects underneath
- retry classification: transient vs deterministic vs policy-blocked

## Required Backend Surfaces For The App

The Rust core should expose explicit endpoints or commands for:

### Runtime

- `doctor`
- `start_run`
- `cancel_run`
- `retry_run`
- `get_run`
- `list_runs`
- `stream_run_events`

### Graph

- `graph_summary`
- `graph_query`
- `graph_impact`
- `graph_path`
- `graph_neighbors`

### Models

- `list_models`
- `model_health`
- `set_active_model`
- `run_inference_test`

### Config

- `get_settings`
- `update_settings`
- `validate_settings`

### Metrics

- `get_bootstrap_metrics`
- `get_cache_metrics`
- `get_latency_metrics`
- `get_benchmark_history`

## Security Requirements For The App

This app will control local execution, so security cannot be casual.

Required:
- `contextIsolation: true`
- `sandbox: true` where compatible
- `nodeIntegration: false`
- strict preload APIs
- CSP by default
- signed builds later
- no renderer-originated raw shell commands
- no raw file writes without policy path

All user-triggered dangerous actions should be:
- explicit
- attributable
- logged
- replayable

## Performance Requirements For The App

The desktop layer must not become the slow part of the product.

Track and optimize:
- cold start time
- time to first usable screen
- graph view load time
- run event rendering latency
- memory usage over long sessions
- renderer FPS on large journals and graph views
- IPC round-trip latency

Guidelines:
- push heavy work to Rust
- keep Electron mostly orchestration + visualization
- cache view models
- batch UI updates during event storms

## Professional Delivery Requirements

This should be built like a real product, not a demo shell.

Needed over time:
- installers
- auto-update strategy
- crash reporting
- structured app logs
- telemetry that can be disabled
- workspace migration logic
- config versioning
- multi-workspace support
- release channel strategy

## Recommended Build Order For The App

1. control-plane API contract in Rust
2. Electron shell + preload IPC
3. runtime console and run journal
4. graph explorer
5. model control surface
6. config manager
7. eval/benchmark surface

## Working Thesis For The App

The Electron app should become the place where an operator can:
- understand what the system knows
- see what it is doing
- control what it is allowed to do
- inspect why it succeeded or failed
- improve the runtime based on evidence

That is the right product shape.

Not "AI chat app."
Not "developer toy."

A serious local AI systems console.
