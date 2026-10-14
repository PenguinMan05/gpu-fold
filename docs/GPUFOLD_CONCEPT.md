# Open GPU Swarm for GitHub Projects
## Concept draft 0.1

### Summary

An open-source, peer-to-peer network that lets people contribute GPU time to help with work on public GitHub projects. Anyone can run a requester node to publish a task, a worker node to contribute GPU compute, or both. There is no permanent central job server: the requester’s own node starts and coordinates a task while volunteer nodes connect directly, accept work, and return results.

The initial aim is to help open-source maintainers and contributors use AI for bounded tasks such as understanding a codebase, reviewing a change, drafting tests or documentation, and proposing a patch. A person remains responsible for reviewing and applying any code changes.

### Problem and purpose

Many open-source contributors have useful project ideas but limited access to capable GPUs or hosted AI services. At the same time, some community members have GPUs they are willing to share. This project would connect those resources directly, so an open-source project can request AI work and volunteers can choose whether to contribute compute.

### Guiding principles

- Anyone can inspect, contribute to, run, or host the software.
- Nodes can request work, provide compute, or do both.
- No central operator is required to hold jobs, route every message, or control the network.
- Workers opt in to each task and set local GPU, power, and time limits.
- The first release supports public repositories and bounded, approved task types.
- AI outputs are suggestions. A human project contributor reviews patches before they are applied or submitted.

### Participants and node roles

- **Requester node:** A contributor selects a repository, commit, and task. Their own node publishes the job, tracks responses, and assembles results.
- **Worker node:** A volunteer’s node advertises available GPU capability, receives job details, runs an approved inference workload, and returns results.
- **Dual-role node:** Any participant may request work and contribute compute on other jobs.

These are software roles, not separate kinds of accounts or centrally granted permissions.

### Proposed workflow

1. A requester selects a public GitHub repository at a specific commit and describes a bounded task, such as “suggest tests for this function” or “review this change for likely regressions.”
2. The requester’s node creates a signed job manifest. It identifies the repository commit, included files and their hashes, task type, model/runtime requirements, resource limits, and expected result format.
3. The job is announced to the peer network. Interested workers connect directly, check the manifest against their local policy, and accept or decline it.
4. A worker runs the supported AI inference task on its own machine and returns a structured result or patch suggestion.
5. The requester checks the result, optionally compares it with independently produced results, applies it to a local checkout, and runs tests locally.
6. A human reviews the change and decides whether to open a pull request. The software does not merge changes automatically.

The initial version should not send GitHub credentials to workers or ask volunteer GPUs to run arbitrary shell commands, repository scripts, or executables supplied by strangers.

### Peer-to-peer design

The network can use a distributed hash table (DHT) or similar peer-discovery protocol to find nodes and advertise jobs. Job files and model assets can use content hashes so peers can check that transferred data matches the requested files.

There is no central job scheduler in this design. The requester node is a temporary coordinator for its own task. If that node disconnects, the first version may pause or lose coordination for that job; keeping jobs available after a requester disconnects would require a later peer-replicated job-state design.

“No central host server” still leaves a peer-discovery problem: a new node needs some way to find its first peers. The initial release could support direct invitations or an exchanged peer address, then add DHT discovery with a small configurable list of community bootstrap peers. Bootstrap peers help nodes enter the network; they need not store jobs or results. Direct connections through home routers may fail for some participants, so connectivity needs testing across real networks.

### Workloads and realistic scope

The easiest workloads to distribute are many independent jobs: separate code-review questions, documentation drafts, test-generation requests, or repository questions. Each worker can handle a task without synchronizing constantly with other GPUs.

A single coding task is more difficult to speed up by spreading it across many GPUs. An AI coding agent typically makes decisions in sequence: inspect code, propose an edit, see test results, then decide what to do next. Splitting one model across machines is possible, but network delays and volunteer disconnections can make it slower and more fragile. The first version should prioritize many independent jobs over the promise of one enormous pooled GPU.

### Safety, privacy, and result checks

- Start with public repository content. Private repositories and secret-bearing inputs should be out of scope until a trustworthy privacy design exists.
- Do not give workers GitHub tokens, write access, or permission to merge or push changes.
- Use a limited set of task types and a standard, signed worker runtime. A job description is data for the worker, not permission to execute arbitrary code.
- Require local volunteer controls for GPU usage, temperature/power limits where available, pause/stop, and maximum job duration.
- Hashes can verify that files arrived intact; they cannot prove that an AI result is correct. The requester should validate results, and selected jobs can be duplicated across workers to spot inconsistent answers.
- Apply patches and run repository tests on the requester’s machine, preferably in an isolated checkout. Volunteer GPUs should not execute the repository’s test scripts in the initial release.

### Language recommendation: Rust

Use **Rust for the peer node, job protocol, discovery, and command-line application**. These parts handle untrusted network input, parallel connections, task state, and local permissions. Rust’s memory-safety guarantees and concurrency support make it a stronger default for this always-on, network-facing software than C, while remaining suitable for a small cross-platform node.

Use established GPU and inference libraries through stable interfaces rather than writing custom GPU kernels for the first release. The node can call supported CUDA, ROCm, or other inference backends through adapters or foreign-function interfaces. C remains useful at those low-level boundaries and for integrating existing libraries, but writing the whole peer application in C would increase memory-safety and maintenance risk.

### License and contribution model

Proposed software license: **Apache-2.0** for broad reuse and an explicit patent grant. MIT is also suitable if the project prioritizes the shortest, simplest permissive license. The license applies to the software; it does not guarantee that any hosted service, GPU time, model weights, or datasets are free to use.

Contributions should be accepted through public issues and pull requests with documented build steps, protocol specifications, and a code of conduct. The peer protocol should be documented independently of one specific client implementation so multiple compatible node implementations can participate.

### Initial milestone sequence

1. Define and document a signed job-manifest format and a small set of safe task types.
2. Build a Rust node that can connect directly to another node using a user-provided peer address.
3. Run one worker inference task end to end and return a structured result.
4. Add opt-in GPU capability reporting, resource limits, cancellation, and retry behavior.
5. Add peer discovery and DHT-based job announcements, including bootstrap configuration.
6. Test across different GPUs, operating systems, home-router setups, and peers that disconnect unexpectedly.

### One-sentence description

**An open, peer-to-peer GPU swarm where anyone can request or donate compute to help people work on public GitHub projects, with no central job server and human review for every code change.**

### Background references

- [BitTorrent BEP 5: DHT Protocol](https://www.bittorrent.org/beps/bep_0005.html)
- [libp2p: Distributed Hash Tables](https://libp2p.io/docs/dht/)
- [libp2p: Hole Punching](https://docs.libp2p.io/concepts/hole-punching)
- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0.html)

