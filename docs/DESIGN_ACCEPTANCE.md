# Concept draft 0.1 acceptance

The governing product concept is [GPUFold: Open GPU Swarm for GitHub Projects](GPUFOLD_CONCEPT.md). Its promise is a peer-to-peer network where requesters publish bounded jobs and volunteer workers opt in to local GPU inference. A person reviews and applies any code change.

## Current product path

The current Linux application provides a local, single-machine first step for the clarified ticket workflow:

| Requirement | Current implementation |
|---|---|
| Point GPUFold at a public GitHub repository | Repository URL field; anonymous metadata and issue APIs; repository source is pinned to a commit when a run starts. |
| Find tickets that have not been taken by a maintainer | Paginated open-issue scan, pull requests excluded, unassigned issues shown oldest first; eligibility is checked again before inference. |
| Use a model file selected by the user | Existing settings select a local runtime/model directory or GGUF file, or a loopback-compatible local inference endpoint. The runtime determines whether inference uses GPU or CPU. |
| Work through tickets | User selects up to 10 tickets; the app processes them sequentially, persists one run per issue, and shows per-ticket outcomes. |
| Produce a fix | The model proposes bounded Rust edits and a regression test. Source paths and replacements are validated before any patch can be applied. |
| Protect the repository and user | Queue inference does not execute repository code. A human reviews a plan and changed files, explicitly approves sandbox execution, and decides whether to publish a draft PR. |

“Unattended” currently means **open and unassigned**. This is a practical GitHub signal, not a guarantee that nobody is discussing the issue. People can inspect links and comments before selecting tickets.

## Concept requirements not yet accepted

| Concept area | Status |
|---|---|
| Requester and worker roles on separate nodes | Not implemented. The local requester and local inference runtime run on the same machine. |
| Signed job manifest with commit/file hashes and task constraints | Not implemented. Runs pin their source commit locally; there is no network job protocol. |
| Direct peer connection and worker accept/decline | Not implemented. |
| DHT or bootstrap peer discovery | Not implemented. |
| Volunteer GPU resource policy, cancellation and retry | Local queue cancellation exists; peer worker controls, temperature/power ceilings, and remote retry are not implemented. |
| Safe public-repository context transfer | Public repositories only. No worker network exists, and no GitHub token is sent to an inference process. |
| General open-source workload support | The current patch and test pipeline supports Rust Cargo repositories only. |
| Human review | Implemented as separate proposal review, patch application, test verification, and explicit draft-PR publishing steps. No automatic merge is available. |

## Existing safety and quality gates

- Public, non-archived repositories only. The current repair engine requires Rust Cargo metadata and a committed lockfile.
- Model context is bounded and sent only to the configured local provider. Runtime and model files are user-selected; they are not downloaded by GPUFold.
- The Rust patch validator requires exact source citations and bounded unique replacements, protects existing tests, and checks the proposed reproduction test against the pristine source.
- Test execution uses rootless Podman, pinned image identity, offline vendored dependencies, no network or host credentials, read-only source, and a bounded writable copy.
- Draft-PR publication requires level-3 evidence, per-file review, repository contribution checks, explicit consent, and a fork destination. GPUFold does not merge or push to upstream.

The previous one-issue implementation and its verified Linux artifacts establish a baseline only. Queue behavior must pass CI on the current revision; actual model quality and the peer-to-peer concept require separate real-machine and multi-node validation.
