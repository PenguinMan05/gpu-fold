# GPUFold

GPUFold scans a public GitHub repository for open, unassigned issues and uses a local model you choose to prepare repair proposals. It works through selected tickets sequentially, saving a separate reviewable run for each issue.

The queue is the first local workflow toward the peer-to-peer GPU swarm described in the [GPUFold concept](docs/GPUFOLD_CONCEPT.md). This build runs on one machine; it does not yet connect requester and volunteer nodes over a peer network.

## Get started

Build on Linux with Rust 1.90 or newer:

```bash
bash scripts/build-linux.sh --install
```

The desktop app appears as **GPUFold**. The existing command-line binary is `rust-repair` for compatibility.

1. In **Model & settings**, choose your local model directory or GGUF file and installed inference runtime, or enter an existing local OpenAI-compatible endpoint. Choose a GPU-capable runtime for GPU inference; CPU mode is available where needed. GPUFold does not silently download model weights.
2. Prepare the rootless Podman sandbox from **Home → Prepare test sandbox**, or run `bash scripts/build-sandbox.sh --approve-image-download` in a terminal. The base image is downloaded only after that explicit approval.
3. In **Repository**, enter a public GitHub URL and select **Scan for unattended tickets**. GPUFold considers open issues with no assignees unattended; pull requests are excluded. Issue discovery does not require a GitHub token.
4. Select up to 10 tickets and choose **Generate proposals**. GPUFold rechecks that each issue is still open and unassigned, pins the repository commit, then sends bounded public source context to your configured local model.
5. Review each result in **Queue**. Approve its plan, review the changed files, apply the patch locally, and run the isolated baseline and verification steps. Queue processing does not run repository scripts, apply code, or publish anything.
6. After level-3 test evidence, you can separately sign in and explicitly approve a draft pull request to your fork. GPUFold never merges a pull request or pushes to the upstream repository.

## What works in this build

- Linux desktop and CLI, persistent run history, cancellation, and per-ticket review.
- Public GitHub issue scanning with pagination, pull-request exclusion, unassigned filtering, and a queue of up to 10 tickets per batch.
- Sequential inference using the configured local runtime/model or compatible local endpoint.
- Bounded Rust proposal validation and an isolated, offline Cargo test sandbox.
- Explicit review and verification before publishing a draft pull request.

The current repair engine supports **Rust repositories with a committed `Cargo.lock` and crates.io dependencies**. It does not yet implement the volunteer worker network, signed swarm manifests, direct peer transport, peer discovery/DHT, or GPU resource sharing in the concept document. Actual inference speed and model quality depend on the local model, runtime, and hardware; the synthetic fixture is not a real-world repair benchmark.

## Requirements and limits

- Linux x86-64 desktop, Git, rootless Podman, and a Secret Service keyring for optional GitHub publishing.
- An installed local inference runtime and local model assets, unless you already run a compatible local endpoint.
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
