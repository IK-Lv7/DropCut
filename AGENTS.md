# AGENT.md

## Project Overview

このプロジェクトは、**完全無料・完全ローカルで動作するAI動画編集アプリ**を開発する。

主な目的は、CapCut Pro、Descript、Adobe Premiere Pro などの有料動画編集ツールで提供されている便利な自動編集機能のうち、ローカルPC上だけで実現可能なものを、誰でも簡単に無料で利用できる形で提供することである。

本アプリは従来型の高機能動画編集ソフトを目指さない。

Adobe Premiere Pro や DaVinci Resolve のような複雑なタイムライン編集ではなく、

> 「動画を入れて、やりたい処理を選び、ボタンを押すだけ」

というシンプルなAI動画編集ツールを目指す。

---

# 1. Core Principles

開発時は以下を最優先する。

## 1.1 完全無料

ユーザーに対して、

* サブスクリプション
* 買い切り課金
* クレジット制
* 使用回数制限
* 動画時間制限
* ウォーターマーク
* 広告

を原則として設けない。

基本機能はすべて無料で提供する。

---

## 1.2 完全ローカル処理

動画・画像・音声・字幕などのユーザーデータは、原則として外部サーバーへ送信しない。

すべてユーザーのPC上で処理する。

禁止事項：

* 動画ファイルのクラウドアップロード
* 音声データの外部AI API送信
* OpenAI API等の有料APIへの依存
* 外部推論サーバーへの依存
* 開発者管理サーバーでの動画処理

例外を導入する場合でも、ローカル機能とは完全に分離し、ユーザーが明示的に有効化しない限り利用しないこと。

---

# 2. Cost Policy

開発者側で継続的なサーバー費用が発生する設計は禁止する。

理想：

```text
User PC
   │
   ├─ FFmpeg
   ├─ Whisper
   ├─ ONNX Runtime
   ├─ OpenCV
   ├─ Local AI Models
   │
   └─ Export Video
```

サーバー：

```text
基本的に不要
```

必要になる可能性があるもの：

* GitHub
* GitHub Releases
* GitHub Actions
* 静的Webサイト
* アップデート確認

可能な限り無料枠または静的配信を利用する。

---

# 3. Primary Platform

初期リリースでは **Windowsを最優先** とする。

優先順位：

1. Windows
2. macOS
3. Linux

Windows版完成前にマルチプラットフォーム対応を理由として開発を複雑化しない。

---

# 4. Target User

主な対象：

* TikTok投稿者
* YouTube Shorts投稿者
* YouTuber
* VTuber
* 配信者
* Discordユーザー
* 学生
* 動画編集初心者
* SNS投稿者
* ゲームクリップ投稿者

特に、

> 「Premiereを覚えるほどではないが、動画を簡単に編集したい」

ユーザーを中心とする。

---

# 5. Main UX

アプリ起動後、最初に表示するUIはできる限りシンプルにする。

例：

```text
┌───────────────────────────────────────┐
│                                       │
│          動画をここにドロップ          │
│                                       │
│              + 動画を選択              │
│                                       │
└───────────────────────────────────────┘
```

動画追加後：

```text
動画：sample.mp4

編集

☑ 自動字幕
☑ 無音部分を削除
☐ フィラーワード削除
☑ ノイズ除去
☑ 音量を自動調整
☐ 縦動画に変換

出力

○ 元動画と同じ
● YouTube
○ YouTube Shorts
○ TikTok
○ Discord
○ Custom

             [ 動画を作成 ]
```

複雑なタイムラインUIは初期段階では作らない。

---

# 6. MVP

最初のリリースでは以下を実装する。

## 6.1 動画読み込み

対応：

* MP4
* MOV
* WebM
* MKV
* AVI

内部処理はFFmpegを中心にする。

---

## 6.2 動画圧縮

ユーザーが用途を選択できるようにする。

プリセット：

```text
Discord
TikTok
YouTube
YouTube Shorts
X
Instagram
Custom
```

ユーザーは、

* 最大ファイルサイズ
* 解像度
* FPS
* Codec
* Bitrate

を必要に応じて変更可能。

ただし通常モードでは細かい設定を隠す。

---

## 6.3 自動字幕

ローカルWhisperを使用する。

候補：

* whisper.cpp
* faster-whisper
* ONNX Whisper

クラウドAPIは禁止。

機能：

* 音声認識
* 字幕タイミング生成
* SRT出力
* ASS出力
* 動画への字幕焼き込み

