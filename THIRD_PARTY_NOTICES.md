# Third-party notices

DropCut itself is released under the MIT License (see `LICENSE`). It uses and redistributes the third-party software and models below under their own licenses. Dependency versions are pinned in `package-lock.json` and `src-tauri/Cargo.lock`; the full list of transitive Rust and npm licenses can be generated with `cargo license` and `npx license-checker`.

Current primary dependencies:

- Tauri (Apache-2.0 / MIT)
- React (MIT)
- Vite (MIT)
- Lucide (ISC)
- whisper.cpp v1.9.4 (MIT, built from source for Windows packages)
- Silero VAD v5.1.2, GGML conversion `ggml-silero-v5.1.2.bin` from `ggml-org/whisper-vad` (MIT), embedded in the app to skip non-speech before transcription
- llama.cpp b11140 (MIT, official prebuilt Windows CPU binaries, checksum-pinned)
- Qwen2.5-1.5B-Instruct Q4_K_M GGUF (Apache-2.0, bundled ~1.1 GB, checksum-pinned; license at `resources/licenses/Qwen2.5-LICENSE.txt`)
- FFmpeg 8.1.3 BtbN `win64-gpl-8.1` static build (GPL and applicable component licenses)
- reqwest (Apache-2.0 / MIT)
- RustCrypto SHA-2 (Apache-2.0 / MIT)
- rustface and the SeetaFace frontal face detection model `seeta_fd_frontal_v1.0.bin` (BSD-2-Clause, embedded in the application for local face tracking; license text in `resources/licenses/SeetaFace-LICENSE.txt`)

Windows release preparation downloads a checksum-pinned FFmpeg archive and compiles whisper.cpp from its pinned upstream tag. Their upstream license files are copied into the packaged `licenses` resource directory. Development builds can instead use compatible executables installed on `PATH`.

### FFmpeg (GPL) and source availability

The Windows installer bundles an unmodified BtbN `win64-gpl` FFmpeg 8.1.3 build and runs it as a separate program (DropCut only starts it with command-line arguments; it is not linked into DropCut). FFmpeg's license text is packaged in the `licenses` directory. The corresponding FFmpeg source is available from https://ffmpeg.org/releases/ (tag `n8.1.3`) and from the build recipe repository https://github.com/BtbN/FFmpeg-Builds (release `autobuild-2026-09-22-13-18`). Anyone redistributing the installer must keep these notices and offer the same source access.

Optional converted Whisper GGML model files are downloaded from the `ggerganov/whisper.cpp` Hugging Face repository, which identifies the repository license as MIT. Their license and attribution requirements are not covered by DropCut's MIT license; check the model card before redistributing a model.
