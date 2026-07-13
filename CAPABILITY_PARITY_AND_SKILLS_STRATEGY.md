# Capability Parity and Skills Strategy

## Purpose

This document defines the target capability scope of Everything relative to advanced coding-agent systems such as Codex and Claude Code.

The goal is not superficial feature imitation.
The goal is practical parity or superiority in real workflows, while staying local-first and free-first.

## Capability Target

Everything should aim to perform the same broad classes of work that top coding agents can perform, including:

- repository exploration
- code understanding
- graph-backed navigation
- code editing
- debugging
- test execution
- refactor planning
- architecture review
- documentation generation
- installer/bootstrap repair
- local app testing
- workflow automation
- multi-step engineering execution

## Parity Definition

Parity means:

- the user can ask for the same kinds of work
- the runtime can decompose and execute that work
- the system can inspect evidence and recover from failure
- the system can scale to large repositories

Parity does not require identical UI or identical prompt style.
It requires equivalent or better practical outcomes.

## Free-First Requirement

The core capability layer must be implementable with:

- free open-source libraries
- local runtimes
- local models
- self-hosted or offline-compatible components where possible

The project should treat paid-provider integration as optional augmentation, not baseline infrastructure.

## Built-In Skill Architecture Requirement

Everything should support internal skills as a first-class system capability.

These skills should be:

- versioned
- typed
- inspectable
- composable
- testable
- enableable or disableable

## Skill Categories

The runtime should eventually support built-in or bundled skills for:

- repository investigation
- graph query assistance
- debugging workflows
- UI and UX testing
- benchmark and regression evaluation
- installer diagnostics
- context compression
- context degradation recovery
- memory management
- code review
- documentation generation
- architecture summarization

## Internal Ability vs External Plugin Rule

The highest-leverage abilities should live inside the product.

Examples of internal abilities:

- graph-backed retrieval
- code impact analysis
- run journaling
- context budgeting
- verifier passes
- environment diagnostics

Plugins and add-on skills should extend the system, not replace the core value.

## Tool Strategy Requirements

The runtime must have a strong tool model.

### Required properties

- typed tools
- narrow tool contracts
- explicit permissions
- structured outputs
- replayable invocations
- rich error messages

### Tool priorities

Prefer:

- symbolic tools
- repository tools
- graph tools
- build/test tools
- environment diagnostic tools

Only use GUI or browser-like automation when that is the correct evidence surface.

## Repository Mastery Requirement

To compete with high-end coding agents, Everything must be excellent on very large codebases.

Requirements:

- graph-first navigation
- scoped file reading
- impact analysis
- subsystem localization
- retrieval ranking
- state mutation discovery
- change blast-radius awareness

This is a major differentiation target.

## Multi-Step Engineering Workflow Requirement

Everything must not stop at answering questions.
It must execute long engineering workflows coherently.

Required workflow abilities:

- clarify success criteria
- break work into stages
- persist intermediate state
- validate each stage
- recover after failure
- continue after interruption

## Verification Parity Requirement

Everything must meet or exceed cloud-agent quality by using stronger runtime verification.

Required behaviors:

- test before claiming success where appropriate
- inspect logs and artifacts
- report confidence
- distinguish blocked from solved
- preserve evidence paths

## UI Testing and App Testing Requirement

The system must support real application testing workflows, including:

- desktop app testing
- web UI testing
- CLI testing
- API testing
- regression checks

This can use bundled open-source frameworks and local tooling.

## Extensibility Requirements

The project must be able to grow beyond the initial feature set.

### Required extension surfaces

- internal skills
- external plugins
- new model backends
- new retrieval strategies
- new verifiers
- new adapters
- new graph extractors

### Required extension safety

- explicit enablement
- version compatibility
- diagnostics
- policy boundaries

## Model-Agnostic Capability Layer

The product should be designed so that capabilities are not hardcoded to one vendor.

Requirements:

- provider-agnostic orchestration
- local-first model support
- capability discovery based on actual model traits
- graceful degradation when a model is weak

## Acceptance Criteria for Parity Direction

Everything is on the right path when it can:

- understand and modify real repositories
- work across large codebases without broad context dumping
- inspect and test local applications
- manage long-running tasks with preserved state
- provide operator visibility for all important actions
- do this with a fully free baseline stack

## Strategic Differentiation

Everything should not try to beat frontier products only by raw model intelligence.

It should beat them where runtime engineering matters:

- large-project structure awareness
- persistent execution state
- local control
- inspectable memory
- deterministic tooling
- fully accessible operator surface
- reproducible installation

## Final Requirement

The project should evolve into a serious local engineering runtime that can do the same categories of work as top coding agents, while remaining:

- free
- local-first
- graph-native
- extensible
- operator-controlled
- suitable for very large projects
