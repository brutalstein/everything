# Product Vision and Non-Negotiables

## Product Mission

Everything must become a local-first AI runtime and operator environment that can perform real engineering, research, automation, and project-control work at a level comparable to or better than the strongest coding-agent products, while remaining fully usable without paid APIs.

The project should not feel like a lightweight assistant wrapper.
It should feel like a serious execution system:

- fast
- persistent
- inspectable
- self-improving
- graph-aware
- operator-friendly
- installable by normal users

## Core Promise

The product promise is:

1. High performance without sacrificing correctness.
2. High quality without requiring cloud dependence.
3. A long-lived runtime that does not collapse after a few interactions.
4. Full control and visibility from a rich desktop interface.
5. One-command onboarding that leaves the system ready to run.
6. Dynamic intelligence that adapts to the actual local model and machine.

## Non-Negotiable Product Outcomes

The following outcomes are mandatory.

### 1. Maximum practical speed

The system must optimize for real-world latency, not only theoretical architecture purity.

Requirements:

- Prefer fast paths for simple tasks.
- Avoid unnecessary multi-agent or multi-stage overhead on easy work.
- Move performance-critical work into Rust.
- Cache aggressively where correctness allows.
- Reuse graph, memory, and artifact state instead of recomputing.
- Reduce token waste through scoped retrieval and compact state passing.

### 2. Maximum practical quality

The system must optimize for reliable task completion, not just plausible output.

Requirements:

- Strong verification layers.
- Evidence-backed answers for important claims.
- Clear distinction between observed facts and inferred reasoning.
- Task completion criteria that are explicit and machine-checkable where possible.
- Recovery behavior instead of silent failure.

### 3. Infinite runtime flow

For this project, "infinite runtime flow" means a runtime that is designed for indefinite operation, resumability, and continuity, not literal unbounded computation without controls.

Requirements:

- The runtime must support long-lived sessions.
- Every run must be resumable after interruption.
- State must survive process restarts where appropriate.
- Background execution should be durable and inspectable.
- Transient failures must not force full session loss.
- Long tasks must checkpoint progress before risky operations.

### 4. Full feature accessibility from the UI

Everything that matters operationally must be available from the desktop interface.

Requirements:

- Runtime control
- model control
- graph navigation
- run inspection
- settings
- logs
- memory inspection
- artifact browsing
- skill management
- installation diagnostics
- benchmarking
- policy visibility

The CLI can remain important, but the UI must never be a second-class shell that hides critical system capabilities.

### 5. Single-script universal installation

A new user should be able to run one installation script and reach a ready-to-operate system state with minimal manual repair.

Requirements:

- detect machine capabilities
- detect missing dependencies
- choose the best compatible setup path
- install only what is needed
- avoid paid dependencies
- bootstrap local models and runtime pieces intelligently
- leave the system in a runnable state, not a half-installed state

### 6. Dynamic context adaptation

The runtime must adapt its context strategy to the actual local model in use.

Requirements:

- detect model context window
- estimate safe working context
- scale retrieval depth to model quality and context size
- compress or partition state when the model is weak
- increase verification when the active model is weaker
- exploit larger models more aggressively without assuming they are always present

## Product Positioning

Everything should aim to become:

- a local AI operating runtime
- a codebase intelligence engine
- an execution control plane
- a graph-native project mastery system
- a fully inspectable coding and automation environment

It should not be positioned as:

- a thin chat UI
- a prompt toy
- a cloud-dependent wrapper
- a demo-only local agent

## Quality Bar

The quality bar is not "works on the developer machine."
The quality bar is:

- installable by others
- understandable by others
- recoverable under failure
- measurable under load
- maintainable over time

## Strategic Product Thesis

Strong cloud agents often win through model strength.
Everything should win through runtime strength.

That means:

- better state discipline
- better retrieval discipline
- better large-project navigation
- better operator control
- better verification
- better persistence
- better packaging

The system should be designed so that a free local stack can produce serious results through superior runtime engineering.

## Mandatory Design Principles

### Local-first by default

Core functionality must work without paid services.

### Performance-first, but evidence-based

Optimizations should be measured, not imagined.

### Graph-native repository understanding

Large-project mastery must come from structural understanding, not only grep and embeddings.

### Verifier-first reliability

Unchecked generation is never enough for serious tasks.

### Explicit state over hidden prompt memory

State should be persisted, queryable, and inspectable.

### Operator-grade transparency

The user should be able to see what the system knows, why it acted, what failed, and what changed.

### Modular capability growth

The project must be able to gain new skills, tools, and internal abilities without destabilizing the core runtime.

## High-Level Success Criteria

The product is succeeding when:

- it can be installed by non-authors with one guided command
- it remains responsive on large real projects
- it handles long-running work without losing state
- it exposes nearly all important actions through the desktop shell
- it adapts behavior to weak and strong local models
- it can perform most day-to-day coding-agent workflows without requiring paid APIs
- it scales from simple tasks to very large repositories without collapsing into broad context dumping
