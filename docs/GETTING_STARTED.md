# Start using GPUFold

1. Install the `.deb` package and open **GPUFold** in your applications menu.
2. **Connect model** opens on first launch. Choose one option:
   - **LM Studio:** load a coding model, start its Developer / Local Server, then click **Check connection** in GPUFold.
   - **Ollama:** install a coding model, keep the Ollama service running, and click **Check connection**. Choose the installed model from the list.
   - **My model file:** select a separately installed `llama-server` (llama.cpp) or `mistralrs` engine, choose your GGUF file, approve launching it, and click **Start model and connect**. GPUFold waits for loading to finish. A compatible GPU build of the engine is required for GPU inference.
   - **Other local server:** start an OpenAI-compatible server and enter its local address. Base URLs and `localhost` are accepted; remote addresses are rejected.
3. When **Connected** appears, continue through setup. **Test environment** downloads the isolated Rust test image after your approval. You can prepare this later, before verifying repairs.
4. Finish setup and open **Issues**. Paste a public Rust / Cargo repository URL. Click **Find open issues**. GPUFold looks for open issues with no assignees; discussion may already exist, so review each ticket.
5. Select up to 10 issues and click **Start selected issues**. Keep your model app running. Watch progress; **Stop current task** cancels the current work. Finished results remain in **Results** and **History**.
6. Open a result, read the model's explanation, and approve its plan. Review each file, apply the local patch, and approve sandbox tests. The UI states the next step at each stage.
7. If verification succeeds, optionally sign in to GitHub from Advanced settings and approve a draft pull request. Nothing is merged automatically.

## If the model does not connect

“Connection refused” means no model server is listening at the configured address. The `.deb` installs GPUFold, not a model engine or weights. Open your model app, load a model, start its local server, check the port, and click **Check connection** again. A running server with an empty model list is not ready. After changing servers or model launch settings, recheck the connection.

## Current project support

This version repairs Rust / Cargo projects with a committed Cargo.lock and crates.io dependencies. It cannot repair JavaScript, Python, C++, or other language projects yet. The model engine needs enough GPU memory (or CPU/RAM) for your model and context. Use the memory estimate in Setup and your engine's diagnostics.
