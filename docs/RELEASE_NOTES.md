## GPUFold 0.5.0 — source investigation and AI conversation

- Move task controls to the right side. AI conversation sits above Stop current task. Remove the bottom activity panel and the duplicate controls in the left sidebar.
- Open a separate, scrollable dialogue feed with task messages, brief model replies, source reads/searches, revisions, and results. Replies stream in the panel; prior replies are retained and saved with run history. Ctrl+4 toggles it.
- Give the model built-in read-only source tools: read_file and search_code, limited to the pinned repository. It can inspect implementations, callers, manifests, tests, and project guidance before producing a patch. These tools never run commands or access the network.
- Refocus rejected attempts on the implicated source and include the prior model summary with concrete correction feedback. Source inspection requests get fresh evidence rather than repeating a blind generation.
- Automatically correct citation line numbers only when the full snippet matches unique exact pinned source (or already matches its stated location). Invented or ambiguous snippets remain rejected. Verified edits, existing-test protection, sandbox approval, and draft-PR review remain enforced.

Install the uniquely named .deb on x86-64 Ubuntu 24.04 / Linux Mint 22 generation systems (glibc 2.39+). The inference engine remains included: select a local coding GGUF and click Load model.

Verification covers exact citation anchoring, rejection of invented/ambiguous citations, read-only tool dispatch, focused source on subsequent attempts, shell/traversal rejection, saved conversations, the right-side controls, on-demand dialogue rendering, real GGUF tokenization/schema generation, native desktop screenshots, isolated repair fixtures, and package installation.

Model repair quality still depends on the chosen coding model. This improves investigation and retry behavior; it does not guarantee every ticket can be solved. A proposal still goes through human review and test approval. The current repair pipeline supports Rust/Cargo repositories with Cargo.lock; the peer GPU network remains unimplemented.

The toy CI GGUF uses a declared 2048-token context solely for protocol testing; weights stay unchanged and it is not shipped. CI has no physical GPU.
