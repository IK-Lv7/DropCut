<h1 align="center">DropCut</h1>
<p align="center"><b>Drop a video. Let your PC cut it.</b></p>

<p align="center">
  <a href="https://github.com/IK-Lv7/DropCut/releases/latest"><img alt="Download for Windows" src="https://img.shields.io/badge/%E2%AC%87%20Download%20for%20Windows-installer-2ea44f?style=for-the-badge&logo=windows&logoColor=white"></a>
</p>
<p align="center"><sub>Free · No account · Runs offline · Windows 10/11 (64-bit)</sub></p>

<p align="center">A free, open-source AI video editor that runs 100% on your computer.<br>
No subscription. No watermark. No upload. No account.</p>

<p align="center">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <a href="https://github.com/IK-Lv7/DropCut/actions/workflows/windows.yml"><img alt="Windows build" src="https://github.com/IK-Lv7/DropCut/actions/workflows/windows.yml/badge.svg"></a>
  <img alt="Platform: Windows" src="https://img.shields.io/badge/platform-Windows-0078D6.svg">
  <img alt="Built with Tauri, React and Rust" src="https://img.shields.io/badge/built%20with-Tauri%20%C2%B7%20React%20%C2%B7%20Rust-orange.svg">
</p>

<p align="center"><a href="#日本語">日本語</a> · <a href="#features">Features</a> · <a href="#why-dropcut">Why DropCut</a> · <a href="#get-started">Get started</a> · <a href="#roadmap">Roadmap</a></p>

---

Most "AI video tools" ask you to upload your footage, pay monthly, and export with a watermark. DropCut does the boring editing on your own PC instead: subtitles, silence and filler-word cutting, vertical Shorts, highlight clips. Your videos never leave your machine.

> ⭐ If DropCut saves you time, a star helps other people find it.

