# Contributing

## Before changing code

1. Read the root `AGENTS.md` and the nearest nested `AGENTS.md`, when present.
2. Keep Rust as the execution and durable-state source of truth.
3. Put external effects behind typed adapter traits.
4. Persist state before long or failure-prone work.
5. Do not let model-editable content change safety policy or irreversible-action rules.

## Development workflow

Create focused changes with tests near the owning crate or package. Run the complete local verification set before opening a pull request:

```bash
./scripts/verify.sh
```

On Windows PowerShell:

```powershell
./scripts/verify.ps1
```

At minimum, Rust changes require formatting, Clippy with warnings denied, and workspace tests. Electron changes require typecheck and production build. Python SDK changes require pytest and a wheel/sdist build.

## Contracts

Versioned API and persisted-state contracts must remain backward-compatible unless a migration and explicit version change are included. Keep DTO ownership in `everything-domain`; control-plane clients should mirror rather than redefine runtime authority.

## Generated content

Do not commit `target`, `node_modules`, Electron `out`, `.everything`, Python caches, SQLite runtime files, or generated graph output. Use `scripts/package_source.py` when preparing a source archive.
