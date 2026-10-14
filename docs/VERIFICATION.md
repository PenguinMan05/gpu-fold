# Verification

## Baseline before the ticket-queue refocus

Commit `0745a5d2ce6747bbca29b26aeeab0e07b750f167` passed GitHub Actions run [38471619205](https://github.com/PenguinMan05/gpu-fold/actions/runs/38471619205). That run covered 28 workspace tests, workspace check, release build, rootless sandbox probe, a level-3 synthetic repair fixture, public repository pinning/preflight, native desktop startup/screenshot, Debian packaging, and package installation.

The baseline artifact required GLIBC 2.39 or newer (Ubuntu 24.04 / Mint 22 generation, x86-64). It did not verify real model inference, RTX 4070 performance, live app-driven OAuth/fork/PR publishing, or real-project repair quality.

## Current source revision

The repository issue scan, eligibility filtering, sequential queue processing, queue UI, and GPUFold rebrand have been added after the baseline. The local work environment does not include Rust/Cargo, so `cargo fmt`, `cargo test`, and `cargo check` could not be run here. This revision must pass the repository's GitHub Actions workflow before the queue can be called verified. See the latest workflow run linked from the GitHub repository after this commit is pushed.

## Checks still requiring real hardware or user setup

- Inference with the user's selected model file and the intended GPU runtime.
- Runtime performance and VRAM measurements on a reference GPU.
- Live GitHub OAuth, fork, push, and draft-PR flow.
- Multi-node peer discovery and volunteer-worker behavior from concept draft 0.1; no peer protocol is implemented in this build.
- Real issue repair quality, non-Rust repository support, and accessibility/comprehension review.
