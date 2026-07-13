# Everything Requirements Index

This directory contains the high-detail English product requirements for the next phase of the Everything project.

These documents are intentionally written as design-driving requirements, not as vague vision notes. They should be treated as the source package for architecture decisions, roadmap planning, acceptance criteria, and future implementation breakdowns.

## Document Set

- `PRODUCT_VISION_AND_NON_NEGOTIABLES.md`
  Defines the product vision, the non-negotiable quality bar, the meaning of "infinite runtime flow", and the core product constraints.

- `RUNTIME_AND_INTELLIGENCE_REQUIREMENTS.md`
  Defines runtime behavior, graph-backed project mastery, local-model orchestration, dynamic context management, verification, memory, and large-project handling.

- `OPERATOR_INTERFACE_REQUIREMENTS.md`
  Defines the Electron operator surface, full feature accessibility from the UI, observability, run control, graph navigation, and advanced workflow management.

- `UNIVERSAL_INSTALLATION_AND_DISTRIBUTION.md`
  Defines the one-script installation experience, cross-machine readiness, packaging, bootstrapping, environment detection, and zero-friction first run.

- `CAPABILITY_PARITY_AND_SKILLS_STRATEGY.md`
  Defines the target capability parity with Codex and Claude Code, built-in skill architecture, tool strategy, fully free ecosystem expectations, and extensibility requirements.

## Intended Usage

Use this document set when making decisions about:

- architecture
- runtime boundaries
- UI scope
- installer and packaging flow
- local model support
- memory and context systems
- graph and codebase intelligence
- skill/plugin/tool integration
- benchmarks and acceptance gates

## Interpretation Rule

If there is a conflict between convenience and these documents, the documents win unless they are explicitly revised.

If a requirement seems too expensive, it should be decomposed into phases, not silently weakened.
