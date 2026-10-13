# Verification — 2026-10-13

## Passed

- Local `cargo check --offline --locked --workspace` with Rust 1.99.0.
- 25 regression tests: unsafe paths/archives, exact citations, patch scope, existing tests, worktree tampering, evidence consistency, secret redaction, publish blockers, local model streaming, cancellation, inherited pipes, MCP changed definitions, wire limits and argument roots.
- GitHub release build of the desktop and CLI.
- CI run 38464736953: rootless Podman known-bug fixture reproduced twice before the production patch, then reached level 3 with two verification passes.
- The same CI run's initial isolation probe: host secret access, inherited environment, network/device/socket exposure and writable-copy checks passed. Its original cancellation cleanup was slow; a revised stop-container-first implementation passed the stricter nine-second probe in CI run 38465895689.

## Found and fixed

- Missing rmcp feature dependencies and obsolete TOML serialization compatibility.
- Podman image IDs may be returned as bare 64-character hex; normalized and validated.
- Native X11 launch requires libxkbcommon-x11-0; added to CI and Debian dependencies.
- Package dependencies now use the binary's measured minimum GLIBC version.
- Live Git archive import exposed a global PAX header and trailing directory separators. Bounded metadata and normal directory entries are accepted; links, unsafe paths and special files remain rejected. The regression test passes.

## Current Linux build

Final application commit `0745a5d2ce6747bbca29b26aeeab0e07b750f167` passed [CI run 38471619205](https://github.com/PenguinMan05/gpu-fold/actions/runs/38471619205): 28 workspace tests, workspace check, release build, sandbox probe, level-3 fixture, live public GitHub repository pinning/preflight, native launch/screenshot, Debian package creation and actual package installation. The final UI screenshot was inspected for text contrast. Cancellation and container cleanup completed in 3,209 ms.

The workspace's 28 tests include 25 core regressions plus three desktop checks for exact runtime launch approval, installed/source sandbox script discovery, and stale MCP configuration invalidation. The UI review also fixed empty task submission, zero/invalid issue numbers, setup instructions from the installed launcher, and first-run card contrast.

The Linux x86-64 installer requires GLIBC 2.39 or newer (Ubuntu 24.04 / Mint 22 generation). The inference runtime and complete local model remain separate prerequisites. The supplied synthetic demo does not require a model, but its test execution still needs the explicitly prepared rootless sandbox image.

Artifact `linux-build` (ID 11851519723) ZIP SHA-256: `0e2632aa91c5f52751d6c235b06a7cbaa206741ef067c47f5a7bf45a2618b37b`. The downloaded archive passed its checksum and ZIP integrity check; the screenshot, package metadata, public pin and fixture/sandbox records were inspected.

## Still requires evidence

- Actual local coding-model inference and reference RTX 4070 performance/VRAM measurements.
- Live app-driven device-flow/fork/push/draft-PR tests.
- Full real-bug/control/hostile benchmark and accessibility/comprehension study.

Successful CI artifacts include the fixture record, sandbox report, desktop startup log, screenshot, binaries and package. Failed artifacts are diagnostics and are not releases.
