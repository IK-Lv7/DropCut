# DropCut

Drop a video. Let your PC cut it.

- 100% free, no subscription, no watermark
- No upload: everything runs on your computer
- No accounts, no telemetry

DropCut is a local-first desktop video editor. Choose a video, pick where it is going (YouTube, Shorts/TikTok, Discord, X, …) and press one button. Advanced settings stay hidden until you want them.

> **Status:** early development (v0.1). Windows is the primary target; the app is built with Tauri and also runs on Linux and macOS with your own FFmpeg. Expect rough edges and please report them.

## Features

| | |
|---|---|
| Export & compress | MP4 export with destination presets, size targets, cancel, and crash-safe output |
| Automatic subtitles | Local Whisper transcription, SRT + styled ASS files, optional burn-in, six templates |
| Cleanup | Silence removal, loudness normalization, noise reduction |
| Edit by text | Review the transcript, remove filler words or sentences, and the video follows |
| Vertical video | Face-aware 9:16 reframing, gentle auto zoom, one-click "Make it a Short" |
| Highlights | Finds the most engaging clips of a long video; a bundled small local AI model re-ranks them |

Details and limitations of each feature: [docs/HOW_IT_WORKS.md](docs/HOW_IT_WORKS.md).

## Privacy and safety

- Videos, audio and transcripts never leave your PC. The only network access is the optional download of Whisper models from the official whisper.cpp repository (checksum-verified; no video data is sent).
- Your source file is never modified. Output is written next to it as `<name>_edited.mp4`, via a temporary file that is renamed only after verification.
- External programs (FFmpeg, whisper.cpp, llama.cpp) are started with argument lists, never shell strings, and all paths and IDs from the UI are validated.
- Transcript and subtitle text are never written to logs.

Found a vulnerability? See [SECURITY.md](SECURITY.md).

## Download

Windows installers are built by GitHub Actions (see the *Windows build* workflow). The installer is not code-signed yet, so Windows SmartScreen may warn on first launch. It bundles FFmpeg, whisper.cpp, llama.cpp and a ~1.1 GB local AI model; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Development

Requirements: Node.js 20+, Rust stable, the [Tauri prerequisites](https://tauri.app/start/prerequisites/), and FFmpeg/FFprobe on `PATH` (plus `whisper-cli` for subtitles).

```bash
npm install
npm run tauri dev            # full desktop app
npm run build                # frontend type check + build
cd src-tauri && cargo test --locked
```

Windows release build (downloads checksum-pinned tools, builds whisper.cpp):

```powershell
./scripts/prepare-windows-tools.ps1
npm run tauri build -- --bundles nsis
```

Tools are resolved from the bundled `resources/tools` directory first, then from `PATH`. Architecture notes for contributors are in [CLAUDE.md](CLAUDE.md); the product spec and roadmap are in [AGENTS.md](AGENTS.md) (Japanese). See [CONTRIBUTING.md](CONTRIBUTING.md) to get involved.

## License

DropCut is released under the [MIT License](LICENSE). It bundles and calls third-party software under their own licenses, including a GPL build of FFmpeg; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
