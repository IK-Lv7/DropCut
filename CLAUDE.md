# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

DropCut: a free, fully local, one-button AI video editing desktop app (Tauri 2 + React 19/TypeScript/Vite frontend, Rust backend). Windows is the priority platform. `AGENTS.md` (in Japanese) holds the full product spec, phase roadmap, and non-negotiable rules; read it before adding features. Key constraints from it:

- No cloud/paid APIs, no uploads, no telemetry; everything runs locally (only model downloads may touch the network).
- Never overwrite the source video: write `<name>_edited.mp4` beside it via a temp file that is renamed only after verification.
- Spawn FFmpeg/whisper with argument arrays, never concatenated shell strings; validate all external input (paths, job IDs, model URLs).
- Don't show technical errors to users (map through `friendly_error`); support cancel and clean up temp files; don't log subtitle text.
- Don't hardcode the product name (it may change); keep the UI beginner-simple (presets like weak/normal/strong, advanced settings hidden by default).
- Phases: 1 export/compression, 2 Whisper subtitles, 3 silence/normalize/noise, 4 filler words and text-based editing, 5 face detection and 9:16 reframe, 6 auto zoom/subtitle templates/Shorts preset, 7 highlight detection + bundled local LLM re-ranking are implemented.

## Commands

```bash
npm install
npm run tauri dev          # full desktop app (needed for import/export)
npm run dev                # frontend only in browser (Tauri invoke calls will not work)
npm run build              # tsc + vite build (frontend type check is part of this)
cd src-tauri && cargo test --locked                # Rust tests (in lib.rs `mod tests`)
cd src-tauri && cargo test <name>                  # single Rust test
cd src-tauri && cargo test -- --ignored            # manual checks that need FFmpeg/whisper output; each documents its DROPCUT_TEST_* env vars
./scripts/prepare-windows-tools.ps1                # Windows: fetch pinned FFmpeg, build whisper.cpp into src-tauri/resources/tools
npm run tauri build -- --bundles nsis              # Windows installer
```

No linter or frontend test runner is configured. CI is `.github/workflows/windows.yml` (frontend build, `cargo test`, tool prep, NSIS build).

## Architecture

- **Frontend** ([src/](src/)): a single-screen `App.tsx` plus `types.ts` (mirrors Rust serde structs) and `presets.ts` (destination presets). It talks to Rust only via `invoke` commands and listens to the `export-progress` and `model-download-progress` events. File drop uses Tauri's webview drag-drop event, not HTML5 DnD. When you change a settings struct in Rust, update `types.ts` and the `start_export` payload in `App.tsx` together.
- **Backend** ([src-tauri/src/](src-tauri/src/)):
  - `lib.rs` (~1700 lines): all commands (registered in `run()` via `generate_handler!`), the export pipeline, and tests. `start_export` registers the job and calls `run_export`, which orchestrates: validate input, probe, optional silence detection (`detect_silence` -> `build_keep_ranges`), user cuts (`subtract_cuts`), optional face tracking, optional loudness analysis (two-pass `loudnorm`, with the noise filter applied first so measurements match the export), optional Whisper subtitles (`create_subtitles`, run on the already-trimmed audio timeline so SRT stays in sync), then `build_video_args` for the final FFmpeg encode. Jobs are tracked in `JobManager` (job id -> cancel `AtomicBool`); `run_cancelable` runs child processes and kills them on cancel.
  - `transcript.rs`: parses `whisper-cli -ojf` token JSON into words (handles UTF-8 characters split across tokens), flags filler words (`high` = hesitation, preselected; `low` = ambiguous, left for review), and holds `subtract_cuts`, which removes user-chosen ranges from the silence keep-ranges. `analyze_transcript` is a separate command from export: the UI analyzes first, the user edits, then sends `cutRanges` in the export settings.
  - `reframe.rs`: samples frames through an FFmpeg pipe, detects faces with embedded `rustface` (model in `resources/models/`, compiled in via `include_bytes!`), smooths the path, and emits a `sendcmd` script that steers a `crop@rf` filter. That filter must run on the source timeline before `trim`/`concat`, which is why `build_video_args` splits the reframed stream per keep-range.
  - `subtitles.rs`: converts whisper's SRT into a styled ASS file laid out for the real output size (`output_dimensions` in `lib.rs`), with width-aware wrapping. The ASS file is both the burn-in source and a saved output. Templates live in `template()`.
  - `highlight.rs`: scores each second (speech density, loudness, keywords), picks non-overlapping windows, snaps to sentence edges. `find_highlights` command transcribes then scores (optionally re-ranks with `llm.rs`: bundled `llama-completion` + Qwen GGUF in `resources/llm/`, one call per candidate, ChatML prompt, falls back to heuristics if unavailable); the UI turns picked clips into `cutRanges` (the complement), so export needs no new path.
- Auto zoom is `auto_zoom_filter` in `lib.rs`: one closed-form `zoompan` expression, no keyframes. `build_video_args` takes an `EditPlan`; reframe and zoom filters are joined into a source-timeline chain that runs before `trim`/`concat`.
  - `tools.rs`: resolves `ffmpeg`/`ffprobe`/`whisper-cli`, bundled `resources/tools` first, then `PATH`.
  - `models.rs`: Whisper GGML model catalogue and downloader (fixed official URL, streamed to `.part`, size-limited, SHA-256 verified, then renamed).
- Subtitle burn-in needs an FFmpeg build with libass (`subtitles` filter); the app detects this and falls back to SRT-only output.
- Bundling: `tauri.conf.json` ships `resources/tools/` and `resources/licenses/`; the tools are git-ignored and generated by the prepare script. Update `THIRD_PARTY_NOTICES.md` when adding bundled software or models.
