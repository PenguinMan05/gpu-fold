# Design v1.1 acceptance record

The supplied design remains the specification. A compiled application and a synthetic fixture do not establish all its quality gates.

| Design section | Implementation and verification |
|---|---|
| 1–3: local app, issue and sweep modes | Rust eframe desktop, anonymous public-repository preflight, bounded findings-only sweep, reviewed issue patches. Native launch is a CI gate. |
| 4: engine, persisted state and budgets | UI-independent library and CLI, SQLite transitions, per-run file locks, cancellation, restart recovery, three proposal attempts, ten MCP results. Regression tests run locally and in CI. |
| 5: local AI | Literal-loopback provider, streaming JSON output, managed installed mistralrs process, explicit load/unload and CPU selection, tokenizer budgeting, deterministic request parameters and cancellation. Adapter tests use a mock local server. Actual CUDA/model performance requires a model and GPU. |
| 5: source and patch validity | Rust tree-sitter outlines, bounded retrieval, verbatim citations, unique replacements, five-file/150-line cap, syntax and existing-test preservation, source integrity checks. Regression tests pass. |
| 6: MCP | Optional Desktop JSON import; approved local server/definition hashes; per-run tool allowlist; model-requested tool review; argument-root checks; output delimiters; wire limits and cancellation. Mock MCP tests reject changed definitions and oversized messages. Remote MCP is disabled. Local server privileges are explicitly disclosed; argument checks cannot sandbox arbitrary server internals. |
| 7: sandbox | Rootless Podman; immutable image ID; read-only snapshot and vendor mounts; a writable copy entirely in bounded tmpfs; network/device/capability/resource restrictions; checksummed registry fetch and offline tests. CI's known-bug fixture reached level 3 and its initial isolation probe passed. A revised stop-container-first cancellation probe passed in CI run 38465895689. |
| 8: Git | Pinned bare cache, isolated Git configuration, hooks/LFS/diff helpers disabled, export attributes overridden to preserve original blobs, path and archive validation, app-managed worktree and one fork commit. |
| 9: publishing | Keyring credentials, configurable device flow or token, fork reuse/polling, new branch, explicit draft-PR confirmation, mandatory disclosure and reviewed-file/etiquette/evidence gates, daily and open-PR limits. No merge, force-push or upstream push. Live OAuth/fork/PR testing has not been performed with the app. |
| 10: benchmark | Synthetic fixture and adversarial protocol/process/sandbox checks exist. The specified 30 real bugs, 10 controls, reference-GPU measurements and five-person comprehension study are not complete. No fix-rate or model-quality claim is made. |
| 11: delivery | Linux first; Rust desktop/library/CLI; committed lockfile; reproducible build commands; CI builds binaries and a Debian package only after its gates pass. Windows/macOS runners and accessibility review remain outstanding. |

## Conservative behavior

- Only level-3 changes may be published in this build. The advanced lower-evidence override is not implemented.
- An upstream change stops publication and requires a fresh reviewed/tested run; automatic rebase is not implemented.
- Git/custom registries are rejected; source-provided Cargo configuration and toolchain selectors are shown in preflight and excluded from execution copies.
- Existing tests remain byte-for-byte unchanged. Formatting-only replacements and malformed Rust are rejected rather than automatically running repository-provided tooling.
- No models or inference runtime are silently downloaded. Select complete local assets and an installed Rust inference runtime, or connect an existing compatible local provider.

These limits are visible instead of being represented as completed design milestones.
