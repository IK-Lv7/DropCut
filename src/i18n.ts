import { useCallback, useEffect, useState } from "react";

export type Lang = "en" | "ja";

const STORAGE_KEY = "dropcut-language";

export function initialLang(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "en" || saved === "ja") return saved;
  } catch { /* storage may be unavailable */ }
  return navigator.language?.toLowerCase().startsWith("ja") ? "ja" : "en";
}

/**
 * English text is the key, so untranslated strings (and any new message the
 * backend adds) fall back to English instead of breaking.
 * `{name}` placeholders are filled from the vars argument.
 */
const JA: Record<string, string> = {
  // Header / hero / footer
  "Everything stays on this PC": "すべてこのPC内で処理されます",
  "A new version ({version}) is available.": "新しいバージョン ({version}) があります。",
  "Installing…": "インストール中…",
  "Update & restart": "更新して再起動",
  "Update failed. Try again later.": "更新に失敗しました。あとでもう一度お試しください。",
  "SIMPLE VIDEO EXPORT": "かんたん動画書き出し",
  "Make every video fit.": "どんな動画も、ぴったりに。",
  "No upload. Choose a video, pick a destination, and export.": "アップロード不要。動画を選んで、投稿先を選んで、書き出すだけ。",
  "Drop a video here": "ここに動画をドロップ",
  "or": "または",
  "Choose a video": "動画を選ぶ",
  "SELECTED VIDEO": "選択中の動画",
  "Remove video": "動画を外す",
  "Local processing": "ローカル処理",
  "Free · No watermark · No upload": "無料 · ウォーターマークなし · アップロードなし",
  "Reset": "リセット",
  "Language": "言語",

  // Section headings
  "Choose edits": "編集を選ぶ",
  "Optional local processing before export": "書き出し前に行う、任意のローカル処理",
  "Choose a destination": "投稿先を選ぶ",
  "DropCut applies sensible settings automatically": "最適な設定が自動で適用されます",
  "Choose quality": "画質を選ぶ",
  "Recommended works well for most videos": "ほとんどの動画では「おすすめ」で十分です",

  // Shorts preset
  "Make it a Short": "ショート動画にする",
  "Vertical 9:16 · follow faces · trim pauses · clean audio · captions · gentle zoom": "縦型 9:16 · 顔を追従 · 間をカット · 音声クリーン · 字幕 · ゆるやかなズーム",
  "This video has no audio, so audio cleanup and captions were skipped.": "この動画には音声がないため、音声処理と字幕はスキップしました。",
  "Captions were skipped. Download a Whisper model below to add them.": "字幕はスキップしました。下のWhisperモデルをダウンロードすると追加できます。",
  "Captions were skipped. Download a Whisper model below and turn on Automatic subtitles to add them.": "字幕はスキップしました。下のWhisperモデルをダウンロードし、「自動字幕」をオンにすると追加できます。",
  "Filler-word cutting stays optional because it needs a quick review.": "フィラーワードのカットは確認が必要なため、任意のままにしています。",

  // Silence
  "Remove silence": "無音をカット",
  "Cut quiet sections while keeping natural pauses": "自然な間を残しつつ、無音部分をカットします",
  "Cut strength": "カットの強さ",
  "Weak": "弱",
  "Normal": "標準",
  "Strong": "強",
  "Advanced settings let you fine-tune the silence threshold, duration, and retained padding.": "詳細設定で、無音のしきい値・長さ・残す余白を細かく調整できます。",

  // Transcript editing
  "Remove filler words · Edit by text": "フィラーワード削除 · テキストで編集",
  "Review the transcript, then cut words like “um” or “えーと” from the video": "文字起こしを確認して、「えーと」「um」などの言葉を動画からカットします",
  "Analyze speech": "音声を解析",
  "Analyze again": "もう一度解析",
  "Select filler words": "フィラーワードを選択",
  "Restore all": "すべて元に戻す",
  "Choose a video first.": "先に動画を選んでください。",
  "Download a Whisper model below to analyze speech.": "音声を解析するには、下のWhisperモデルをダウンロードしてください。",
  "Analysis runs locally and cuts nothing yet. Removed words are cut when you create the video.": "解析はローカルで実行され、この時点では何もカットされません。削除した言葉は動画作成時にカットされます。",
  "{count} filler word found · {removed} removed ({seconds}s)": "フィラーワード {count} 件を検出 · {removed} 件を削除（{seconds}秒）",
  "{count} filler words found · {removed} removed ({seconds}s)": "フィラーワード {count} 件を検出 · {removed} 件を削除（{seconds}秒）",
  "No speech was found.": "音声が見つかりませんでした。",
  "Click a word, or drag across several, to remove or restore them — like editing text. Red-highlighted words are hesitations; amber ones can be real speech, so they start unchecked.": "言葉をクリック、または複数をドラッグすると、テキスト編集のように削除・復元できます。赤は言いよどみ、黄色は本当の発言の可能性があるため、最初は未選択です。",

  // Highlights
  "Find highlights": "ハイライトを探す",
  "Keep only the most engaging moments of a long video": "長い動画から、見どころだけを残します",
  "Clip length": "クリップの長さ",
  "Number of clips": "クリップ数",
  "{count} clip": "{count} 本",
  "{count} clips": "{count} 本",
  "Use the built-in local AI to pick the best clips (slower, private)": "内蔵のローカルAIでベストなクリップを選ぶ（遅いが、外部送信なし）",
  "Search again": "もう一度探す",
  "Download a Whisper model below to find highlights.": "ハイライトを探すには、下のWhisperモデルをダウンロードしてください。",
  "Selected": "選択中",
  "Not used": "使わない",
  "{picked} of {total} selected · {duration} in the final video": "{total} 件中 {picked} 件を選択 · 完成動画は {duration}",
  "Ranked with the built-in local AI model.": "内蔵のローカルAIモデルで順位付けしました。",
  "Ranked by speech, loudness and keywords.": "発話・音量・キーワードで順位付けしました。",
  "Analysis runs locally. Checked clips are joined in time order when you create the video; nothing else is kept.": "解析はローカルで実行されます。チェックしたクリップが時間順につなげられ、それ以外は残りません。",
  "No highlights were found. The video may be shorter than the clip length.": "ハイライトが見つかりませんでした。動画がクリップの長さより短い可能性があります。",
  "Laughter": "笑い声",
  "Key words": "キーワード",
  "Loud, energetic moment": "盛り上がった場面",
  "Lively conversation": "活発な会話",

  // Noise / normalize
  "Reduce noise": "ノイズ除去",
  "Soften background hiss and hum in the voice track": "声のトラックの、サーッというノイズやハム音を軽減します",
  "Reduction strength": "除去の強さ",
  "Strong settings remove more noise but can make voices sound thin.": "強にするとノイズは減りますが、声が細くなることがあります。",
  "Normalize audio": "音量を揃える",
  "Balance loudness for comfortable playback": "聞きやすい音量にバランスを整えます",

  // Reframe / zoom
  "Vertical 9:16 · follow faces": "縦型 9:16 · 顔を追従",
  "Crop a landscape video to portrait and keep the speaker in frame": "横長の動画を縦型にクロップし、話している人をフレーム内に収めます",
  "This video is already vertical, so it will not be cropped.": "この動画はすでに縦型なので、クロップされません。",
  "Faces are found on this PC and the crop follows them smoothly. Without a face, the crop stays centered.": "顔の検出はこのPC上で行われ、クロップがなめらかに追従します。顔がない場合は中央に固定されます。",
  "Auto zoom": "自動ズーム",
  "Add a light, regular push-in so the video feels less static": "軽いズームインを一定間隔で加え、単調さをなくします",
  "Zoom style": "ズームのスタイル",
  "Natural": "ナチュラル",
  "up to 5%": "最大5%",
  "up to 8%": "最大8%",
  "up to 12%": "最大12%",
  "Zoom never exceeds 12% and returns to 100% between pushes.": "ズームは最大12%までで、ズームの合間に100%へ戻ります。",

  // Subtitles
  "Automatic subtitles": "自動字幕",
  "Create an SRT file with local whisper.cpp": "ローカルのwhisper.cppでSRTファイルを作成します",
  "Video subtitles": "動画の字幕",
  "SRT only (FFmpeg lacks libass)": "SRTのみ（FFmpegにlibassがありません）",
  "Burn into video + SRT": "動画に焼き込み + SRT",
  "SRT file only": "SRTファイルのみ",
  "Simple": "シンプル",
  "Gaming": "ゲーミング",
  "Minimal": "ミニマル",
  "Pop": "ポップ",
  "Styled captions are also saved next to the video as an .ass file.": "スタイル付き字幕は、動画の隣に .ass ファイルとしても保存されます。",
  "Spoken language": "話している言語",
  "Japanese": "日本語",
  "English": "English",
  "Auto detect": "自動検出",
  "Ready: {path}": "準備完了: {path}",
  "whisper-cli is not available on PATH": "PATH上にwhisper-cliが見つかりません",

  // Models
  "Whisper model": "Whisperモデル",
  "Downloaded only when you choose one": "選んだときだけダウンロードされます",
  "RECOMMENDED": "おすすめ",
  "Cancel": "キャンセル",
  "Download": "ダウンロード",
  "Remove {name}": "{name} を削除",
  "Use a custom local .bin model": "ローカルの .bin モデルを使う",
  "Remove {name} from this PC? You can download it again later.": "{name} をこのPCから削除しますか？あとで再ダウンロードできます。",
  "Remove whisper model": "Whisperモデルの削除",
  "Model selection is available in the desktop app.": "モデルの選択はデスクトップアプリで利用できます。",
  "Fastest, suitable for drafts": "最速。下書き向け",
  "Balanced speed and accuracy": "速度と精度のバランス型",
  "More accurate, slower on CPU": "より高精度。CPUでは遅め",
  "Highest accuracy, requires more memory": "最高精度。より多くのメモリが必要",
  "Most accurate for hard audio, 2-3x slower than Turbo (q5_0)": "聞き取りにくい音声に最も強い。Turbo(q5_0)の2〜3倍遅い",
  "Best accuracy for Japanese, compressed (q5_0)": "日本語で最高精度。圧縮版 (q5_0)",
  "Downloading model": "モデルをダウンロード中",
  "Starting model download": "モデルのダウンロードを開始",
  "Verifying model integrity": "モデルの整合性を確認中",
  "Model ready": "モデルの準備ができました",
  "Model download cancelled": "モデルのダウンロードをキャンセルしました",

  // Presets
  "Original": "オリジナル",
  "Keep size and orientation": "サイズと向きを維持",
  "Landscape · 1080p": "横型 · 1080p",
  "Portrait · 1080p": "縦型 · 1080p",
  "Fit a size limit": "容量制限に合わせる",
  "Sharing · 720p": "共有向け · 720p",
  "Feed · 4:5": "フィード · 4:5",
  "Custom": "カスタム",
  "Choose every setting": "すべて自分で設定",
  "Maximum file size": "最大ファイルサイズ",

  // Quality
  "Smaller file": "小さいファイル",
  "Recommended": "おすすめ",
  "High quality": "高画質",
  "BEST": "最適",

  // Advanced
  "Advanced settings": "詳細設定",
  "Silence removal": "無音カット",
  "Threshold": "しきい値",
  "Minimum silence": "最小の無音時間",
  "Padding": "余白",
  "Subtitle style": "字幕スタイル",
  "Font": "フォント",
  "Auto": "自動",
  "Size": "サイズ",
  "Position": "位置",
  "Bottom": "下",
  "Middle": "中央",
  "Top": "上",
  "Stroke": "縁取り",
  "Shadow": "影",
  "Background": "背景",
  "Style default": "スタイルの既定",
  "Box": "ボックス",
  "None": "なし",
  "Line length": "1行の長さ",
  "chars": "文字",
  "Audio normalization": "音量の正規化",
  "Target loudness": "目標ラウドネス",
  "Loudness range": "ラウドネスレンジ",
  "True peak": "トゥルーピーク",
  "Width": "幅",
  "Height": "高さ",
  "Codec": "コーデック",
  "H.264 (compatible)": "H.264（互換性重視）",
  "H.265 (smaller)": "H.265（小さいサイズ）",
  "Video bitrate": "動画ビットレート",
  "Output format": "出力形式",
  "Output size": "出力サイズ",
  "sec": "秒",

  // Actions / status
  "Create video": "動画を作成",
  "Cancel processing": "処理をキャンセル",
  "Your original video is never modified": "元の動画は変更されません",
  "Saved to: {path}": "保存先: {path}",

  // Errors (frontend)
  "Unsupported file type. Choose an MP4, MOV, WebM, MKV, or AVI file.": "対応していないファイル形式です。MP4、MOV、WebM、MKV、AVIのいずれかを選んでください。",
  "Video processing is available in the desktop app.": "動画の処理はデスクトップアプリで利用できます。",
  "FFmpeg and FFprobe were not found. Install them, add them to PATH, and restart DropCut.": "FFmpegとFFprobeが見つかりません。インストールしてPATHに追加し、DropCutを再起動してください。",
  "whisper.cpp was not found. Install whisper-cli and add it to PATH before enabling automatic subtitles.": "whisper.cppが見つかりません。自動字幕を使う前に、whisper-cliをインストールしてPATHに追加してください。",
  "Choose a local whisper.cpp model before exporting subtitles.": "字幕を書き出す前に、ローカルのwhisper.cppモデルを選んでください。",
  "Silence removal requires a video with an audio track.": "無音カットには、音声トラックのある動画が必要です。",
  "Audio normalization requires a video with an audio track.": "音量の正規化には、音声トラックのある動画が必要です。",
  "Noise reduction requires a video with an audio track.": "ノイズ除去には、音声トラックのある動画が必要です。",
  "Analyze the speech first, or turn off transcript editing.": "先に音声を解析するか、テキスト編集をオフにしてください。",
  "Vertical conversion needs a portrait destination such as Shorts, TikTok, or Instagram.": "縦型変換には、Shorts・TikTok・Instagramなど縦型の投稿先が必要です。",
  "whisper.cpp was not found. Install whisper-cli and add it to PATH before analyzing speech.": "whisper.cppが見つかりません。音声を解析する前に、whisper-cliをインストールしてPATHに追加してください。",
  "Choose a Whisper model before analyzing speech.": "音声を解析する前に、Whisperモデルを選んでください。",
  "whisper.cpp was not found. Install whisper-cli and add it to PATH before finding highlights.": "whisper.cppが見つかりません。ハイライトを探す前に、whisper-cliをインストールしてPATHに追加してください。",
  "Choose a Whisper model before finding highlights.": "ハイライトを探す前に、Whisperモデルを選んでください。",

  // Backend progress messages
  "Preparing export": "書き出しを準備中",
  "Preparing transcript": "文字起こしを準備中",
  "Preparing highlight search": "ハイライト検索を準備中",
  "Detecting quiet sections": "無音部分を検出中",
  "Extracting audio": "音声を抽出中",
  "Extracting audio for subtitles": "字幕用に音声を抽出中",
  "Transcribing locally with whisper.cpp": "whisper.cppでローカルに文字起こし中",
  "Analyzing audio loudness": "音声のラウドネスを解析中",
  "Encoding video": "動画をエンコード中",
  "Verifying output": "出力を確認中",
  "Processing cancelled": "処理をキャンセルしました",
  "Subtitle generation failed": "字幕の生成に失敗しました",
  "Video export failed": "動画の書き出しに失敗しました",
  "The export exceeded the size limit and was not saved": "書き出しがサイズ上限を超えたため、保存されませんでした",
  "Video and subtitles ready": "動画と字幕ができました",
  "Video ready": "動画ができました",
  "Transcript ready": "文字起こしができました",
  "Transcription failed": "文字起こしに失敗しました",
  "Judging clips with the local AI model": "ローカルAIモデルでクリップを評価中",
  "Finding highlights": "ハイライトを探しています",
  "Highlights ready": "ハイライトができました",
  "Highlight search failed": "ハイライト検索に失敗しました",

  // Backend errors
  "The video could not be processed. Check that the file is valid and FFmpeg is available.": "動画を処理できませんでした。ファイルが正しいことと、FFmpegが利用できることを確認してください。",
  "This file type is not supported.": "このファイル形式には対応していません。",
  "The video file could not be found.": "動画ファイルが見つかりませんでした。",
  "The selected path is not a video file.": "選択されたパスは動画ファイルではありません。",
  "This model is already being downloaded.": "このモデルはすでにダウンロード中です。",
  "A model cannot be removed while it is downloading.": "ダウンロード中のモデルは削除できません。",
  "The silence threshold must be between -60 dB and -10 dB.": "無音のしきい値は -60 dB 〜 -10 dB の範囲で指定してください。",
  "The minimum silence duration must be between 0.1 and 10 seconds.": "最小の無音時間は 0.1 〜 10 秒の範囲で指定してください。",
  "Silence padding must be between 0 and 2 seconds.": "無音の余白は 0 〜 2 秒の範囲で指定してください。",
  "The entire video was detected as silence. Try a weaker setting.": "動画全体が無音と判定されました。弱めの設定をお試しください。",
  "Invalid job identifier.": "ジョブIDが正しくありません。",
  "A job with the same identifier is already running.": "同じIDのジョブがすでに実行中です。",
  "The selected whisper.cpp model is not a valid .bin file.": "選択されたwhisper.cppモデルは、有効な .bin ファイルではありません。",
  "Unsupported transcription language.": "対応していない文字起こし言語です。",
  "Audio extraction for subtitle generation failed.": "字幕生成のための音声抽出に失敗しました。",
  "Audio extraction for transcription failed.": "文字起こしのための音声抽出に失敗しました。",
  "Local transcription failed. Check the model and try again.": "ローカルの文字起こしに失敗しました。モデルを確認して、もう一度お試しください。",
  "whisper-cli produced an empty SRT file.": "whisper-cliが空のSRTファイルを出力しました。",
  "Target loudness must be between -24 and -5 LUFS.": "目標ラウドネスは -24 〜 -5 LUFS の範囲で指定してください。",
  "Loudness range must be between 1 and 20 LU.": "ラウドネスレンジは 1 〜 20 LU の範囲で指定してください。",
  "True peak must be between -9 and 0 dB.": "トゥルーピークは -9 〜 0 dB の範囲で指定してください。",
  "Audio loudness analysis failed. Try exporting without normalization.": "音声のラウドネス解析に失敗しました。音量の正規化なしで書き出してみてください。",
  "Custom dimensions must be 240–7680 pixels wide and 240–4320 pixels high.": "カスタムサイズは、幅 240〜7680 px、高さ 240〜4320 px の範囲で指定してください。",
  "Unknown export preset.": "不明な書き出しプリセットです。",
  "The temporary subtitle filename is invalid.": "字幕の一時ファイル名が正しくありません。",
  "Unknown video codec.": "不明な動画コーデックです。",
  "The selected audio processing options require an audio track.": "選択した音声処理には、音声トラックが必要です。",
  "Video export failed. Change the settings or try another video.": "動画の書き出しに失敗しました。設定を変えるか、別の動画をお試しください。",
  "The exported video could not be verified.": "書き出した動画を確認できませんでした。",
  "Unknown auto zoom style.": "不明な自動ズームのスタイルです。",
  "Unknown noise reduction strength.": "不明なノイズ除去の強さです。",
  "This video has no audio to analyze for highlights.": "この動画には、ハイライトを解析できる音声がありません。",
  "Invalid timestamp.": "タイムスタンプが正しくありません。",
  "Could not read a preview frame.": "プレビューのフレームを読み込めませんでした。",
  "Highlight length must be 10-180 seconds and 1-10 clips.": "ハイライトの長さは 10〜180 秒、クリップ数は 1〜10 本の範囲で指定してください。",
  "The server returned an unexpected model size.": "サーバーから想定外のモデルサイズが返されました。",
  "The model download exceeded the expected size.": "モデルのダウンロードが想定サイズを超えました。",
  "The model checksum is invalid. The downloaded file was rejected.": "モデルのチェックサムが正しくありません。ダウンロードしたファイルは破棄されました。",
};

export type TFunction = (text: string, vars?: Record<string, string | number>) => string;

export function translate(lang: Lang, text: string, vars?: Record<string, string | number>) {
  let result = lang === "ja" ? JA[text] ?? text : text;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) result = result.split(`{${name}}`).join(String(value));
  }
  return result;
}

export function useLanguage() {
  const [lang, setLangState] = useState<Lang>(initialLang);
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    try { localStorage.setItem(STORAGE_KEY, next); } catch { /* ignore */ }
  }, []);
  const t = useCallback<TFunction>((text, vars) => translate(lang, text, vars), [lang]);
  return { lang, setLang, t };
}
