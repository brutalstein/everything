## What changed

Describe the user-visible behavior and the smallest relevant implementation boundary.

## Safety and permissions

- [ ] No new secret, token, private repository content, or external-account payload is logged.
- [ ] New workspace mutations use the typed tool runtime and verifier.
- [ ] New connector actions declare exact scopes, risk, schema, dry-run, and idempotency behavior.
- [ ] Autonomous writes are bounded by an exact policy and budget.

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings`
- [ ] `cargo test --locked --workspace`
- [ ] Electron typecheck, build, and production audit
- [ ] Python tests and package build
- [ ] Installer and MVP smoke checks

Include any intentionally skipped gate and the concrete reason.