> **Status:** early development (v0.1). Windows is the main target. Expect rough edges, and please [open an issue](https://github.com/IK-Lv7/DropCut/issues) when you hit one.

## Features

- **One-button export**: pick where the video is going (YouTube, Shorts/TikTok, Discord, X…) and get the right size, quality and file size.
- **Automatic subtitles**: local Whisper speech recognition (Japanese, English and more). Six styles (Simple, YouTube, TikTok, Gaming, Minimal, Pop), burned in or saved as `.srt` / `.ass`.
- **Make it a Short**: one click for vertical 9:16 with face-following crop, gentle zoom, trimmed pauses, cleaner audio and captions.
- **Edit by text**: read the transcript, click the filler words ("um", "えーと") or sentences you don't want, and the video is cut to match.
- **Find highlights**: pulls the most engaging clips out of a long video. A small AI model bundled with the app ranks them and suggests titles, offline.
- **Audio cleanup**: silence removal, loudness normalization, noise reduction.
- **Safe by design**: your original file is never touched; output is `<name>_edited.mp4` next to it.

<!-- Add a demo GIF here: assets/demo.gif -->

## Why DropCut

| | DropCut | Typical online AI editors |
|---|---|---|
| Price | Free, forever | Subscription |
| Watermark | None | On the free tier |
| Your video is uploaded | **Never** | Always |
| Works offline | Yes (after model download) | No |
| Account required | No | Yes |
| Source code | Open (MIT) | Closed |

## Get started

<p align="center">
  <a href="https://github.com/IK-Lv7/DropCut/releases/latest"><img alt="Download for Windows" src="https://img.shields.io/badge/%E2%AC%87%20Download%20for%20Windows-installer-2ea44f?style=for-the-badge&logo=windows&logoColor=white"></a>
</p>

1. Click **Download**, then grab `DropCut_x.y.z_x64-setup.exe` from the latest release.
2. Run the installer. If SmartScreen warns you, click **More info → Run anyway** (the installer is not code-signed yet).
3. Open DropCut and drop a video onto the window.

**Windows:** installers are built by GitHub Actions; see the [*Windows build*](https://github.com/IK-Lv7/DropCut/actions/workflows/windows.yml) workflow. The installer is not code-signed yet, so SmartScreen may warn on first launch. It bundles FFmpeg, whisper.cpp, llama.cpp and a ~1.1 GB local AI model ([notices](THIRD_PARTY_NOTICES.md)).

**Build from source:** you need Node.js 20+, Rust stable, the [Tauri prerequisites](https://tauri.app/start/prerequisites/), and FFmpeg/FFprobe on `PATH` (plus `whisper-cli` for subtitles).

```bash
git clone https://github.com/IK-Lv7/DropCut.git
cd DropCut
npm install
npm run tauri dev
```

Tests: `npm run build` and `cd src-tauri && cargo test --locked`. Windows release build:

```powershell
./scripts/prepare-windows-tools.ps1
npm run tauri build -- --bundles nsis
```

## Privacy

- Videos, audio and transcripts never leave your PC. The only network use is the optional Whisper model download from the official whisper.cpp repository (size-limited and SHA-256 verified; no video data is sent).
- No telemetry, no accounts, no analytics.
- Subtitle and transcript text is never written to logs.

Security issues: see [SECURITY.md](SECURITY.md). How each feature works: [docs/HOW_IT_WORKS.md](docs/HOW_IT_WORKS.md).

## Roadmap

Done: export and presets · Whisper subtitles · silence / loudness / noise · filler words and text editing · face-aware 9:16 · auto zoom, subtitle templates, Shorts preset · highlights with a local AI model.

Ideas: speaker-aware subtitles, scene-change aware highlights, code signing, auto update. Suggestions are welcome in [Issues](https://github.com/IK-Lv7/DropCut/issues).

## Contributing

Bug reports, ideas and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md). Architecture notes are in [CLAUDE.md](CLAUDE.md) and the product spec is [AGENTS.md](AGENTS.md) (Japanese).

## License

[MIT](LICENSE). DropCut bundles and calls third-party software under their own licenses, including a GPL build of FFmpeg; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

---

## 日本語

**動画をドロップするだけ。編集はPCがやります。**

DropCut は、完全にローカルで動く無料のオープンソース AI 動画編集アプリです。サブスクなし、ウォーターマークなし、アップロードなし、アカウントなし。動画は PC の外に出ません。

<p align="center">
  <a href="https://github.com/IK-Lv7/DropCut/releases/latest"><img alt="Windows 版をダウンロード" src="https://img.shields.io/badge/%E2%AC%87%20Windows%E7%89%88%E3%82%92%E3%83%80%E3%82%A6%E3%83%B3%E3%83%AD%E3%83%BC%E3%83%89-installer-2ea44f?style=for-the-badge&logo=windows&logoColor=white"></a>
</p>

**インストール方法:** 上のボタンから最新の `setup.exe` をダウンロードして実行。SmartScreen の警告が出たら「詳細情報 → 実行」を選んでください（まだコード署名していません）。

- **ワンボタン書き出し**: YouTube、Shorts/TikTok、Discord、X など用途に合わせて出力
- **自動字幕**: ローカルの Whisper で日本語・英語などに対応。6種類のデザイン
- **Make it a Short**: 縦動画化（顔追従）、ズーム、無音カット、音声補正、字幕をワンクリックで
- **文字で編集**: 書き起こしから「えーと」や不要な文をクリックで削除。動画も同じ箇所が切れます
- **ハイライト抽出**: 長い動画から盛り上がる部分を提案。同梱の小型 AI がオフラインで並べ替え
- **元の動画は上書きしません**（`名前_edited.mp4` として横に保存）

Windows 向けに開発中の初期版（v0.1）です。不具合は [Issues](https://github.com/IK-Lv7/DropCut/issues) へ。気に入ったら ⭐ をいただけると励みになります。
