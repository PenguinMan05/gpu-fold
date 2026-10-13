# Rust Repair 0.1.0

A Rust desktop application and headless engine based on the attached **Local Rust AI GitHub Repair - Design Doc v1.1**. Linux is the initial platform. Application source is Apache-2.0 licensed.

## Release status - read first

This is a **source implementation, not a verified binary release**. The creation environment had no Rust compiler, Podman, GUI session, or GPU. Rust, crates.io and Ubuntu package downloads returned HTTP 403; direct network access could not resolve hosts. Consequently `cargo check`, the Rust tests, container isolation tests, GPU inference, and real GitHub publishing could not be executed here. No compiled executable or misleading prebuilt `.deb` is included.

The repository includes the application source, regression tests, an executable local build/install script, a sandbox image recipe, a headless fixture, and a requirement/status report. Follow the build gates below before using it on a real repository. The complete v1.1 benchmark and all future-platform features are not claimed to be complete.

## Build and install on your Linux desktop

You need Rust 1.85 or later (current stable via rustup is recommended), Git, a C compiler, pkg-config, a Secret Service keyring such as GNOME Keyring, and the usual X11/Wayland/OpenGL development libraries. On Ubuntu/Mint, the development packages generally include `build-essential pkg-config libx11-dev libxi-dev libxrandr-dev libxcursor-dev libxinerama-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev libdbus-1-dev`. Podman is needed only for repository test execution.

From the repository checkout:

```bash
bash scripts/build-linux.sh --install
```

The script resolves dependencies and creates `Cargo.lock`, formats the source, runs the engine tests, checks all crates, then builds both binaries. Installation happens only after those steps succeed. It places the binaries in `~/.local/bin` and creates **Rust Repair** in your application menu. A failed build stops with its error and leaves your terminal open.

To build without installation:

```bash
bash scripts/build-linux.sh
```

The code has pinned direct dependencies; transitive dependencies are not vendored and there is no generated root lockfile in this delivery. The first successful build creates that lockfile. Keep it with the source for subsequent `--locked` builds. Do not run `cargo update` as a substitute for resolving a compilation error without review.

## Start using the app

1. Open **Model & settings**. Start your separately installed local model server and enter its model identifier and loopback chat endpoint. The default is `http://127.0.0.1:1234/v1/chat/completions`.
2. Use a compatible local inference server, such as the Rust **mistral.rs** server, configured with a coding model that fits your GPU. Only the server actually loads model weights and schedules GPU work. The app does not silently download models, start a server, or assert that GPU inference is active. **Check NVIDIA GPU** reports `nvidia-smi` information.
3. Optionally choose the weights file for a recorded SHA256. This hashes the selected file; it does not attest that the external server loaded that same file.
4. In **MCP**, select your Desktop JSON file with **Browse JSON** and import it. The accepted format is a top-level `mcpServers` object with local `command`, `args`, and `env` entries. Commands must be absolute executable paths. Importing never launches a server or changes your original file.
5. In **Task**, paste a public GitHub repository URL. Pick an open issue number or describe a specific bug. **Bug sweep** produces findings only and cannot apply a patch directly.
6. Inspect contribution policies and preflight flags. Analyze locally, approve the plan, review every changed file and new reproduction test, then apply the patch locally.
7. Run the explicitly approved baseline and verification in the container. Level 3 requires two baseline failures with the reproduction marker, then two successful full-suite runs after the patch. Compile failures, changing baseline failure sets and truncated logs do not earn level 3.
8. Sign in to GitHub only when publishing. Review the etiquette checklist, title, appended AI disclosure and evidence, and the exact fork/branch/PR operations. The app opens one **draft** PR after your final confirmation.

Local data is under `$XDG_DATA_HOME/rust-repair`, otherwise `~/.local/share/rust-repair`. The app sends no telemetry. Exported records contain public source context, proposals, and test output; review them before sharing.

## Model connection

The model adapter uses the OpenAI-compatible chat request/response format over literal loopback IPs only. DNS names, remote model URLs, URL credentials, redirects and proxy forwarding by the app are rejected. A local server could itself forward requests remotely; choose a local runtime you trust.

The request asks for JSON-object output, temperature 0 and seed 42. Support for those parameters depends on the chosen server/model. Unsupported or invalid output stops the run before any edit. The model has no direct shell, filesystem, GitHub or MCP tools. Its literal replacements and citations are validated by Rust code.

Cancellation disconnects the app's request promptly. An independently running inference server may continue work it already accepted. Integrated model load/unload, exact tokenizer budgets, automatic KV-cache sizing, GPU throughput comparisons and CPU fallback controls remain future work. No particular model quality or 12 GB fit has been benchmarked here.

## Sandboxed tests

Use **rootless** Podman as your normal desktop user. Review `scripts/Containerfile`, then explicitly approve its image download/build:

