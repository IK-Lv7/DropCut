# Bundled tools

Release builds place `ffmpeg.exe`, `ffprobe.exe`, and `whisper-cli.exe` in this directory before Tauri packaging starts. These generated binaries are intentionally not committed.

Run `scripts/prepare-windows-tools.ps1` from the repository root to reproduce them. DropCut checks bundled tools first and falls back to executables on `PATH` during development.
