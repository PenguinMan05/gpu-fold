# GPUFold — Rust Repair

A Linux Rust desktop app for investigating one bug in a public Rust/Cargo repository, reviewing a proposed patch, reproducing the failure in an isolated container, and preparing a draft pull request after human approval.

## Get started

Use the `.deb` in the successful GitHub Actions `linux-build` artifact, or build this checkout with Rust 1.90+:

```bash
bash scripts/build-linux.sh --install
```

The native application is **Rust Repair**. The headless executable is `rust-repair`.

1. Open **Home → Try the known-bug demo** to practice the review workflow without an AI model. It uses a supplied, clearly identified synthetic proposal and cannot publish.
2. On Home, check the sandbox setup approval and click **Prepare test sandbox**. The app runs the bundled setup script as your regular user. It explicitly downloads the official Rust container base the first time; later repository test runs do not download images. From a terminal you can run:

```bash
bash scripts/build-sandbox.sh --approve-image-download
```

3. In **Model & settings**, select your installed `mistralrs` executable and complete local model folder, or enter an existing local OpenAI-compatible endpoint. Select the matching `tokenizer.json` for accurate budgeting and record the model license. Choose CPU mode if needed. Review and approve the displayed launch command, then click **Load local model** and **Check local model connection**. Any change to the executable, model, endpoint, CPU option, or context limits requires fresh approval. A single discovered model ID is selected automatically.
4. Optionally import your Desktop MCP JSON in **MCP**. Import does not launch anything. Review server and tool definitions, classify tools yourself, and allow specific tools in a repair run. MCP provides tools, not model weights.
5. In **Task**, enter a public repository and an open issue or specific bug. Preview the exact source scope, then analyze. A bug sweep produces findings only.
6. Review and approve the plan. Run the original-source baseline twice, review every changed file and reproduction test, apply the patch locally, and run verification. Failed verification can feed a bounded revision attempt.
7. At evidence level 3, sign in for publication, review contribution rules and duplicates, and explicitly approve one draft PR. The app never merges or pushes upstream.

## What has been checked

The workspace compiles and 27 local regression tests pass, including desktop approval and setup checks. GitHub CI has executed the real rootless known-bug workflow to level 3 and a sandbox isolation probe. CI also gates the native desktop launch, screenshot and package build; download binaries from a successful run rather than a failed artifact.

The tests verify application behavior, not the accuracy of a coding model. Actual RTX 4070 inference, the design's real-world benchmark and live app-driven OAuth/fork/pull-request operations have not yet been verified. See [design acceptance](docs/DESIGN_ACCEPTANCE.md) for the full implemented/remaining distinction.

## Prerequisites

The app package needs a Linux desktop, Git, rootless Podman and a running Secret Service keyring for GitHub credentials. The inference runtime and model are separate local assets. No paid AI account or hosted project server is required.

Building from source additionally needs a C compiler and common Linux development packages, including pkg-config, libdbus-1-dev, X11/Wayland/OpenGL headers, and libxkbcommon-x11-0. `Cargo.lock` is committed; builds use `--locked`.

## Boundaries

- Public Rust repositories with a committed Cargo.lock; crates.io registry dependencies only.
- Test code runs inside rootless containers: no host credentials, network, GPU or runtime socket. Host source mounts are read-only; the writable copy and target directory live in bounded container tmpfs.
- The external local model receives only bounded public source context and reviewed tool results. Literal loopback endpoints, no proxy and no redirects are enforced by the app.
- A managed mistralrs process runs selected local assets with a minimal environment. User-selected runtime executables and MCP servers run with the user's privileges; only run executables you trust.
- MCP definitions are pinned and rechecked; wire and result sizes are capped. Root checks validate recognized path arguments but cannot enforce the internals of an arbitrary privileged local server. Remote MCP is disabled.
- The app refuses to publish below evidence level 3 and stops if upstream moved. It never automatically repeats an interrupted publication.
- No telemetry. History lives in `$XDG_DATA_HOME/rust-repair`, or `~/.local/share/rust-repair`. Workspace deletion retains the history and PR metadata used for rate limits.

## Verify and package

```bash
bash scripts/verify-fixture.sh
./dist/rust-repair sandbox-probe
bash scripts/package-deb.sh
```

The package records its actual minimum GLIBC requirement and includes the sandbox recipe and trusted Rust container entrypoint. It is a Linux x86-64 package; cross-distribution compatibility is not assumed.

Application code is Apache-2.0, matching the repository. Model and dependency licenses remain separate.
