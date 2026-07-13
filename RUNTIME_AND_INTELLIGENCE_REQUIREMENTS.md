# Runtime and Intelligence Requirements

## Purpose

This document defines the runtime, intelligence, context, memory, and project-mastery requirements for Everything.

It is the core spec for making a free local agent runtime behave like a high-end engineering system instead of a fragile prompt loop.

## Runtime Objectives

The runtime must be:

- persistent
- resumable
- fast
- observable
- deterministic where possible
- adaptive where necessary

## Execution Model

The runtime must support multiple execution depths.

### Fast path

Used for:

- short Q&A
- quick file lookup
- simple tool calls
- narrow code edits
- metadata retrieval

Requirements:

- minimal orchestration overhead
- low token usage
- direct retrieval before planning
- bounded latency

### Balanced path

Used for:

- normal coding tasks
- medium-complexity debugging
- scoped refactors
- multi-step repository reasoning

Requirements:

- graph-scoped retrieval
- explicit checkpoints
- verifier pass when the action is meaningful
- run journal persistence

### Deep path

Used for:

- large-project analysis
- high-risk changes
- hard debugging
- architecture synthesis
- repository-wide impact work

Requirements:

- staged planning
- graph traversal plus selective file reading
- stronger validation
- richer provenance
- structured intermediate artifacts

## Infinite Runtime Flow Requirements

The runtime must support continuity over long periods.

### Persistence requirements

- Persist run state before long or failure-prone work.
- Persist user intent, plan state, tool outcomes, and artifacts separately.
- Preserve completed, failed, cancelled, and blocked states.
- Support restart from checkpoints instead of full restart from scratch.

### Recovery requirements

- Recover from tool failures without losing prior progress.
- Distinguish transient failures from deterministic failures.
- Support retry policies with memory of what was already tried.
- Support pause, resume, and audit of long-running work.

### Session continuity requirements

- Session history must not be the only memory substrate.
- Important constraints must be extracted into structured state.
- Long sessions must be compressible without losing control data.

## Local Model Orchestration Requirements

The system must treat model selection as a runtime engineering problem.

### Model classes

The runtime should support:

- very small local models for routing and extraction
- mid-tier local models for normal execution
- stronger local models for difficult synthesis
- verifier-oriented models or passes

### Routing rules

- Easy work should not consume expensive reasoning depth.
- Weak models should be given narrower tasks and stronger validation.
- Stronger available models may be used for synthesis, but never assumed.
- The runtime must degrade gracefully when only weak models exist.

### Free-first requirement

The default core experience must be achievable with:

- local models
- free open-source libraries
- locally runnable infrastructure

Paid providers may be optional extensions, never a core requirement.

## Dynamic Context Management Requirements

This is a first-class requirement.

The runtime must adapt context handling to the active model and task.

### Inputs to context policy

The policy engine should consider:

- model context window
- model quality tier
- task complexity
- repository size
- graph availability
- amount of prior state
- latency budget
- available memory and CPU/GPU resources

### Required behaviors

- Estimate a safe effective context budget, not just the theoretical maximum.
- Choose between direct prompt, graph retrieval, memory retrieval, or staged decomposition.
- Compress prior reasoning when it becomes less important than current evidence.
- Prefer scoped graph subgraphs over wide file dumps.
- Use delta-oriented state passing for long tasks.
- Reduce context size for weaker local models.
- Increase retrieval precision before increasing retrieval volume.

### Failure modes to prevent

- context poisoning
- lost-in-the-middle behavior
- repeated full-history stuffing
- broad repository dumps
- over-retrieval from irrelevant files

## Graph-Native Large Project Mastery Requirements

Everything must be exceptional at very large projects.

This is not optional.

### Required graph capabilities

- file graph
- symbol graph
- import graph
- call graph
- impact graph
- path queries
- state mutation edges where feasible
- cross-layer traversal

### Required graph workflows

- find where a capability lives
- find what mutates a concept
- find the blast radius of a change
- find route-to-handler-to-state-to-storage paths
- rank the most relevant files before reading them
- explain subsystem relationships

### Required graph properties

- deterministic structural extraction first
- inferred edges clearly marked
- cheap incremental updates
- evidence-backed traversal
- low-latency query access

### Large repository behavior

On huge repositories, the runtime must:

- avoid whole-repo reading
- prioritize graph-based narrowing
- cluster work by subsystem
- keep task-local working sets
- make tradeoffs explicit when certainty is limited

## Memory System Requirements

The memory system must be real, structured, and queryable.

### Memory classes

- session memory
- workspace memory
- task memory
- artifact memory
- graph memory
- preference memory

### Required properties

- versioned
- inspectable
- editable
- attributable
- retrievable by scope
- separate from raw transcript history

### What must be stored

- decisions
- plans
- constraints
- evidence references
- run outcomes
- failed attempts
- environment discoveries
- repository facts that remain valid

## Verification and Reliability Requirements

The runtime must be designed around verification.

### Required verification layers

- schema validation
- tool argument validation
- tool effect validation
- code/test verification where appropriate
- evidence-backed claim checks
- confidence reporting

### Reliability principles

- never silently invent missing facts
- separate observation from judgment
- allow abstention or uncertainty
- prefer blocked over fabricated success
- log why a failure happened

### Acceptance behavior

For important work, the runtime should not mark success unless:

- success criteria are actually met
- final state is observable
- critical side effects are validated

## Built-In Runtime Abilities

The runtime should support internal abilities that are not just external plugins.

Examples:

- graph-backed code search
- code impact analysis
- context budgeting
- prompt compression
- run journaling
- failure classification
- tool policy enforcement
- benchmark comparison
- repository health inspection

These should be part of the runtime product, not left only to ad hoc scripts.

## Performance Requirements

### Measured performance areas

- cold start time
- time to first useful answer
- graph query latency
- retrieval latency
- planning overhead
- tool execution latency
- verification cost
- long-session memory growth

### Runtime optimization principles

- cache what is stable
- stream long results
- update incrementally
- avoid expensive reasoning when symbolic retrieval can answer first
- reduce duplicate tool calls

## Constraints for Implementation

### Rust responsibilities

- execution core
- graph
- caches
- orchestration state
- verification surfaces
- policy enforcement
- performance-critical analysis

### Python responsibilities

- SDK
- eval harnesses
- experiments
- convenience tooling
- operator automation

### Desktop responsibilities

- control plane
- visibility
- workflow composition
- interactive inspection

## Final Requirement

Everything must behave like a serious local execution runtime that compensates for model weakness with:

- graph structure
- explicit state
- adaptive context control
- verifiers
- persistent memory
- measurable runtime engineering
