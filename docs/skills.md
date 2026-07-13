# Everything Skills and Plugins

Everything skills are local, versioned instruction packages. The MVP deliberately supports native runtime workflows and prompt skills instead of arbitrary plugin executables. This keeps startup fast, avoids a second plugin process, and preserves the runtime's permission, context, artifact, and verification guarantees.

## Discovery order

The registry loads skills once into an in-memory cache and refreshes only when requested:

1. Built-in native skills.
2. User skills in `~/.everything/skills` or `$EVERYTHING_HOME/skills`.
3. Workspace skills in `<workspace>/.everything/skills`.

Built-in IDs are reserved. A workspace package overrides a user package with the same ID. Invalid or incompatible packages are isolated and skipped without breaking other skills.

## Package formats

A package is a directory containing either:

- `SKILL.md` with YAML-like front matter, or
- `skill.toml` plus an instruction file, normally `SKILL.md`.

Minimal package:

```text
review-helper/
├── SKILL.md
└── skill.toml
```

See `examples/skills/review-helper` for a complete example.

Required identity fields are `id`, `name`, `version`, and `runtime_api = "v1"`. Supported MVP entrypoints are:

- `prompt:<relative instruction file>`
- `builtin:<native workflow id>`

Instruction paths must stay inside the package. Symlinks are rejected. Packages are limited to 256 files and 4 MiB. Installation uses a temporary directory, backup, and atomic rename.

## Permissions

Supported permissions are:

- `workspace.read`
- `workspace.write`
- `process.execute`
- `network.local`
- `network.external`
- `git.read`
- `git.write`
- `system.install`

The simple task composer shows enabled, compatible read-only skills in **İncele** mode and enabled coding/prompt skills in **Kodla** mode. Coding skills guide proposal generation only; every workspace mutation still passes through an explicit approval step, tool policy, hash guards, verifier evidence, and rollback.

## Manage skills

Desktop: open **Skills**, select a package directory, install it, then enable or disable it.

Rust CLI:

```bash
everything-cli --workspace . skills list
everything-cli --workspace . skills install ./examples/skills/review-helper
printf '%s\n' '{"input":{"objective":"Review the state store","mode":"Balanced"}}' > skill-input.json
everything-cli --workspace . skills run review-helper skill-input.json
everything-cli --workspace . skills disable review-helper
everything-cli --workspace . skills uninstall review-helper
```

Python CLI:

```bash
everything-control skills
everything-control skill-install ./examples/skills/review-helper
# Create an input JSON file containing {"input":{"objective":"Review the state store"}}
everything-control skill-run review-helper ./skill-input.json
```

## Performance model

Skill discovery performs bounded filesystem reads, hashes package content once, and stores enable state in SQLite/WAL. Normal list and execution paths use the cached registry; enabled states are fetched in one query. Skill execution reuses the persistent code graph, model capability profile, context budget, artifact store, and verifier rather than spawning a separate plugin runtime.

## Natural-language coding flow

The desktop **Kodla** mode and the `propose-edit` CLI/API can use an enabled coding or prompt skill as workflow policy. The selected skill never bypasses the proposal/approval boundary. A proposal:

1. retrieves graph and relevant memory context;
2. asks the configured local model for exactly one existing UTF-8 file and complete replacement content;
3. validates the path, size, allowed verification programs, and current content hash;
4. persists a patch artifact and diff with `AwaitingApproval` status;
5. applies only after a second explicit request with `approval_granted = true`;
6. runs deterministic verification and performs hash-controlled rollback when a required check fails.

```bash
everything-cli --workspace . propose-edit --mode balanced --skill scoped-edit "README içindeki hatalı komutu düzelt"
everything-control --workspace . propose-edit --mode balanced --skill scoped-edit "README içindeki hatalı komutu düzelt"
```
