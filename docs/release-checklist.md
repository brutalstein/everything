# Release Checklist

Everything releases are source-first and must be reproducible from a clean checkout.

## Repository setup

- Replace the default GitHub repository in `bootstrap.sh` and `bootstrap.ps1` when publishing under a different owner/name.
- Enable branch protection for `main`: pull request required, force-push disabled, and all CI jobs required.
- Enable Dependabot, secret scanning, push protection, and private vulnerability reporting.
- Do not commit OAuth client secrets, access tokens, refresh tokens, provider test accounts, `.everything/`, or private workspace fixtures.

## Release candidate

1. Use a clean checkout with no generated `target`, `node_modules`, `out`, `dist`, virtualenv, token, or runtime-state directories.
2. Run `scripts/verify.sh` on macOS/Linux or `scripts/verify.ps1` on Windows.
3. Run one live local-model smoke with the intended Ollama model.
4. Exercise OAuth with dedicated test applications and test accounts for every provider changed in the release.
5. Verify the installer twice on the same machine to prove idempotency, then force a controlled failure to prove rollback.
6. Verify background routines while the Electron window is closed, including retry, dead-letter, approval replay, and DST-aware scheduling.
7. Confirm `everything-cli doctor --json` reports every required subsystem and provides actionable remediation for degraded components.

## Publish

- Update every project version together and run `python scripts/check_versions.py --expected <version>`.
- Create a signed or protected tag `v<version>` only after CI is green on Linux, macOS, and Windows.
- Let `.github/workflows/release.yml` build the deterministic source archive, checksum, provenance attestation, and GitHub release.
- Publish release notes that list new permissions/scopes, migrations, known limitations, and the exact native/live tests executed.

## Post-release

- Install from the published GitHub asset using the public bootstrap command on a clean machine.
- Compare the asset checksum with the release checksum.
- Confirm upgrade and rollback from the previous supported version.
- Monitor security alerts and provider API deprecations; revoke or narrow scopes when a capability no longer needs them.
