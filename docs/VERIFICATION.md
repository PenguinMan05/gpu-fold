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

## Final Linux gates

Final source commit `6194db2925807cd24efa8cb81680ef133c867852` passed [CI run 38468404370](https://github.com/PenguinMan05/gpu-fold/actions/runs/38468404370): 25 regression tests, workspace check, release build, sandbox probe, level-3 fixture, live public GitHub repository pinning/preflight, native launch/screenshot, Debian package creation and actual package installation. Cancellation and container cleanup completed in 3,209 ms.

The Linux x86-64 installer requires GLIBC 2.39 or newer (Ubuntu 24.04 / Mint 22 generation). The inference runtime and complete local model remain separate prerequisites. The supplied synthetic demo does not require a model, but its test execution still needs the explicitly prepared rootless sandbox image.

Artifact `linux-build` (ID 11850781377) ZIP SHA-256: `1d244531f0305e28db3d3fc8ae1f517c506a65427257f3c2fa6746f23184c568`. Downloaded artifact bytes matched this digest; the screenshot, package metadata, public pin and fixture/sandbox records were inspected.

## Still requires evidence

- Actual local coding-model inference and reference RTX 4070 performance/VRAM measurements.
- Live app-driven device-flow/fork/push/draft-PR tests.
- Full real-bug/control/hostile benchmark and accessibility/comprehension study.

Successful CI artifacts include the fixture record, sandbox report, desktop startup log, screenshot, binaries and package. Failed artifacts are diagnostics and are not releases.
