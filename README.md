# GPUFold

GPUFold scans a public GitHub repository for open, unassigned issues and uses a local model you choose to prepare repair proposals. It gives selected tickets turns, automatically revisits failed attempts, and saves a separate reviewable run for each issue.

The queue is the first local workflow toward the peer-to-peer GPU swarm described in the [GPUFold concept](docs/GPUFOLD_CONCEPT.md). This build runs on one machine; it does not yet connect requester and volunteer nodes over a peer network.

## Get started

Install the latest `.deb` from [GitHub Releases](https://github.com/PenguinMan05/gpu-fold/releases) and open **GPUFold** from your applications menu (or run `gpufold`). The package targets x86-64 Ubuntu 24.04 / Linux Mint 22 generation systems.

The first launch opens **Setup & settings**:

1. Choose **Local GGUF file**, select your model, and click **Load model**. The installer includes the inference engine; no engine executable, server configuration, or extra model app is needed.
2. GPUFold starts the engine automatically, selects a free local port, and checks when loading finishes. Vulkan GPU acceleration and CPU mode are included. GPU acceleration requires a working compatible GPU driver.
3. Prepare the isolated **Test environment**, or return to this step before verifying a repair.
4. Finish setup, paste a public Rust repository URL in **Issues**, and click **Find open issues**.
5. Select up to 10 issues and click **Start selected issues**. Each result explains the next review step.

LM Studio, Ollama, and other local servers remain optional alternatives. Model weights are supplied by you and are never downloaded automatically. See the [beginner guide](docs/GETTING_STARTED.md). Public issues do not require GitHub sign-in.

To build from source with Rust 1.90+: `bash scripts/build-linux.sh --install`. Existing `rust-repair` CLI and desktop binary names are retained for compatibility.

## What works in this build

- Linux desktop and CLI, persistent run history, cancellation, and per-ticket review.
- Public GitHub issue scanning with pagination, pull-request exclusion, unassigned filtering, and a queue of up to 10 tickets per batch.
- Schema-constrained local proposals, automatic correction retries, a separate AI conversation feed and model summaries, and fair queue turns for unresolved tickets.
- Automatic GGUF token counting and source-context fitting, without a separate tokenizer file.
- Scoped project guidance from AGENTS.md, SKILL.md / skills.md, CLAUDE.md, GEMINI.md, Copilot instructions, and editor rules.
- Built-in read-only file/code search for model-led source investigation, focused revision context, and unique exact citation anchoring.
- Bounded Rust proposal validation and an isolated, offline Cargo test sandbox.
- Explicit review and verification before publishing a draft pull request.

The current repair engine supports **Rust repositories with a committed `Cargo.lock` and crates.io dependencies**. It does not yet implement the volunteer worker network, signed swarm manifests, direct peer transport, peer discovery/DHT, or GPU resource sharing in the concept document. Actual inference speed and model quality depend on the local model, runtime, and hardware; the synthetic fixture is not a real-world repair benchmark.

## Requirements and limits

- Linux x86-64 desktop, Git, rootless Podman, and a Secret Service keyring for optional GitHub publishing.
- A local GGUF model file. The .deb includes its inference engine, libraries, and CPU/Vulkan backends. Optional external model apps can also be used.
- Repository test code runs in a rootless container with no host credentials, network, GPU, or runtime socket. Source is mounted read-only and the test copy lives in bounded container tmpfs.
- MCP support is optional. Imported local MCP servers run with your user privileges; review definitions and allow only tools you trust. Remote MCP is disabled.
- A person reviews and applies each proposal before deciding whether to open a PR.

## Verify and package

```bash
cargo fmt --all --check
cargo test --locked --workspace
cargo check --locked --workspace
bash scripts/verify-fixture.sh
./dist/rust-repair sandbox-probe
bash scripts/package-deb.sh
```

The Debian package targets Ubuntu 24.04 / Mint 22 generation x86-64 systems and records the measured GLIBC minimum. See [implementation status](docs/IMPLEMENTATION_STATUS.md), [design acceptance](docs/DESIGN_ACCEPTANCE.md), and [verification notes](docs/VERIFICATION.md).

GPUFold software is Apache-2.0. Model, runtime, and dependency licenses remain separate.
