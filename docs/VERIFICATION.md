# Verification — 2026-10-13

## Passed

- Local `cargo check --offline --locked --workspace` with Rust 1.99.0.
- 22 regression tests: unsafe paths/archives, exact citations, patch scope, existing tests, worktree tampering, evidence consistency, secret redaction, publish blockers, local model streaming, cancellation, inherited pipes, MCP changed definitions, wire limits and argument roots.
- GitHub release build of the desktop and CLI.
- CI run 38464736953: rootless Podman known-bug fixture reproduced twice before the production patch, then reached level 3 with two verification passes.
- The same CI run's initial isolation probe: host secret access, inherited environment, network/device/socket exposure and writable-copy checks passed. Its original cancellation cleanup was slow; a revised stop-container-first implementation passed the stricter nine-second probe in CI run 38465895689.

## Found and fixed

- Missing rmcp feature dependencies and obsolete TOML serialization compatibility.
- Podman image IDs may be returned as bare 64-character hex; normalized and validated.
- Native X11 launch requires libxkbcommon-x11-0; added to CI and Debian dependencies.
- Package dependencies now use the binary's measured minimum GLIBC version.

## Still requires evidence

- Final Debian package installation checks.
- The native desktop launch and screenshot passed in CI run 38465895689; its package step found a missing ripgrep command, now removed from the packaging script.
- Actual local coding-model inference and reference RTX 4070 performance/VRAM measurements.
- Live app-driven device-flow/fork/push/draft-PR tests.
- Full real-bug/control/hostile benchmark and accessibility/comprehension study.

Successful CI artifacts include the fixture record, sandbox report, desktop startup log, screenshot, binaries and package. Failed artifacts are diagnostics and are not releases.
