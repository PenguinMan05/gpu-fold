# Implementation status against v1.1

This table distinguishes implemented source from runtime verification. **Every Rust entry still needs compilation and execution**, because the creation environment lacked Rust and blocked all compiler/package downloads. Source review is not a substitute for those gates.

| Design area | In this source | Outstanding |
|---|---|---|
| Native UI + independent headless engine | egui/eframe desktop crate; library engine; CLI crate | Native rendering, keyboard and screen-reader review |
| Scope | Public Rust/Cargo repositories; specific issue primary; findings-only sweep | Other ecosystems/platforms |
| Pin/preflight | GitHub commit pin; hostile archive/path rejection; flags; policies; recent issues/PRs | Live GitHub verification; exhaustive semantic duplicate search |
| Model boundary | Local-only endpoint adapter; structured proposals; no model shell/tools | Embedded Candle/mistral.rs loading, streaming tokens, model selection spike |
| GPU | External local runtime schedules inference; NVIDIA read-only status command | Reference RTX 4070 tests, memory planning and model quality comparisons |
| Retrieval | Bounded file map; deterministic keyword ranking; numbered source; whole-file cap | tree-sitter symbol outlines and tokenizer-exact budgeting |
| Patch validation | Exact citations, unique replacements, planned .rs files, protected paths, 5-file/150-line caps, existing test-section guard | AST-aware test preservation; detailed rustfmt/hunk policy |
| Local editing | Proposal JSON editor; resets evidence and working copy; reviewed apply via staged worktree | Rich in-line patch editor and broader diff UI |
| State/store | SQLite runs/settings; saved transitions; restart interrupted steps; history/export | Concurrent-run locking and complete replay tooling |
| Tests | Offline rootless Podman; digest pinning; host-side verified registry fetch; read-only mounts; runtime budgets | Hostile escape/cancellation tests, writable isolated source, Windows/macOS mechanisms |
| Evidence | Two baselines/two post-patch runs; literal failure marker; compiler/flaky/truncated-result rejection | Structured libtest output; broader flaky checks and benchmark accuracy |
| MCP | Explicit JSON import; local stdio via rmcp; consent; tool hashing; schema checks; per-call approval | Remote transports/auth, integrated model tool requests, schema/transport resource limits |
| Secrets | OS keyring GitHub credentials; minimal process environments; basic token redaction | Full adversarial leakage tests and comprehensive secret scanning |
| GitHub | Device flow/client-ID setting; fork polling; isolated git config; reviewed new branch; draft PR; no force/merge | OAuth registration, live API spike, recovery reconciliation, customized fork names |
| Etiquette | Human checklist, common prohibition detection, mandatory disclosure, file-review gates, conservative limits | Exhaustive policy interpretation, semantic duplicates, live PR-state tracking |
| Evaluation | Synthetic addition fixture plus Rust regression tests | 30 real bugs/10 controls/5 hostile fixtures and reference-GPU measurable gates |
| Delivery | Apache-2.0 Rust source, build/install/package scripts, design PDF copy | Compiler-verified binary, installable validated package, lockfile/vendored dependencies |

## Deliberate implementation decisions

- External local model service first, instead of adding a massive untested embedded GPU backend. Both application and recommended mistral.rs service are Rust; model weights and inference service must be installed separately.
- No remote model/MCP path by default. Unsupported remote MCP config entries can be imported for inspection but cannot launch/connect.
- Host downloads verified lockfile registry archives, then cargo runs offline. This avoids running a repository's Cargo configuration during a networked fetch.
- App-authored execution copies exclude Cargo config/toolchain selectors and remain read-only. Compatibility is narrower than v1.1's long-term sandbox design, with no unsafe fallback.
- Full-suite success is required for level 3; no advanced lower-evidence publication override is implemented.
- Publication stops when upstream moves, rather than implementing an unverified automatic rebase.
- Known GitHub effects are never automatically repeated after interruption.
- Apache-2.0 application license follows the existing gpu-fold repository license; dependency/model licenses remain their own.

**Completion boundary:** this is an implementation starting point for the first Linux release, not completion of every milestone or evaluation claim in the 20-page design.