日本語を最優先とする。

将来的に多言語対応する。

---

# 7. Silence Removal

音声の無音区間を検出し、自動カットする。

ユーザー設定：

```text
無音判定：-35 dB
最小無音時間：0.5秒
前後余白：0.15秒
```

ただし通常UIでは、

```text
弱い
普通
強い
```

程度の選択肢を提供する。

Advanced Settingsで詳細設定可能にする。

---

# 8. Filler Word Removal

文字起こし結果から、

日本語：

```text
えー
えっと
あの
その
まあ
うーん
```

英語：

```text
um
uh
you know
like
```

などを検出する。

該当部分の動画・音声を自動カット可能にする。

重要：

字幕文字列だけを削除するのではなく、対応するタイムスタンプから動画を編集する。

ユーザーは削除候補を確認可能にする。

---

# 9. Noise Reduction

音声ノイズ除去をローカル処理する。

候補技術：

* RNNoise
* DeepFilterNet
* FFmpeg Audio Filters

優先順位：

1. DeepFilterNet
2. RNNoise
3. FFmpeg

モデルサイズと処理速度も考慮する。

---

# 10. Audio Normalization

動画間・区間間の音量差を軽減する。

FFmpegの、

```text
loudnorm
```

などを使用する。

目標：

* 急に音量が大きくならない
* 急に声が小さくならない
* SNS投稿に適した音量

---

# 11. Automatic Reframe

横動画を、

```text
16:9
↓
9:16
```

へ変換する。

単純中央クロップだけではなく、人物検出・顔検出を利用して被写体を追跡する。

候補：

* MediaPipe
* YOLO
* OpenCV
* ONNX Runtime

動画全体でクロップ位置が急激に移動しないように、トラッキング結果を平滑化する。

---

# 12. Face Tracking

人物の顔を認識し、

* 自動リフレーム
* 自動ズーム
* 顔中心クロップ

に利用する。

初期段階では顔識別は不要。

「誰なのか」を判定する必要はない。

必要なのは、

```text
顔がどこに存在するか
```

のみ。

---

# 13. Auto Zoom

Shorts/TikTok向けに、発話や一定時間ごとに軽いズームを入れられるようにする。

例：

```text
100%
↓
108%
↓
100%
↓
112%
```

過剰なズームは禁止。

テンプレート：

```text
なし
自然
YouTube
Shorts
```

---

# 14. Subtitle Styling

字幕デザインを複数提供する。

例：

```text
Simple
YouTube
TikTok
Gaming
Minimal
Pop
```

変更可能：

* Font
* Font Size
* Stroke
* Shadow
* Position
* Background
* Maximum Characters Per Line

可能な限りFFmpeg ASSを利用する。

---

# 15. Speaker-Aware Subtitles

将来的に話者分離を追加する。

例：

```text
Person A:
こんにちは

Person B:
こんにちは！
```

ただしMVPには必須ではない。

---

# 16. Automatic Highlight Extraction

将来的に長い動画から、

```text
60分
↓
重要部分
↓
1分
```

のようなハイライト生成を実装する。

可能な限りローカルモデルを使用する。

分析対象：

* 音声
* 字幕
* 音量変化
* 発話密度
* シーン変化
* キーワード

ローカルLLMを使用する場合は、

* llama.cpp
* ONNX Runtime
* GGUF

などを検討する。

---

# 17. Text-Based Video Editing

文字起こし結果を編集すると、動画も編集される方式を将来的に導入する。

例：

```text
今日は東京に行ってきました。
めちゃくちゃ暑かったです。
そのあと渋谷に行きました。
```

ユーザーが、

```text
めちゃくちゃ暑かったです。
```

を削除すると、対応する映像区間も削除される。

Descript型編集を目標とする。

---

# 18. Batch Processing

複数ファイルの一括処理を対応する。

例：

```text
video1.mp4
video2.mp4
video3.mp4
video4.mp4
```

↓

```text
全動画
・字幕
・ノイズ除去
・音量調整
・圧縮
```

を一括適用。

---

# 19. Hardware Acceleration

可能であれば、

* NVIDIA NVENC
* Intel Quick Sync
* AMD AMF
* Apple VideoToolbox

を利用する。

Windows初期版では、

```text
NVIDIA
Intel
AMD
CPU
```

を自動検出する。

ユーザーに複雑なGPU設定を要求しない。

---

# 20. Technology Stack

推奨構成：

## Desktop

候補：

```text
Tauri
```

を第一候補とする。

