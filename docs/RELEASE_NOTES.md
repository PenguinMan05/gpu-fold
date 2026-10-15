## GPUFold 0.4.1 — persistent analysis and live activity

- Fix missing proposal fields by supplying an actual JSON schema to the included engine, including the required explanation field. Optional external providers use schema mode when supported and fall back to plain JSON mode.
- Automatically retry malformed JSON, invalid citations, unsupported repair attempts, and interrupted/truncated generation with corrective feedback. Remove the three-attempt cutoff. Failed queue attempts rotate behind other tickets, so one difficult issue does not block all other selected work.
- Show a visible live activity panel below the status message: source scope, applicable instruction files, generation, validation, errors, retries, and the model’s brief explanation. Stop is available in the activity panel.
- Use the bundled model tokenizer automatically. Fit source and issue excerpts into the input window, and rebalance input/output within the loaded model window when output is truncated. No tokenizer.json is required for GGUF use.
- Include scoped AGENTS.md, skills.md / SKILL.md, CLAUDE.md, GEMINI.md, Copilot instruction files, and editor rules in model context. Project guidance cannot bypass execution, credential, or review controls.
- Bound retained prompts/outputs and activity while preserving the total attempt count and saved state. Existing run histories remain readable.

## Installation

Install the uniquely named .deb on x86-64 Ubuntu 24.04 / Linux Mint 22 generation systems (glibc 2.39+). Open GPUFold, select the GGUF, and click Load model. The pinned llama.cpp Vulkan/CPU engine remains included. Your normal compatible GPU driver is needed for GPU acceleration.

## Verification and limits

Regression checks cover repeated missing-explanation responses beyond three attempts, feedback-driven citation repair, unresolved queue turns, cancellation during retry waits, oversized issue/source fitting, scoped project instructions, bounded activity, and older history records. The real GGUF test checks tokenization and schema-constrained chat output as well as loading, generation, and unloading, before and after package installation. Desktop launch, isolated fail-before/pass-after repair tests, and package installation must pass before publication.

The tiny model verifies the inference interface, not real-world coding quality. CI has no physical GPU. A local model cannot be guaranteed to solve every issue; unresolved analysis retries until stopped. Applying code, running project tests, and publishing still require review. This build supports Rust/Cargo repositories with Cargo.lock; the peer-to-peer GPU network remains unimplemented.
