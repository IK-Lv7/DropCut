# How DropCut works

Technical notes on each feature and on what happens to your files.

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