理由：

* Electronより軽量
* メモリ消費が少ない
* Windowsアプリとして配布しやすい

ただし開発効率を優先する場合、

```text
Electron
```

も許可する。

---

## Frontend

候補：

```text
React
TypeScript
Vite
```

---

## Media Processing

必須：

```text
FFmpeg
FFprobe
```

---

## AI

候補：

```text
whisper.cpp
faster-whisper
ONNX Runtime
MediaPipe
OpenCV
DeepFilterNet
YOLO
llama.cpp
```

---

# 21. No Paid API Rule

以下のサービスへの必須依存は禁止：

```text
OpenAI API
Google Gemini API
Anthropic API
AWS AI
Azure AI
Replicate
Cloudinary AI
ElevenLabs API
```

ユーザー自身がAPIキーを設定する任意機能として追加する場合は検討可能。

ただし、

> APIなしでもアプリの主要機能がすべて利用可能

でなければならない。

---

# 22. Offline First

インターネット接続なしでも、

* 動画編集
* 字幕生成
* 圧縮
* ノイズ除去
* 音量調整
* 自動カット

を実行できる設計にする。

初回AIモデルダウンロードのみインターネットを必要とする設計は許容する。

可能ならモデル同梱版も検討する。

---

# 23. Privacy

ユーザーのファイルを勝手に収集しない。

禁止：

* 動画アップロード
* 音声アップロード
* 字幕本文収集
* 顔画像収集
* 編集内容収集

Telemetryを使用する場合も、

* 明示する
* 匿名化する
* OFF可能にする

こと。

初期版ではTelemetryなしでもよい。

---

# 24. File Safety

元動画を直接編集・上書きしてはならない。

原則：

```text
input.mp4

↓

input_edited.mp4
```

とする。

ユーザーが明示的に許可した場合のみ上書きを許可する。

---

# 25. Crash Safety

長時間動画処理中にアプリが落ちても元ファイルを破損させない。

処理は、

```text
temporary file
↓
processing
↓
verify
↓
final output
```

の流れにする。

---

# 26. Progress UI

AI処理は時間がかかるため、

```text
音声抽出
████████████ 100%

文字起こし
████████░░░░ 68%

字幕生成
□□□□□□□□□□□□

動画書き出し
□□□□□□□□□□□□
```

のように現在の処理を表示する。

単純な無限Loadingのみは禁止。

---

# 27. Cancel Processing

ユーザーは動画処理を途中キャンセルできる。

キャンセル時は、

* FFmpeg
* Python
* AI process

など関連プロセスを正常終了する。

temporary filesも可能な限り削除する。

---

# 28. Model Management

AIモデルをアプリ本体にすべて含めると容量が巨大になるため、モデル管理機能を作る。

例：

```text
Whisper Tiny
75 MB
高速

Whisper Base
145 MB
おすすめ

Whisper Small
466 MB
高精度

Whisper Medium
1.5 GB
最高精度
```

ユーザーが必要なモデルだけダウンロードできるようにする。

---

# 29. Recommended Defaults

初心者に細かい設定を要求しない。

例：

```text
Whisper Model:
Automatic

GPU:
Automatic

Video Encoder:
Automatic

Noise Reduction:
Normal
```

Advanced Settingsを開いた場合のみ詳細表示する。

---

# 30. Export Presets

## YouTube

```text
H.264 / H.265
1080p
AAC
```

## Shorts

```text
1080x1920
9:16
H.264
AAC
```

## TikTok

```text
1080x1920
9:16
H.264
AAC
```

## Discord

ファイルサイズを指定：

```text
10 MB
25 MB
50 MB
100 MB
Custom
```

指定容量以下になるようBitrateを計算する。

---

# 31. Development Priorities

以下の順番で実装する。

## Phase 1

```text
Video Import
FFmpeg Integration
Video Export
Compression
Basic UI
```

## Phase 2

```text
Whisper
Automatic Subtitles
SRT Export
Subtitle Burn-in
```

## Phase 3

```text
Silence Detection
Silence Removal
Audio Normalization
Noise Reduction
```

## Phase 4

```text
Filler Word Detection
Text-Based Editing
```

## Phase 5

```text
Face Detection
Auto Reframe
9:16 Conversion
```

## Phase 6

```text
Auto Zoom
Subtitle Templates
Shorts Automation
```

## Phase 7

```text
Highlight Detection
Local LLM
Automatic Shorts Creation
```

---

# 32. Do Not Build Too Early

