# Start using GPUFold

1. Install the `.deb` package and open **GPUFold** from your applications menu.
2. In **Setup → Load model**, leave **Local GGUF file** selected.
3. Click **Choose GGUF file**, select your local model, then click **Load model**.
4. Wait for **Model loaded**. GPUFold includes the inference engine and starts it automatically. No engine executable, separate server, or port configuration is needed.
5. Continue through setup. **Test environment** downloads the isolated Rust test image after approval. You can prepare this later, before verifying repairs.
6. Open **Issues**, paste a public Rust / Cargo repository URL, and click **Find open issues**. Select up to 10 and click **Start selected issues**.
7. Watch **Working · live activity** below the status message for source scope, project guidance, generation, validation, and retry feedback. **Model summary** streams the model’s brief explanation. Failed attempts get another queue turn automatically; Stop current task cancels generation or retry waits.
8. Open each result and follow its review instructions. Review the model's explanation and changed files, apply the local patch, then approve the isolated tests. GitHub sign-in is needed only if you later publish a draft pull request.

## Model options

GPUFold bundles llama.cpp with Vulkan GPU support and CPU fallback. Your normal compatible GPU driver is still needed for acceleration; no CUDA toolkit or separate inference engine installation is required. If a model does not fit GPU memory, open **Model options** and select **Use CPU only**, then load it again. Large models need sufficient system RAM and can run slowly on CPU.

Use a complete GGUF coding model supported by the included engine. A download page, archive, incomplete download, or unsupported model architecture will not load. **Model loading details** contains the engine's error message. GPUFold never downloads model weights for you.

## Optional model apps

LM Studio, Ollama, or another local OpenAI-compatible server can be selected instead of the included GGUF engine. Start that external app and its local server, load or install its model, then use **Check connection**. This is optional and does not apply to **Local GGUF file**.

## Current project support

This release repairs Rust / Cargo projects with a committed Cargo.lock and crates.io dependencies. Other project languages and the volunteer GPU peer network are not implemented. Model compatibility and repair quality depend on your selected model. Every patch remains subject to your review and test approval.

## Automatic retries and context

Malformed proposal JSON, incorrect source citations, truncated responses, and unsupported repair attempts are saved and retried with corrective feedback. There is no three-attempt cutoff. The queue gives other tickets a turn before revisiting an unresolved one. Histories keep the latest eight prompts and outputs plus recent attempt notes to bound storage. Authentication and missing API errors require attention; one ticket’s problem does not discard the rest of the batch.

The included engine supplies the GGUF tokenizer automatically. GPUFold fits excerpts of source, issue text, project instructions, and feedback into the configured input window. It can rebalance input/output within the already loaded model window after a truncated response. A larger context can still help large tasks; increasing it requires stopping and reloading the model.

Project AI guidance is read from the pinned repository, including applicable nested AGENTS.md files, skill documents, Copilot instructions, and editor rules. Names and source scope appear in live activity. Guidance is bounded by context space, cannot grant command execution or credential access, and does not bypass your review. Retries cannot guarantee that a model can solve every issue; unresolved work continues until you stop it.
