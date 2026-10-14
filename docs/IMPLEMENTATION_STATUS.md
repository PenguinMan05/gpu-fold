# GPUFold implementation status

GPUFold is now centered on its ticket-queue use case: scan a public repository, find open unassigned issues, and generate repair proposals sequentially with the selected local model. Each issue gets an independent, saved run that can be reviewed in the desktop app.

## Implemented in this source revision

- Paginated GitHub issue discovery, oldest first. Pull requests and closed issues are excluded. The default queue includes only issues with no assignee.
- A queue batch limit of 10, per-ticket progress, persisted run history, and an option to skip tickets already in local history.
- Eligibility is checked again while preparing each issue and before model inference, so newly assigned or closed tickets are skipped.
- Sequential local-model analysis. Queue processing creates proposals only; existing plan review, local patch application, isolated verification, and draft-PR approval remain separate steps.
- GPUFold branding in the app, README, desktop entry, and CLI help text.

## Supported today

- One Linux machine, one public GitHub repository at a time, and a local model directory/GGUF file or loopback model endpoint.
- Rust Cargo repositories with a committed `Cargo.lock` and crates.io dependencies. The current parser, patch validator, and test sandbox are Rust-specific.
- Human review for source changes, sandbox execution approval, and draft publication. GPUFold does not merge or push changes to the upstream repository.

## Not implemented from concept draft 0.1

- Peer requester/worker nodes, signed job manifests, direct peer transport, bootstrap invitations, DHT discovery, or peer-replicated job state.
- Volunteer GPU advertising, opt-in worker policies, per-worker power/temperature limits, or cross-network connectivity tests.
- General-purpose repair across non-Rust projects and separate bounded workloads such as code review, documentation, and test suggestions.

The queue feature and its documentation are in the current source tree. The latest build and regression-test result must be read from [verification notes](VERIFICATION.md); only a successful CI run validates this revision. GPU inference throughput, model correctness, peer networking, and real-project repair rate remain unverified.
