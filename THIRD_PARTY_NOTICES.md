# Third-party notices

This is an early development version created before the DropCut project license is finalized. Before public distribution, this document and the packaged license files must receive a complete legal review covering all dependencies, fonts, icons, codecs, and AI models.

Current primary dependencies:

- Tauri (Apache-2.0 / MIT)
- React (MIT)
- Vite (MIT)
- Lucide (ISC)
- whisper.cpp v1.9.4 (MIT, built from source for Windows packages)
- llama.cpp b11140 (MIT, official prebuilt Windows CPU binaries, checksum-pinned)
- Qwen2.5-1.5B-Instruct Q4_K_M GGUF (Apache-2.0, bundled ~1.1 GB, checksum-pinned; license at `resources/licenses/Qwen2.5-LICENSE.txt`)
- FFmpeg 8.1.3 BtbN `win64-gpl-8.1` static build (GPL and applicable component licenses)
- reqwest (Apache-2.0 / MIT)
- RustCrypto SHA-2 (Apache-2.0 / MIT)
- rustface and the SeetaFace frontal face detection model `seeta_fd_frontal_v1.0.bin` (BSD-2-Clause, embedded in the application for local face tracking; license text in `resources/licenses/SeetaFace-LICENSE.txt`)

Windows release preparation downloads a checksum-pinned FFmpeg archive and compiles whisper.cpp from its pinned upstream tag. Their upstream license files are copied into the packaged `licenses` resource directory. Development builds can instead use compatible executables installed on `PATH`.

The selected FFmpeg package is a GPL build. Public distribution therefore requires DropCut's complete corresponding-source and license obligations to be satisfied. The automated build is technical packaging infrastructure, not a substitute for that compliance review.

Optional converted Whisper GGML model files are downloaded from the `ggerganov/whisper.cpp` Hugging Face repository, which identifies the repository license as MIT. Model license and attribution requirements must be reviewed again before distribution.
