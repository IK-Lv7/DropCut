# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's private reporting: *Security → Report a vulnerability* on this repository. Include the affected version, steps to reproduce, and the impact you see. You can expect an acknowledgement within a few days.

## Scope and design

DropCut processes files locally. Security-relevant areas:

- Handling of untrusted media files passed to FFmpeg, whisper.cpp and llama.cpp (arguments are always passed as arrays, never through a shell).
- Validation of paths, job IDs, language codes and model files coming from the UI.
- Model downloads: fixed official URLs, size limit, SHA-256 verification, atomic rename.
- The webview runs with a strict Content Security Policy and only the minimal Tauri permissions (file dialog).

Vulnerabilities in bundled third-party tools should also be reported upstream.
