## Guided setup and clearer issue workflow

- First launch opens a three-step guide: connect a model, prepare optional test tools, and choose issues.
- Model setup includes LM Studio and Ollama instructions, localhost/base-URL handling, available-model selection, and readable connection recovery.
- A selected GGUF can be launched through a separately installed llama-server or mistralrs engine. Loading waits for readiness; mistralrs GGUF files now use its correct `-f` argument.
- Starting a batch requires a successful model connection check. Connections are rechecked on app startup and before a batch.
- Dashboard, issue selection, results, and repair review have distinct navigation and next-step instructions. Each page has its own scroll position, and issue lists no longer hide the Start action below hundreds of rows.
- Higher contrast controls, larger text, and visible error guidance replace tiny connection errors in the status bar.
- Debian package identity is now `gpufold`. Launch **GPUFold** from the application menu or run `gpufold`. Existing CLI names remain available.
- Package downloads include a unique SHA-1 filename suffix and a SHA-256 checksum file.

## Install / upgrade

Download the `.deb` asset for an x86-64 Ubuntu 24.04 / Linux Mint 22 generation system (glibc 2.39+). Open it with your distribution's package installer, or run `sudo apt install ./GPUFold_*.deb` from its download folder. The package replaces the older `rust-repair` package.

Open **GPUFold** from the application menu. Follow **Setup & settings**. You must have a local model app or separately installed model engine and model weights; these are not bundled. Public issue scanning needs no GitHub login. The isolated test environment is needed later to verify repairs.

## Scope and verification

This is an early single-machine release. The repair engine supports Rust / Cargo repositories with a committed Cargo.lock and crates.io dependencies. Other languages and the peer-to-peer volunteer GPU network are not implemented. Test fixtures and mock local model servers verify application behavior; actual inference speed, GPU compatibility, and repair quality depend on your model and engine and require testing on your hardware.

The release workflow publishes only after workspace tests/checks, binary builds, isolated repair and sandbox fixtures, public repository preflight, desktop launch/screenshots, and Debian package installation succeed.
