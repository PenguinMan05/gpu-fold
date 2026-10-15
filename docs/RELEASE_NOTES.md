## GGUF setup now needs only your model file

- The installer includes a pinned llama.cpp inference engine, its shared libraries, Vulkan GPU backend, and CPU support.
- Choose **Local GGUF file → Choose GGUF file → Load model**. No engine executable, external app, server startup, or port configuration is required.
- GPUFold selects a free loopback port, starts the included engine, waits for the model to become ready, and releases it when stopped or the app closes.
- Existing file-based configurations switch to the included engine. LM Studio and Ollama remain optional alternatives.
- Model weights remain local and user supplied. The app never downloads model weights or executables at runtime.

## Install / upgrade

Install the `.deb` on x86-64 Ubuntu 24.04 / Linux Mint 22 generation systems (glibc 2.39+). Open **GPUFold** from the application menu. The package installer resolves its normal system library dependencies. GPU acceleration needs a working Vulkan-capable GPU driver; CPU mode works without a GPU driver or CUDA toolkit.

The included engine is the official llama.cpp b11627 Ubuntu x64 Vulkan build, pinned by SHA-256, from source commit 8c2134a8755d8a9792af513dab20282affe903e4. Upstream license and provenance are included in the package. Its source is https://github.com/ggml-org/llama.cpp/tree/8c2134a8755d8a9792af513dab20282affe903e4.

## Verification and scope

CI verifies the bundled engine's checksum and dependencies, loads a pinned tiny real GGUF, generates real tokens, unloads it, exercises the desktop Load model worker with only a model path, and repeats real inference using the installed package's engine. Workspace tests, isolated repair fixtures, desktop navigation/screenshots, and package installation must also pass before release publication.

The tiny GGUF test verifies loading and inference plumbing, not coding quality. CI does not have a physical GPU; Vulkan GPU performance and compatibility require testing on your hardware. This release's repair engine still targets Rust / Cargo projects with Cargo.lock and crates.io dependencies. The peer-to-peer volunteer GPU network is not implemented.
