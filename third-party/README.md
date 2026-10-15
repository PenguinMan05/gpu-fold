# Included inference engine

The GPUFold release pipeline includes the official llama.cpp `b11627` Ubuntu x64 Vulkan binary release. `scripts/bundle-engine.sh` pins its SHA-256 and writes source/release provenance into `UPSTREAM.json`. No inference executable is downloaded by the installed app.

Source commit: https://github.com/ggml-org/llama.cpp/tree/8c2134a8755d8a9792af513dab20282affe903e4

Release archive SHA-256: `e5eaae62de38776a837d1f415a461dd334342c1a247073c51a065e04ff773c1f`

`llama.cpp-LICENSE.txt` contains the upstream MIT license. The bundle script also preserves any license/notice files in the upstream archive. The engine's source code remains available at the pinned upstream commit; it is not represented as GPUFold's Rust implementation.

The tiny GGUF used in CI is downloaded from the pinned ggml-org/tiny-llamas commit and verified by SHA-256. It is not shipped in the installer and is not a coding model recommendation.