```bash
bash scripts/build-sandbox.sh --approve-image-download
```

The app refuses to test on the host and refuses to pull a missing image. It resolves the configured local image to an immutable digest for each verification. Container execution has:

- no network or GPU devices; no tokens or inherited host environment;
- no runtime socket, no writable host mounts and dropped capabilities;
- a read-only source snapshot, read-only vendor directory, and limited writable `/tmp`;
- 4 GB RAM, 2 CPUs, 256 processes, 1 GB temporary disk and a 10-minute timeout per run;
- direct `cargo test --offline --locked -- --test-threads=1`, never a model-provided shell command.

Dependencies are downloaded by the app from one fixed HTTPS host (`static.crates.io`) with redirects disabled and verified against the committed lockfile. Archive links, special files and unsafe paths are rejected; file/download/expanded-size budgets are applied. Only the crates.io registry source is accepted. Git/custom registry dependencies, repositories without a committed lockfile, and crate archives containing links are rejected in this initial version.

Repository `.cargo/config*` and `rust-toolchain*` files are shown in preflight and removed from disposable execution copies. The sandbox runs its trusted installed toolchain. This can make projects that rely on custom configuration fail; such failures are reported rather than bypassed. Source is read-only even inside the container, so tests that write to their source directory need adaptation. The v1.1 broader writable-copy/OS support is deferred.

Containers share the host kernel. Hostile-fixture isolation and cancellation tests must pass on your machine before real untrusted repositories are tested. No claim of a demonstrated sandbox security boundary is made by this uncompiled delivery.

## MCP

Local stdio MCP is implemented using the official `rmcp` Rust SDK. Discovery requires explicit server-launch consent; every tool call requires approval of both the definition and exact JSON arguments. Definitions are hashed and pinned, arguments are schema checked with external schema references disabled, and definitions are rechecked after reconnect before a call. Results are marked untrusted and size checked after decoding.

MCP servers run with your user privileges and only explicitly listed environment variables. Their working directory is an app-owned empty MCP workspace. MCP roots are not a security sandbox: the app cannot stop an approved server from accessing other files or the network. This limitation is stated in the UI. Tools are called manually, not automatically by the repair model. Remote MCP, server-side OAuth, strict wire-level response limits, automatic read-only classifications, automatic tool retrieval and the full hostile-server benchmark are deferred. Output checks after SDK decoding do not protect against oversized incoming messages before decoding.

## GitHub authorization and publication

The app supports OAuth device flow with an OAuth client ID whose registration has device flow enabled. No registered project OAuth ID is embedded; enter your own in Settings, or use an existing OAuth/classic `public_repo` token. Credentials go into the OS keyring, never SQLite or the test container. Fine-grained cross-owner PR behavior is not assumed to work.

Publishing checks level 3, every changed-file approval, policy/duplicate/issue prerequisites and final user confirmation. Some obvious AI-contribution prohibitions block publishing automatically; policy interpretation still needs human review. Recent 100 issues/PRs are displayed as a starting point, not an exhaustive duplicate search. The app discloses the model and evidence, never merges or force-pushes, and never pushes to upstream. A changed upstream head blocks publication and requires a fresh run.

OAuth organizations, real fork/push/PR permissions, and credential integration still need live verification. No GitHub content was published during creation. Interrupted publication is saved and never automatically retried: inspect GitHub first. Default-name personal forks are supported; customized fork names and automatic rebase are not. The initial rate gate allows three publications in a rolling 24 hours and conservatively blocks any second app publication to a repository that already has a recorded PR, even if it later closed.

## Headless verification

After building and preparing the sandbox:

```bash
bash scripts/verify-fixture.sh
```

This runs a trusted, dependency-free synthetic Rust addition bug through the same engine using a supplied proposal. It requires real fail-before/pass-after observations and exports `dist/fixture-result.json`. It is not a real-world AI evaluation. Other CLI commands are listed by `dist/rust-repair --help`. Tests in `crates/repair-core/tests/security.rs` cover unsafe paths, invented citations, scope violations, weakening test sections, tampered patches, failed compilation, flaky results, unsafe archives, secrets and publication blockers.

`docs/IMPLEMENTATION_STATUS.md` tracks remaining v1.1 requirements. `docs/VERIFICATION.md` records what could and could not be checked here. Once the build and fixture pass, you can make a local package with `bash scripts/package-deb.sh`. That package is built for the machine compiling it; compatibility with other distributions is not presumed.

## Primary references

- Rust MCP SDK: https://github.com/modelcontextprotocol/rust-sdk
- mistral.rs: https://github.com/EricLBuehler/mistral.rs
- egui/eframe: https://github.com/emilk/egui
- GitHub device flow: https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps
- GitHub pull requests: https://docs.github.com/en/rest/pulls/pulls
