# Universal Installation and Distribution Requirements

## Purpose

This document defines the installation, packaging, bootstrap, and first-run requirements for Everything.

The project must be installable and runnable by normal users on different machines with a single primary command or script.

## Core Installation Promise

The ideal first-run flow is:

1. User downloads or clones the project.
2. User runs one command.
3. The installer detects the machine and environment.
4. The installer provisions what is needed.
5. The system validates itself.
6. The product opens in a ready-to-use state or provides a precise repair path.

## Non-Negotiable Requirement

The installation experience must be smart, adaptive, and self-diagnosing.

It is not acceptable to leave the user with:

- a half-configured environment
- unknown missing dependencies
- silent fallback failures
- vague build-toolchain requirements

## One-Script Installation Requirements

There must be a primary installer entrypoint.

Examples of acceptable shapes:

- `install.ps1`
- `install.sh`
- `everything bootstrap`
- a small launcher that selects the correct platform-specific flow

The exact surface can vary, but the experience must remain one-command oriented.

## Platform Detection Requirements

The installer must detect:

- operating system
- CPU architecture
- available RAM
- GPU presence where relevant
- Rust toolchain availability
- Python availability
- Node availability
- build toolchain availability
- model runtime availability
- network availability
- existing partial installs

## Adaptive Installation Requirements

The installer must choose a reasonable path based on the machine.

Examples:

- If strong GPU support exists, prefer better local model setups.
- If no GPU exists, choose lighter default model profiles.
- If a build toolchain is missing, either install or redirect to prebuilt artifacts.
- If a prebuilt runtime is available, prefer it over forcing a local compile on first run.

## Cross-Machine Readiness Requirements

The project must be designed so that many users can get it running on different machines without manual engineering intervention.

### Required strategies

- preflight checks
- automatic compatibility detection
- repair suggestions
- fallback profiles
- safe defaults
- optional advanced configuration

### First-class support priority

The project should explicitly define its target support tiers, for example:

- Tier 1: common Windows developer and power-user machines
- Tier 2: common Linux machines
- Tier 3: macOS where dependencies allow

The installer should know what tier the machine belongs to and adapt messaging accordingly.

## Zero-Friction First Run Requirements

After installation, the product should not require the user to manually discover hidden steps.

The installer should handle or clearly stage:

- dependency setup
- environment validation
- model runtime setup
- graph readiness
- cache directory creation
- desktop shell readiness
- backend readiness

## Validation Requirements

The installer must run a final validation phase.

### Required checks

- runtime executable availability
- desktop app availability
- model runtime detection
- graph subsystem readiness
- write permissions for required directories
- service startup smoke test
- UI/backend handshake smoke test if feasible

### Validation output

The user must see:

- what passed
- what failed
- what was skipped
- what is optional
- what to do next

## Prebuilt Artifact Requirement

To maximize usability, the project should support prebuilt components where possible.

Especially important:

- backend runtime binaries
- graph-related heavy components
- desktop distributions

Reason:

The first-run experience should not depend on every user having a correct native compiler setup if that can be avoided.

## Repair and Self-Healing Requirements

The installer must not only install.
It should also repair.

### Required repair capabilities

- detect broken or partial installs
- re-run only failed steps
- clear stale caches safely
- reinstall missing internal pieces
- revalidate after repair

## Free Ecosystem Requirement

The installer and distribution path must preserve the project's free-first promise.

Core setup must not require:

- paid API keys
- paid package feeds
- paid cloud runtime services
- commercial-only dependencies

Optional paid integrations can exist later, but the default install path must remain fully functional without them.

## Packaging Requirements

The project should eventually support:

- source install
- operator/developer install
- packaged desktop distribution
- reproducible bootstrap for contributors

### Packaging goals

- minimize setup ambiguity
- minimize machine-specific surprises
- separate core runtime from optional extras
- keep upgrade paths clear

## Smart Model Setup Requirements

Since local models are central, the installer should set intelligent defaults.

### It should be able to:

- detect available model runners
- suggest default local models
- choose lightweight or heavyweight profiles
- configure context settings based on detected models
- avoid overcommitting hardware

## Logging Requirements

Installation must be observable.

The installer should emit:

- human-readable progress
- machine-readable logs
- failure summaries
- environment snapshots where safe

## Acceptance Criteria

The installation system is acceptable only if:

- a new user can run one main command
- the command performs real environment detection
- the command reaches a runnable state or gives a precise recovery path
- the command does not require paid dependencies
- the command avoids unnecessary native compilation when prebuilt paths are available

## Final Requirement

Everything must be easy to adopt.

The project cannot become a world-class local runtime if only its author can install it reliably.