以下は初期版では作らない。

```text
Full Premiere-like timeline
3D effects
Professional color grading
After Effects-like compositions
Cloud collaboration
Cloud storage
User accounts
Social network
Plugin marketplace
Online project synchronization
```

これらは開発を大幅に複雑化する。

---

# 33. UI Philosophy

UIの目標：

```text
Canva / CapCutレベルのわかりやすさ
+
FFmpegレベルの自由度
```

ただしFFmpegの複雑さをユーザーには見せない。

悪い例：

```text
CRF
Preset
Profile
Level
B-frame
GOP
VBV
```

通常ユーザーには表示しない。

良い例：

```text
画質

○ 小さいファイル
● おすすめ
○ 高画質
```

---

# 34. AI Feature Rule

「AI」という言葉を使うためだけの機能を追加しない。

必ず、

```text
ユーザーの編集時間を短縮する
```

機能でなければならない。

---

# 35. Performance

動画処理ではコピーを可能な限り避ける。

利用可能な場合は、

```text
stream copy
```

を利用する。

AI処理でも動画全体を毎回デコードするのではなく、必要な音声・フレームのみ処理する。

---

# 36. Temporary Files

Temporary directoryを管理する。

例：

```text
AppData/Local/<APP>/temp/
```

処理終了後に不要ファイルを削除する。

アプリ終了時にも残存tempをチェックする。

---

# 37. Logging

開発時ログ：

```text
FFmpeg command
FFmpeg stderr
AI model
GPU
processing time
errors
```

を記録する。

ただしユーザーの字幕本文などプライベートなデータはログへ書かない。

---

# 38. Error Messages

技術的なエラーをそのまま表示しない。

悪い例：

```text
FFmpeg exited with code -1073741819
```

良い例：

```text
動画の書き出しに失敗しました。

GPUエンコードに失敗したため、
CPUエンコードで再試行できます。

[CPUで再試行]
```

Advanced detailsからログを確認可能にする。

---

# 39. Automatic Fallback

GPU処理に失敗した場合：

```text
NVENC
↓
失敗
↓
CPU
```

など自動Fallbackを行う。

---

# 40. Open Source

可能であればGitHubでOSSとして公開する。

メリット：

* 信頼性
* ローカル処理であることを確認可能
* コントリビューター獲得
* AIモデル統合の改善

READMEでは、

```text
100% Free
No Subscription
No Watermark
No Upload
Runs Locally
```

を強調する。

---

# 41. Licensing

利用するOSSライブラリ・AIモデルのライセンスを必ず確認する。

特に、

```text
FFmpeg
AI Models
Fonts
Icons
Pretrained Models
```

について、

* 商用利用
* 再配布
* モデル同梱

の可否を確認する。

LICENSES / THIRD_PARTY_NOTICESを用意する。

---

# 42. Security

動画・字幕・モデルなど外部入力は信頼しない。

注意するもの：

```text
malformed media
path traversal
command injection
FFmpeg arguments
model download URLs
ZIP extraction
```

FFmpeg commandは文字列連結ではなく安全なargument配列で実行する。

---

# 43. Update System

将来的に自動更新を導入する。

候補：

```text
GitHub Releases
Tauri Updater
```

アカウント登録は不要。

---

# 44. Possible Product Name

仮称：

```text
DropCut
```

コンセプト：

```text
Drop a video.
Let AI cut it.
```

ただし正式名称は後から変更可能な構造にする。

コード内に製品名をハードコードしすぎない。

---

# 45. Final Product Goal

最終的にユーザーが、

```text
動画を入れる
↓
「Shortsにする」
↓
AIが処理
↓
完成
```

だけで動画を作れる状態を目標とする。

内部では、

```text
Whisper
FFmpeg
Face Detection
Noise Reduction
Audio Analysis
Local LLM
```

など複数技術を利用するが、ユーザーにその複雑さを見せない。

---

# 46. Most Important Rule

機能を追加する際は必ず以下を確認する。

```text
1. 完全ローカルで実行できるか？
2. 開発者側の継続費用は発生しないか？
3. 無料でユーザーに提供できるか？
4. 本当に動画編集時間を短縮するか？
5. 初心者でも迷わず使用できるか？
```

1〜3を満たさない機能は原則として実装しない。

本プロジェクトの最大の価値は、

> 有料AI動画編集サービスで提供されている便利な処理を、
> ユーザー自身のPC性能を利用して、
> 無料・無制限・プライベートに提供すること。

である。
