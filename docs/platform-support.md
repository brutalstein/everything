# Platform Support

## Tier 1

- Windows 11 x86_64 with the stable MSVC Rust toolchain
- Ubuntu 24.04 x86_64
- macOS 14+ on Apple Silicon

Tier 1 means the GitHub CI/release matrix compiles, formats, lints, and tests the Rust workspace; tests and packages the Python SDK; type-checks/builds/audits Electron; parses platform installers; and verifies deterministic source packaging.

Smart setup installs a per-user background service using Scheduled Tasks on Windows, systemd user services on Linux, and LaunchAgents on macOS. Startup-folder/XDG fallbacks are used only when the primary user-service manager is unavailable.

## Tier 2

- Windows x86_64 GNU Rust target
- Other recent Debian, Fedora, Arch, and openSUSE distributions
- macOS x86_64

Tier 2 platforms are community-supported and may require local linker or system-library configuration. Machine-specific settings belong in untracked local files, never committed `.cargo/config.toml`.

Linux connector secrets require a usable Secret Service/libsecret session. Headless Linux without a session keyring can use explicitly supplied environment-backed secrets, but the normal desktop setup should use the OS vault. Bubblewrap is installed when supported; otherwise process execution remains strict-allowlist-only.

## Packaging status

The repository publishes a deterministic source archive, SHA-256 checksum, and provenance attestation. The smart installer builds a user-local desktop/runtime installation from that verified source. Platform-signed native installer packages and notarized store distribution are future release work.

## Not supported

Mobile platforms, browser-only execution, remote multi-user daemon exposure, provider scraping/cookie automation, and architectures without upstream Rust/Node/Electron support are outside the current scope.
