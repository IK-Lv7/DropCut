# DropCut

Drop a video. Let your PC cut it.

- 100% Free
- No Subscription
- No Watermark
- No Upload
- Runs Locally

DropCut is a local-first video editing app: choose a video, select its destination, and press one button. Phase 1 includes video import, FFmpeg-powered MP4 export, destination presets, custom encoding settings, progress reporting, cancellation, and crash-safe output that never overwrites the source. Phase 2 adds optional, fully local SRT generation through whisper.cpp. Phase 3 adds automatic silence removal, audio normalization, and noise reduction. Phase 4 adds filler-word detection with a reviewable, editable transcript. Phase 5 adds face-aware 9:16 reframing. Phase 6 adds auto zoom, styled subtitle templates, and a one-click Shorts preset.

## Development requirements

- Node.js 20 or later
- Rust stable
- FFmpeg and FFprobe available on `PATH`
- `whisper-cli` on `PATH` when automatic subtitles are enabled
- The platform-specific Tauri prerequisites

## Development

```bash
npm install
npm run tauri dev
```

Use `npm run dev` to inspect the frontend only. Video import and export require the Tauri desktop runtime.

DropCut resolves processing tools from its bundled `tools` resource directory first, then falls back to `PATH`. This keeps local development lightweight while allowing Windows release packages to work without a separate FFmpeg or whisper.cpp installation.

## Windows release build

The release preparation script downloads the pinned BtbN FFmpeg 8.1.3 archive, verifies its SHA-256 checksum, and builds the pinned whisper.cpp v1.9.4 source locally. It places only the required executables and upstream licenses in Tauri's resource directories.

```powershell
./scripts/prepare-windows-tools.ps1
npm run tauri build -- --bundles nsis
```

GitHub Actions runs the same preparation and creates an NSIS installer artifact. Whisper models remain optional downloads so the installer does not include several gigabytes of model data.

## Safety and privacy

DropCut never uploads videos. It leaves the source untouched and writes `source_name_edited.mp4` beside it. Processing uses a temporary file that is renamed only after export verification succeeds.

When automatic subtitles are enabled, audio is converted to a temporary 16 kHz mono WAV file and passed directly to the local `whisper-cli` process. The temporary audio is removed after transcription. The generated SRT is saved beside the exported video and can optionally be burned into the video.

The model manager can download multilingual Tiny, Base, Small, and Medium GGML models from the fixed official whisper.cpp repository. Downloads are streamed into a `.part` file, limited to the expected size, verified with SHA-256, and renamed only after verification succeeds. No video, audio, subtitle text, or local path is included in a model request.

Subtitle burn-in requires an FFmpeg build with the `subtitles` filter provided by libass. DropCut detects this capability and falls back to SRT-only output when the filter is unavailable.

Silence removal uses FFmpeg's local `silencedetect` filter. Weak, Normal, and Strong presets provide beginner-friendly defaults, while Advanced settings expose the threshold, minimum duration, and retained padding. DropCut trims video and audio from the same detected ranges. When subtitles are also enabled, Whisper receives the already-trimmed audio timeline so the resulting SRT remains synchronized with the edited video.

Noise reduction uses FFmpeg's `afftdn` denoiser (with a high-pass filter at Normal and Strong) ahead of loudness normalization, so the loudness measurement matches what is exported.

Filler words and text editing start with an "Analyze speech" step. whisper-cli returns token-level timestamps, DropCut groups them into words, and hesitations such as えーと, うーん, um, and uh are pre-selected for removal. Words that are often real speech (あの, その, like, you know) are highlighted but left unselected. Clicking any word removes or restores it, and the corresponding video and audio ranges are cut from the same timestamps, combined with silence removal when both are enabled. Transcript text is shown only in the app and is never logged or written to disk.

Vertical 9:16 conversion samples about four frames per second, finds faces with the embedded SeetaFace detector (pure Rust, no network), and smooths the result with a dead zone and a speed limit so the crop never jumps. FFmpeg then follows that path with a `sendcmd`-driven `crop`, before any silence or transcript cuts. Landscape frames without a detected face use a centered crop, and videos that are already vertical are left alone. Only frontal faces are detected in this first version.

Subtitle templates (Simple, YouTube, TikTok, Gaming, Minimal, Pop) are rendered through an ASS file generated for the real output resolution, so text size, margins, and line breaks match the exported frame. Long lines are wrapped by character width (never starting a line with closing punctuation), and Advanced settings expose the font, size, stroke, shadow, position, background box, and line length. The styled `.ass` file is saved next to the video together with the SRT.

Auto zoom applies a light, repeating push-in with FFmpeg's `zoompan` on the source timeline, so it stays steady across cuts. Natural, YouTube, and Shorts styles peak at about 5%, 8%, and 12%; the zoom always returns to 100% between pushes.

"Make it a Short" turns on vertical 9:16 framing with face tracking, silence trimming, noise reduction, loudness normalization, Shorts-style zoom, and TikTok-style captions when a Whisper model is installed. Filler-word cutting stays a separate, reviewed step.

## Highlights

"Find highlights" transcribes locally, scores speech density, loudness and keywords, and proposes the most engaging clips. Checked clips are joined on export. The installer also bundles llama.cpp and a small Qwen2.5 model that re-ranks the candidates and suggests titles, entirely offline. If it is missing, the signal-based ranking is used.
