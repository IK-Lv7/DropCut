import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import {
  AudioWaveform, Captions, ChevronDown, Download, Film, FolderOpen, HardDrive, LockKeyhole,
  MessageSquareText, Flame, RotateCcw, Scissors, Search, Settings2, ShieldCheck, Smartphone, Sparkles,
  Trash2, Undo2, Upload, Volume2, X, ZoomIn,
} from "lucide-react";
import { PRESETS, SUPPORTED_EXTENSIONS } from "./presets";
import type {
  Highlight,
  AutoZoomTemplate, CutRange, ExportSettings, ModelDownloadProgress, PresetId, ProgressEvent, Quality,
  Strength, SubtitleTemplate, ToolStatus, TranscriptWord, VideoInfo, WhisperModel,
} from "./types";

const isTauri = () => "__TAURI_INTERNALS__" in window;

function formatBytes(bytes: number) {
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

/** Words that are almost never content are removed by default; uncertain ones are left for review. */
function selectFillers(words: TranscriptWord[]) {
  return new Set(words.flatMap((word, index) => (word.filler === "high" ? [index] : [])));
}

const SUBTITLE_TEMPLATES: Array<{ id: SubtitleTemplate; label: string }> = [
  { id: "simple", label: "Simple" }, { id: "youtube", label: "YouTube" }, { id: "tiktok", label: "TikTok" },
  { id: "gaming", label: "Gaming" }, { id: "minimal", label: "Minimal" }, { id: "pop", label: "Pop" },
];
const SUBTITLE_FONTS = ["Yu Gothic", "Meiryo", "Noto Sans JP", "Arial", "Arial Black", "Impact", "Segoe UI"];
const ZOOM_TEMPLATES: Array<{ id: Exclude<AutoZoomTemplate, "none">; label: string; hint: string }> = [
  { id: "natural", label: "Natural", hint: "up to 5%" },
  { id: "youtube", label: "YouTube", hint: "up to 8%" },
  { id: "shorts", label: "Shorts", hint: "up to 12%" },
];

/** Empty text means "use the template's value". */
function optionalNumber(text: string) {
  return text.trim() === "" ? null : Number(text);
}

function formatDuration(seconds: number) {
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${m}:${String(s).padStart(2, "0")}`;
}

export default function App() {
  const [video, setVideo] = useState<VideoInfo | null>(null);
  const [preset, setPreset] = useState<PresetId>("youtube");
  const [quality, setQuality] = useState<Quality>("recommended");
  const [advanced, setAdvanced] = useState(false);
  const [maxSizeMb, setMaxSizeMb] = useState(25);
  const [customWidth, setCustomWidth] = useState(1920);
  const [customHeight, setCustomHeight] = useState(1080);
  const [customFps, setCustomFps] = useState(30);
  const [customCodec, setCustomCodec] = useState<"h264" | "h265">("h264");
  const [customBitrate, setCustomBitrate] = useState(8000);
  const [silenceRemovalEnabled, setSilenceRemovalEnabled] = useState(false);
  const [silenceStrength, setSilenceStrength] = useState<"weak" | "normal" | "strong" | "custom">("normal");
  const [silenceThresholdDb, setSilenceThresholdDb] = useState(-35);
  const [minimumSilenceDuration, setMinimumSilenceDuration] = useState(0.5);
  const [silencePadding, setSilencePadding] = useState(0.15);
  const [audioNormalizationEnabled, setAudioNormalizationEnabled] = useState(false);
  const [targetLufs, setTargetLufs] = useState(-16);
  const [loudnessRange, setLoudnessRange] = useState(11);
  const [truePeakDb, setTruePeakDb] = useState(-1.5);
  const [noiseReductionEnabled, setNoiseReductionEnabled] = useState(false);
  const [noiseStrength, setNoiseStrength] = useState<Strength>("normal");
  const [reframeEnabled, setReframeEnabled] = useState(false);
  const [transcriptEditEnabled, setTranscriptEditEnabled] = useState(false);
  const [transcript, setTranscript] = useState<TranscriptWord[] | null>(null);
  const [removedWords, setRemovedWords] = useState<Set<number>>(new Set());
  const [highlightEnabled, setHighlightEnabled] = useState(false);
  const [highlightLength, setHighlightLength] = useState(30);
  const [highlightCount, setHighlightCount] = useState(3);
  const [llmReady, setLlmReady] = useState(false);
  const [useAi, setUseAi] = useState(true);
  const [aiUsed, setAiUsed] = useState(false);
  const [highlights, setHighlights] = useState<Highlight[] | null>(null);
  const [pickedHighlights, setPickedHighlights] = useState<Set<number>>(new Set());
  const [autoZoom, setAutoZoom] = useState<AutoZoomTemplate>("none");
  const [subtitleTemplate, setSubtitleTemplate] = useState<SubtitleTemplate>("simple");
  const [subtitleFont, setSubtitleFont] = useState("");
  const [subtitleSize, setSubtitleSize] = useState("");
  const [subtitleOutline, setSubtitleOutline] = useState("");
  const [subtitleShadow, setSubtitleShadow] = useState("");
  const [subtitlePosition, setSubtitlePosition] = useState<"bottom" | "middle" | "top">("bottom");
  const [subtitleBackground, setSubtitleBackground] = useState<"template" | "on" | "off">("template");
  const [subtitleMaxChars, setSubtitleMaxChars] = useState("");
  const [shortsNote, setShortsNote] = useState<string | null>(null);
  const [subtitlesEnabled, setSubtitlesEnabled] = useState(false);
  const [subtitleModelPath, setSubtitleModelPath] = useState<string | null>(null);
  const [subtitleLanguage, setSubtitleLanguage] = useState<"auto" | "ja" | "en">("ja");
  const [burnInSubtitles, setBurnInSubtitles] = useState(true);
  const [burnInAvailable, setBurnInAvailable] = useState<boolean | null>(null);
  const [whisperStatus, setWhisperStatus] = useState<ToolStatus | null>(null);
  const [whisperModels, setWhisperModels] = useState<WhisperModel[]>([]);
  const [downloadingModelId, setDownloadingModelId] = useState<string | null>(null);
  const [modelProgress, setModelProgress] = useState<ModelDownloadProgress | null>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ffmpegReady, setFfmpegReady] = useState<boolean | null>(null);
  const processing = !!jobId;

  const refreshModels = useCallback(async () => {
    if (!isTauri()) return;
    const models = await invoke<WhisperModel[]>("list_whisper_models");
    setWhisperModels(models);
    setSubtitleModelPath((currentPath) => {
      if (currentPath) return currentPath;
      const preferred = models.find((model) => model.recommended && model.installed)
        ?? models.find((model) => model.installed);
      return preferred?.path ?? null;
    });
  }, []);

  useEffect(() => {
    setTranscript(null);
    setRemovedWords(new Set());
    setHighlights(null);
    setPickedHighlights(new Set());
  }, [video?.path]);

  const wordCuts = useMemo<CutRange[]>(() => {
    if (!transcriptEditEnabled || !transcript) return [];
    const ranges: CutRange[] = [];
    let current: CutRange | null = null;
    for (let index = 0; index < transcript.length; index += 1) {
      const word = transcript[index];
      if (!removedWords.has(index)) {
        current = null;
      } else if (current) {
        current.end = Math.max(current.end, word.end);
      } else {
        current = { start: word.start, end: word.end };
        ranges.push(current);
      }
    }
    return ranges.filter((range) => range.end > range.start);
  }, [transcriptEditEnabled, transcript, removedWords]);
  // Keeping only the picked highlights means cutting everything between them.
  const cutRanges = useMemo<CutRange[]>(() => {
    if (!highlightEnabled || !highlights || !video || pickedHighlights.size === 0) return wordCuts;
    const keep = highlights.filter((_, index) => pickedHighlights.has(index)).sort((a, b) => a.start - b.start);
    const cuts: CutRange[] = [];
    let cursor = 0;
    for (const clip of keep) {
      if (clip.start > cursor) cuts.push({ start: cursor, end: clip.start });
      cursor = Math.max(cursor, clip.end);
    }
    if (cursor < video.durationSeconds) cuts.push({ start: cursor, end: video.durationSeconds });
    return [...wordCuts, ...cuts];
  }, [highlightEnabled, highlights, pickedHighlights, video, wordCuts]);
  const removedSeconds = wordCuts.reduce((total, range) => total + range.end - range.start, 0);
  const fillerCount = transcript?.filter((word) => word.filler).length ?? 0;
  const alreadyPortrait = !!video && video.width * 16 <= video.height * 9;

  const loadVideo = useCallback(async (path: string) => {
    setError(null);
    const ext = path.split(".").pop()?.toLowerCase();
    if (!ext || !SUPPORTED_EXTENSIONS.includes(ext)) {
      setError("Unsupported file type. Choose an MP4, MOV, WebM, MKV, or AVI file.");
      return;
    }
    try {
      const info = await invoke<VideoInfo>("probe_video", { path });
      setVideo(info);
      setProgress(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<boolean>("check_llm").then(setLlmReady).catch(() => setLlmReady(false));
    invoke<boolean>("check_ffmpeg").then(setFfmpegReady).catch(() => setFfmpegReady(false));
    invoke<boolean>("check_subtitle_burn_in").then((available) => {
      setBurnInAvailable(available);
      if (!available) setBurnInSubtitles(false);
    }).catch(() => {
      setBurnInAvailable(false);
      setBurnInSubtitles(false);
    });
    invoke<ToolStatus>("check_whisper").then(setWhisperStatus).catch(() => setWhisperStatus({ available: false, executable: null }));
    refreshModels().catch((reason) => setError(String(reason)));
    const unlistenProgress = listen<ProgressEvent>("export-progress", ({ payload }) => {
      setProgress(payload);
      if (["done", "cancelled", "error"].includes(payload.stage)) setJobId(null);
    });
    const unlistenDrop = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop" && event.payload.paths[0]) loadVideo(event.payload.paths[0]);
    });
    const unlistenModelProgress = listen<ModelDownloadProgress>("model-download-progress", ({ payload }) => {
      setModelProgress(payload);
    });
    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenDrop.then((fn) => fn());
      unlistenModelProgress.then((fn) => fn());
    };
  }, [loadVideo, refreshModels]);

  const chooseVideo = async () => {
    setError(null);
    if (!isTauri()) {
      document.getElementById("browser-file")?.click();
      return;
    }
    const selected = await open({ multiple: false, filters: [{ name: "Video", extensions: SUPPORTED_EXTENSIONS }] });
    if (selected) await loadVideo(selected);
  };

  const exportVideo = async () => {
    if (!video || processing) return;
    if (ffmpegReady === false) {
      setError("FFmpeg and FFprobe were not found. Install them, add them to PATH, and restart DropCut.");
      return;
    }
    if (subtitlesEnabled && !whisperStatus?.available) {
      setError("whisper.cpp was not found. Install whisper-cli and add it to PATH before enabling automatic subtitles.");
      return;
    }
    if (subtitlesEnabled && !subtitleModelPath) {
      setError("Choose a local whisper.cpp model before exporting subtitles.");
      return;
    }
    if (silenceRemovalEnabled && !video.hasAudio) {
      setError("Silence removal requires a video with an audio track.");
      return;
    }
    if (audioNormalizationEnabled && !video.hasAudio) {
      setError("Audio normalization requires a video with an audio track.");
      return;
    }
    if (noiseReductionEnabled && !video.hasAudio) {
      setError("Noise reduction requires a video with an audio track.");
      return;
    }
    if (transcriptEditEnabled && !transcript) {
      setError("Analyze the speech first, or turn off transcript editing.");
      return;
    }
    if (reframeEnabled && (preset === "youtube" || preset === "x")) {
      setError("Vertical conversion needs a portrait destination such as Shorts, TikTok, or Instagram.");
      return;
    }
    const id = `export-${Date.now()}`;
    const settings: ExportSettings = {
      preset, quality, maxSizeMb: preset === "discord" ? maxSizeMb : null,
      width: preset === "custom" ? customWidth : null,
      height: preset === "custom" ? customHeight : null,
      fps: preset === "custom" ? customFps : null,
      codec: preset === "custom" ? customCodec : "h264",
      bitrateKbps: preset === "custom" ? customBitrate : null,
      subtitles: {
        enabled: subtitlesEnabled,
        modelPath: subtitleModelPath,
        language: subtitleLanguage,
        burnIn: burnInSubtitles,
        style: {
          template: subtitleTemplate,
          font: subtitleFont || null,
          sizePercent: optionalNumber(subtitleSize),
          outline: optionalNumber(subtitleOutline),
          shadow: optionalNumber(subtitleShadow),
          position: subtitlePosition,
          background: subtitleBackground === "template" ? null : subtitleBackground === "on",
          maxChars: optionalNumber(subtitleMaxChars),
        },
      },
      silenceRemoval: {
        enabled: silenceRemovalEnabled,
        thresholdDb: silenceThresholdDb,
        minimumDurationSeconds: minimumSilenceDuration,
        paddingSeconds: silencePadding,
      },
      audioNormalization: {
        enabled: audioNormalizationEnabled,
        targetLufs,
        loudnessRange,
        truePeakDb,
      },
      noiseReduction: { enabled: noiseReductionEnabled, strength: noiseStrength },
      reframe: { enabled: reframeEnabled },
      cutRanges,
      autoZoom: { template: autoZoom },
    };
    setJobId(id);
    setError(null);
    setProgress({ jobId: id, stage: "preparing", percent: 0, message: "Preparing export" });
    try {
      await invoke("start_export", { jobId: id, inputPath: video.path, settings });
    } catch (reason) {
      setJobId(null);
      setError(String(reason));
    }
  };

  const analyzeSpeech = async () => {
    if (!video || processing) return;
    if (!whisperStatus?.available) {
      setError("whisper.cpp was not found. Install whisper-cli and add it to PATH before analyzing speech.");
      return;
    }
    if (!subtitleModelPath) {
      setError("Choose a Whisper model before analyzing speech.");
      return;
    }
    const id = `analyze-${Date.now()}`;
    setJobId(id);
    setError(null);
    setProgress({ jobId: id, stage: "extractingAudio", percent: 0, message: "Preparing transcript" });
    try {
      const words = await invoke<TranscriptWord[] | null>("analyze_transcript", {
        jobId: id, inputPath: video.path, modelPath: subtitleModelPath, language: subtitleLanguage,
      });
      if (words) {
        setTranscript(words);
        setRemovedWords(selectFillers(words));
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setJobId(null);
    }
  };

  const findHighlights = async () => {
    if (!video || processing) return;
    if (!whisperStatus?.available) {
      setError("whisper.cpp was not found. Install whisper-cli and add it to PATH before finding highlights.");
      return;
    }
    if (!subtitleModelPath) {
      setError("Choose a Whisper model before finding highlights.");
      return;
    }
    const id = `highlight-${Date.now()}`;
    setJobId(id);
    setError(null);
    setProgress({ jobId: id, stage: "extractingAudio", percent: 0, message: "Preparing highlight search" });
    try {
      const result = await invoke<{ words: TranscriptWord[]; highlights: Highlight[]; aiUsed: boolean } | null>("find_highlights", {
        jobId: id, inputPath: video.path, modelPath: subtitleModelPath, language: subtitleLanguage,
        targetSeconds: highlightLength, count: highlightCount, useLlm: useAi && llmReady,
      });
      if (result) {
        setHighlights(result.highlights);
        setAiUsed(result.aiUsed);
        setPickedHighlights(new Set(result.highlights.length ? [0] : []));
        if (result.highlights.length === 0) setError("No highlights were found. The video may be shorter than the clip length.");
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setJobId(null);
    }
  };

  const togglePickedHighlight = (index: number) => {
    setPickedHighlights((current) => {
      const next = new Set(current);
      if (!next.delete(index)) next.add(index);
      return next;
    });
  };

  const applyShortsPreset = () => {
    setPreset("shorts");
    setReframeEnabled(true);
    setAutoZoom("shorts");
    setSubtitleTemplate("tiktok");
    const notes: string[] = [];
    if (video && !video.hasAudio) {
      notes.push("This video has no audio, so audio cleanup and captions were skipped.");
    } else {
      applySilenceStrength("normal");
      setSilenceRemovalEnabled(true);
      setNoiseReductionEnabled(true);
      setNoiseStrength("normal");
      setAudioNormalizationEnabled(true);
      const canCaption = !!whisperStatus?.available && !!subtitleModelPath;
      setSubtitlesEnabled(canCaption);
      setBurnInSubtitles(burnInAvailable !== false);
      if (!canCaption) notes.push("Captions were skipped. Download a Whisper model below and turn on Automatic subtitles to add them.");
    }
    notes.push("Filler-word cutting stays optional because it needs a quick review.");
    setShortsNote(notes.join(" "));
  };

  const toggleWord = (index: number) => {
    setRemovedWords((current) => {
      const next = new Set(current);
      if (!next.delete(index)) next.add(index);
      return next;
    });
  };

  const applySilenceStrength = (strength: "weak" | "normal" | "strong") => {
    const values = {
      weak: { threshold: -42, duration: 1, padding: 0.2 },
      normal: { threshold: -35, duration: 0.5, padding: 0.15 },
      strong: { threshold: -30, duration: 0.3, padding: 0.1 },
    }[strength];
    setSilenceStrength(strength);
    setSilenceThresholdDb(values.threshold);
    setMinimumSilenceDuration(values.duration);
    setSilencePadding(values.padding);
  };

  const cancel = async () => {
    if (jobId) await invoke("cancel_export", { jobId });
  };

  const chooseSubtitleModel = async () => {
    if (!isTauri()) {
      setError("Model selection is available in the desktop app.");
      return;
    }
    const selected = await open({ multiple: false, filters: [{ name: "whisper.cpp model", extensions: ["bin"] }] });
    if (selected) setSubtitleModelPath(selected);
  };

  const downloadModel = async (modelId: string) => {
    setDownloadingModelId(modelId);
    setModelProgress(null);
    setError(null);
    try {
      const path = await invoke<string | null>("download_whisper_model", { modelId });
      if (path) setSubtitleModelPath(path);
      await refreshModels();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setDownloadingModelId(null);
    }
  };

  const cancelModelDownload = async (modelId: string) => {
    await invoke("cancel_model_download", { modelId });
  };

  const deleteModel = async (model: WhisperModel) => {
    const approved = await confirm(`Remove ${model.name} from this PC? You can download it again later.`, {
      title: "Remove whisper model",
      kind: "warning",
    });
    if (!approved) return;
    try {
      await invoke("delete_whisper_model", { modelId: model.id });
      if (subtitleModelPath === model.path) setSubtitleModelPath(null);
      await refreshModels();
    } catch (reason) {
      setError(String(reason));
    }
  };

  const selectedPreset = useMemo(() => PRESETS.find((item) => item.id === preset)!, [preset]);

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand"><span className="brand-mark"><Film size={20} /></span><span>DropCut</span><em>LOCAL</em></div>
        <div className="privacy"><LockKeyhole size={15} /> Everything stays on this PC</div>
      </header>

      <section className="hero">
        <div className="eyebrow"><Sparkles size={15} /> SIMPLE VIDEO EXPORT</div>
        <h1>Make every video fit.</h1>
        <p>No upload. Choose a video, pick a destination, and export.</p>
      </section>

      <section className="workspace">
        {!video ? (
          <button className="dropzone" onClick={chooseVideo} type="button">
            <span className="upload-icon"><Upload size={28} /></span>
            <strong>Drop a video here</strong>
            <span>or</span>
            <b><FolderOpen size={17} /> Choose a video</b>
            <small>MP4 · MOV · WebM · MKV · AVI</small>
          </button>
        ) : (
          <div className="video-card">
            <div className="video-thumb"><Film size={30} /></div>
            <div className="video-meta">
              <span>SELECTED VIDEO</span>
              <strong title={video.path}>{video.name}</strong>
              <p>{formatDuration(video.durationSeconds)} <i /> {video.width}×{video.height} <i /> {formatBytes(video.sizeBytes)}</p>
            </div>
            {!processing && <button className="icon-button" onClick={() => { setVideo(null); setProgress(null); }} aria-label="Remove video"><X size={19} /></button>}
          </div>
        )}
        <input id="browser-file" hidden type="file" accept="video/mp4,video/quicktime,video/webm,video/x-matroska,video/x-msvideo" onChange={() => setError("Video processing is available in the desktop app.")} />

        <div className="section-heading"><div><span>01</span><div><h2>Choose edits</h2><p>Optional local processing before export</p></div></div></div>
        <button className="shorts-preset" type="button" disabled={processing} onClick={applyShortsPreset}>
          <span className="feature-icon"><Sparkles size={21} /></span>
          <span><strong>Make it a Short</strong><small>Vertical 9:16 · follow faces · trim pauses · clean audio · captions · gentle zoom</small></span>
        </button>
        {shortsNote && <p className="shorts-note">{shortsNote}</p>}
        <div className={`feature-card ${silenceRemovalEnabled ? "selected" : ""}`}>
          <button className="feature-main" type="button" onClick={() => setSilenceRemovalEnabled(!silenceRemovalEnabled)}>
            <span className="feature-icon"><Scissors size={21} /></span>
            <span><strong>Remove silence</strong><small>Cut quiet sections while keeping natural pauses</small></span>
            <span className={`switch ${silenceRemovalEnabled ? "on" : ""}`}><i /></span>
          </button>
          {silenceRemovalEnabled && (
            <div className="feature-settings silence-settings">
              <span>Cut strength</span>
              <div className="strength-options">
                {(["weak", "normal", "strong"] as const).map((strength) => (
                  <button className={silenceStrength === strength ? "active" : ""} type="button" key={strength} onClick={() => applySilenceStrength(strength)}>{strength[0].toUpperCase() + strength.slice(1)}</button>
                ))}
              </div>
              <p>Advanced settings let you fine-tune the silence threshold, duration, and retained padding.</p>
            </div>
          )}
        </div>
        <div className={`feature-card ${transcriptEditEnabled ? "selected" : ""}`}>
          <button className="feature-main" type="button" onClick={() => setTranscriptEditEnabled(!transcriptEditEnabled)}>
            <span className="feature-icon"><MessageSquareText size={21} /></span>
            <span><strong>Remove filler words · Edit by text</strong><small>Review the transcript, then cut words like “um” or “えーと” from the video</small></span>
            <span className={`switch ${transcriptEditEnabled ? "on" : ""}`}><i /></span>
          </button>
          {transcriptEditEnabled && (
            <div className="transcript-panel">
              <div className="transcript-actions">
                <button type="button" disabled={!video || processing || !subtitleModelPath || !whisperStatus?.available} onClick={analyzeSpeech}><Search size={14} /> {transcript ? "Analyze again" : "Analyze speech"}</button>
                {transcript && <button type="button" disabled={processing} onClick={() => setRemovedWords(selectFillers(transcript))}>Select filler words</button>}
                {transcript && <button type="button" disabled={processing || removedWords.size === 0} onClick={() => setRemovedWords(new Set())}><Undo2 size={14} /> Restore all</button>}
              </div>
              {!video && <p>Choose a video first.</p>}
              {video && !subtitleModelPath && <p>Download a Whisper model below to analyze speech.</p>}
              {!transcript ? (
                <p>Analysis runs locally and cuts nothing yet. Removed words are cut when you create the video.</p>
              ) : (
                <>
                  <p className="transcript-summary"><strong>{fillerCount}</strong> filler {fillerCount === 1 ? "word" : "words"} found · <strong>{removedWords.size}</strong> removed ({removedSeconds.toFixed(1)}s)</p>
                  <div className="transcript-words" lang={subtitleLanguage === "auto" ? undefined : subtitleLanguage}>
                    {transcript.length === 0 && <span>No speech was found.</span>}
                    {transcript.map((word, index) => (
                      <Fragment key={index}>
                        <button
                          type="button"
                          className={`word ${word.filler ? `filler-${word.filler}` : ""} ${removedWords.has(index) ? "removed" : ""}`}
                          title={`${word.start.toFixed(2)}s – ${word.end.toFixed(2)}s`}
                          disabled={processing}
                          onClick={() => toggleWord(index)}
                        >{word.text}</button>
                        {/^[\x00-\x7F]/.test(word.text) ? " " : null}
                      </Fragment>
                    ))}
                  </div>
                  <p>Click a word to remove or restore it. Red-highlighted words are hesitations; amber ones can be real speech, so they start unchecked.</p>
                </>
              )}
            </div>
          )}
        </div>
        <div className={`feature-card ${highlightEnabled ? "selected" : ""}`}>
          <button className="feature-main" type="button" onClick={() => setHighlightEnabled(!highlightEnabled)}>
            <span className="feature-icon"><Flame size={21} /></span>
            <span><strong>Find highlights</strong><small>Keep only the most engaging moments of a long video</small></span>
            <span className={`switch ${highlightEnabled ? "on" : ""}`}><i /></span>
          </button>
          {highlightEnabled && (
            <div className="transcript-panel">
              <span className="option-label">Clip length</span>
              <div className="strength-options choice-row four">
                {[15, 30, 60, 90].map((seconds) => (
                  <button className={highlightLength === seconds ? "active" : ""} type="button" key={seconds} onClick={() => setHighlightLength(seconds)}>{seconds}s</button>
                ))}
              </div>
              <span className="option-label">Number of clips</span>
              <div className="strength-options choice-row">
                {[1, 3, 5].map((count) => (
                  <button className={highlightCount === count ? "active" : ""} type="button" key={count} onClick={() => setHighlightCount(count)}>{count} {count === 1 ? "clip" : "clips"}</button>
                ))}
              </div>
              {llmReady && (
                <button type="button" className={`compact-toggle ${useAi ? "active" : ""}`} onClick={() => setUseAi(!useAi)}>
                  <span className="mini-switch"><i /></span><span>Use the built-in local AI to pick the best clips (slower, private)</span>
                </button>
              )}
              <div className="transcript-actions">
                <button type="button" disabled={!video || processing || !subtitleModelPath || !whisperStatus?.available} onClick={findHighlights}><Search size={14} /> {highlights ? "Search again" : "Find highlights"}</button>
              </div>
              {!video && <p>Choose a video first.</p>}
              {video && !subtitleModelPath && <p>Download a Whisper model below to find highlights.</p>}
              {highlights && highlights.length > 0 && (
                <>
                  <div className="highlight-list">
                    {highlights.map((clip, index) => {
                      const picked = pickedHighlights.has(index);
                      return (
                        <button type="button" key={index} disabled={processing} aria-pressed={picked} className={`highlight-item ${picked ? "picked" : ""}`} onClick={() => togglePickedHighlight(index)}>
                          <span className="highlight-check">{picked ? "✓" : ""}</span>
                          <span className="highlight-body"><strong>#{index + 1} · {formatDuration(clip.start)} – {formatDuration(clip.end)}</strong><small>{clip.reason} · {Math.round(clip.end - clip.start)}s</small></span>
                          <span className="highlight-state">{picked ? "Selected" : "Not used"}</span>
                        </button>
                      );
                    })}
                  </div>
                  <p className="transcript-summary"><strong>{pickedHighlights.size}</strong> of {highlights.length} selected · {formatDuration(highlights.filter((_, i) => pickedHighlights.has(i)).reduce((t, c) => t + c.end - c.start, 0))} in the final video</p>
                </>
              )}
              {highlights && highlights.length > 0 && <p>{aiUsed ? "Ranked with the built-in local AI model." : "Ranked by speech, loudness and keywords."}</p>}
              <p>Analysis runs locally. Checked clips are joined in time order when you create the video; nothing else is kept.</p>
            </div>
          )}
        </div>
        <div className={`feature-card ${noiseReductionEnabled ? "selected" : ""}`}>
          <button className="feature-main" type="button" onClick={() => setNoiseReductionEnabled(!noiseReductionEnabled)}>
            <span className="feature-icon"><AudioWaveform size={21} /></span>
            <span><strong>Reduce noise</strong><small>Soften background hiss and hum in the voice track</small></span>
            <span className={`switch ${noiseReductionEnabled ? "on" : ""}`}><i /></span>
          </button>
          {noiseReductionEnabled && (
            <div className="feature-settings silence-settings">
              <span>Reduction strength</span>
              <div className="strength-options">
                {(["weak", "normal", "strong"] as const).map((strength) => (
                  <button className={noiseStrength === strength ? "active" : ""} type="button" key={strength} onClick={() => setNoiseStrength(strength)}>{strength[0].toUpperCase() + strength.slice(1)}</button>
                ))}
              </div>
              <p>Strong settings remove more noise but can make voices sound thin.</p>
            </div>
          )}
        </div>
        <div className={`feature-card ${audioNormalizationEnabled ? "selected" : ""}`}>
          <button className="feature-main" type="button" onClick={() => setAudioNormalizationEnabled(!audioNormalizationEnabled)}>
            <span className="feature-icon"><Volume2 size={21} /></span>
            <span><strong>Normalize audio</strong><small>Balance loudness for comfortable playback</small></span>
            <span className={`switch ${audioNormalizationEnabled ? "on" : ""}`}><i /></span>
          </button>
        </div>
        <div className={`feature-card ${reframeEnabled ? "selected" : ""}`}>
          <button className="feature-main" type="button" onClick={() => { setReframeEnabled(!reframeEnabled); if (!reframeEnabled && (preset === "youtube" || preset === "x")) setPreset("shorts"); }}>
            <span className="feature-icon"><Smartphone size={21} /></span>
            <span><strong>Vertical 9:16 · follow faces</strong><small>Crop a landscape video to portrait and keep the speaker in frame</small></span>
            <span className={`switch ${reframeEnabled ? "on" : ""}`}><i /></span>
          </button>
          {reframeEnabled && (
            <div className="feature-settings">
              <p className="tool-ready">{alreadyPortrait ? "This video is already vertical, so it will not be cropped." : "Faces are found on this PC and the crop follows them smoothly. Without a face, the crop stays centered."}</p>
            </div>
          )}
        </div>
        <div className={`feature-card ${autoZoom !== "none" ? "selected" : ""}`}>
          <button className="feature-main" type="button" onClick={() => setAutoZoom(autoZoom === "none" ? "natural" : "none")}>
            <span className="feature-icon"><ZoomIn size={21} /></span>
            <span><strong>Auto zoom</strong><small>Add a light, regular push-in so the video feels less static</small></span>
            <span className={`switch ${autoZoom !== "none" ? "on" : ""}`}><i /></span>
          </button>
          {autoZoom !== "none" && (
            <div className="feature-settings silence-settings">
              <span>Zoom style</span>
              <div className="strength-options">
                {ZOOM_TEMPLATES.map((item) => (
                  <button className={autoZoom === item.id ? "active" : ""} type="button" key={item.id} title={item.hint} onClick={() => setAutoZoom(item.id)}>{item.label}</button>
                ))}
              </div>
              <p>Zoom never exceeds 12% and returns to 100% between pushes.</p>
            </div>
          )}
        </div>
        <div className={`feature-card ${subtitlesEnabled ? "selected" : ""}`}>
          <button className="feature-main" type="button" onClick={() => setSubtitlesEnabled(!subtitlesEnabled)}>
            <span className="feature-icon"><Captions size={21} /></span>
            <span><strong>Automatic subtitles</strong><small>Create an SRT file with local whisper.cpp</small></span>
            <span className={`switch ${subtitlesEnabled ? "on" : ""}`}><i /></span>
          </button>
          {subtitlesEnabled && (
            <div className="feature-settings">
              <label className="burn-in-field">Video subtitles<button className={`compact-toggle ${burnInSubtitles ? "active" : ""}`} type="button" disabled={burnInAvailable === false} onClick={() => setBurnInSubtitles(!burnInSubtitles)}><span className="mini-switch"><i /></span>{burnInAvailable === false ? "SRT only (FFmpeg lacks libass)" : burnInSubtitles ? "Burn into video + SRT" : "SRT file only"}</button></label>
              <div className="strength-options template-options">
                {SUBTITLE_TEMPLATES.map((item) => (
                  <button className={subtitleTemplate === item.id ? "active" : ""} type="button" key={item.id} onClick={() => setSubtitleTemplate(item.id)}>{item.label}</button>
                ))}
              </div>
              <p>Styled captions are also saved next to the video as an .ass file.</p>
            </div>
          )}
        </div>
        {(subtitlesEnabled || transcriptEditEnabled || highlightEnabled) && (
          <div className="feature-card selected">
            <div className="feature-settings">
              <label>Spoken language<select value={subtitleLanguage} onChange={(event) => setSubtitleLanguage(event.target.value as "auto" | "ja" | "en")}><option value="ja">Japanese</option><option value="en">English</option><option value="auto">Auto detect</option></select></label>
              <p className={whisperStatus?.available ? "tool-ready" : "tool-missing"}>{whisperStatus?.available ? `Ready: ${whisperStatus.executable}` : "whisper-cli is not available on PATH"}</p>
              <div className="model-manager">
                <div className="model-manager-title"><strong>Whisper model</strong><span>Downloaded only when you choose one</span></div>
                <div className="model-list">
                  {whisperModels.map((model) => {
                    const selected = model.path === subtitleModelPath;
                    const downloading = downloadingModelId === model.id;
                    return (
                      <div className={`model-row ${selected ? "selected" : ""}`} key={model.id}>
                        <button className="model-info" type="button" disabled={!model.installed} onClick={() => model.path && setSubtitleModelPath(model.path)}>
                          <span><strong>{model.name}{model.recommended && <em>RECOMMENDED</em>}</strong><small>{model.description} · {formatBytes(model.sizeBytes)}</small></span>
                        </button>
                        {downloading ? (
                          <button className="model-action cancel" type="button" onClick={() => cancelModelDownload(model.id)}><X size={14} /> Cancel</button>
                        ) : model.installed ? (
                          <button className="model-action remove" type="button" disabled={processing} aria-label={`Remove ${model.name}`} onClick={() => deleteModel(model)}><Trash2 size={14} /></button>
                        ) : (
                          <button className="model-action" type="button" disabled={downloadingModelId !== null} onClick={() => downloadModel(model.id)}><Download size={14} /> Download</button>
                        )}
                      </div>
                    );
                  })}
                </div>
                {downloadingModelId && modelProgress?.modelId === downloadingModelId && (
                  <div className="model-download-progress">
                    <div><span>{modelProgress.message}</span><strong>{Math.round(modelProgress.percent)}%</strong></div>
                    <div className="progress-track"><span style={{ width: `${modelProgress.percent}%` }} /></div>
                    <small>{formatBytes(modelProgress.downloadedBytes)} / {formatBytes(modelProgress.totalBytes)}</small>
                  </div>
                )}
                <button className="custom-model-button" type="button" onClick={chooseSubtitleModel}><FolderOpen size={14} /> Use a custom local .bin model</button>
              </div>
            </div>
          </div>
        )}

        <div className="section-heading"><div><span>02</span><div><h2>Choose a destination</h2><p>DropCut applies sensible settings automatically</p></div></div></div>
        <div className="preset-grid">
          {PRESETS.map((item) => (
            <button key={item.id} className={`preset-card ${preset === item.id ? "selected" : ""}`} onClick={() => { setPreset(item.id); if (item.id === "custom") setAdvanced(true); if (reframeEnabled && (item.id === "youtube" || item.id === "x")) setReframeEnabled(false); }} type="button">
              <span className="radio">{preset === item.id && <span />}</span>
              <strong>{item.label}</strong>
              <small>{item.hint}</small>
              {item.ratio && <em>{item.ratio}</em>}
            </button>
          ))}
        </div>

        {preset === "discord" && (
          <div className="size-row"><span>Maximum file size</span><div>{[10, 25, 50, 100].map((size) => <button className={maxSizeMb === size ? "active" : ""} onClick={() => setMaxSizeMb(size)} key={size}>{size} MB</button>)}</div></div>
        )}

        <div className="section-heading"><div><span>03</span><div><h2>Choose quality</h2><p>Recommended works well for most videos</p></div></div></div>
        <div className="quality-row">
          {(["small", "recommended", "high"] as Quality[]).map((value) => (
            <button key={value} className={quality === value ? "selected" : ""} onClick={() => setQuality(value)}>
              <span className="radio">{quality === value && <span />}</span>
              <strong>{value === "small" ? "Smaller file" : value === "recommended" ? "Recommended" : "High quality"}</strong>
              {value === "recommended" && <em>BEST</em>}
            </button>
          ))}
        </div>

        <button className="advanced-toggle" onClick={() => setAdvanced(!advanced)}><Settings2 size={17} /> Advanced settings <ChevronDown className={advanced ? "turned" : ""} size={17} /></button>
        {advanced && (
          <div className="advanced-panel">
            {silenceRemovalEnabled && (
              <div className="advanced-group">
                <strong>Silence removal</strong>
                <div className="custom-grid silence-advanced-grid">
                  <label>Threshold<input type="number" min="-60" max="-10" step="1" value={silenceThresholdDb} onChange={(event) => { setSilenceStrength("custom"); setSilenceThresholdDb(Number(event.target.value)); }} /><small>dB</small></label>
                  <label>Minimum silence<input type="number" min="0.1" max="10" step="0.1" value={minimumSilenceDuration} onChange={(event) => { setSilenceStrength("custom"); setMinimumSilenceDuration(Number(event.target.value)); }} /><small>sec</small></label>
                  <label>Padding<input type="number" min="0" max="2" step="0.05" value={silencePadding} onChange={(event) => { setSilenceStrength("custom"); setSilencePadding(Number(event.target.value)); }} /><small>sec</small></label>
                </div>
              </div>
            )}
            {subtitlesEnabled && (
              <div className="advanced-group">
                <strong>Subtitle style</strong>
                <div className="custom-grid silence-advanced-grid">
                  <label>Font<select value={subtitleFont} onChange={(event) => setSubtitleFont(event.target.value)}><option value="">Auto</option>{SUBTITLE_FONTS.map((font) => <option key={font} value={font}>{font}</option>)}</select></label>
                  <label>Size<input type="number" min="2" max="12" step="0.5" placeholder="Auto" value={subtitleSize} onChange={(event) => setSubtitleSize(event.target.value)} /><small>%</small></label>
                  <label>Position<select value={subtitlePosition} onChange={(event) => setSubtitlePosition(event.target.value as "bottom" | "middle" | "top")}><option value="bottom">Bottom</option><option value="middle">Middle</option><option value="top">Top</option></select></label>
                  <label>Stroke<input type="number" min="0" max="10" step="0.5" placeholder="Auto" value={subtitleOutline} onChange={(event) => setSubtitleOutline(event.target.value)} /><small>px</small></label>
                  <label>Shadow<input type="number" min="0" max="8" step="0.5" placeholder="Auto" value={subtitleShadow} onChange={(event) => setSubtitleShadow(event.target.value)} /><small>px</small></label>
                  <label>Background<select value={subtitleBackground} onChange={(event) => setSubtitleBackground(event.target.value as "template" | "on" | "off")}><option value="template">Style default</option><option value="on">Box</option><option value="off">None</option></select></label>
                  <label>Line length<input type="number" min="6" max="60" step="1" placeholder="Auto" value={subtitleMaxChars} onChange={(event) => setSubtitleMaxChars(event.target.value)} /><small>chars</small></label>
                </div>
              </div>
            )}
            {audioNormalizationEnabled && (
              <div className="advanced-group">
                <strong>Audio normalization</strong>
                <div className="custom-grid audio-advanced-grid">
                  <label>Target loudness<input type="number" min="-24" max="-5" step="0.5" value={targetLufs} onChange={(event) => setTargetLufs(Number(event.target.value))} /><small>LUFS</small></label>
                  <label>Loudness range<input type="number" min="1" max="20" step="1" value={loudnessRange} onChange={(event) => setLoudnessRange(Number(event.target.value))} /><small>LU</small></label>
                  <label>True peak<input type="number" min="-9" max="0" step="0.1" value={truePeakDb} onChange={(event) => setTruePeakDb(Number(event.target.value))} /><small>dB</small></label>
                </div>
              </div>
            )}
            {preset === "custom" ? (
              <div className="custom-grid">
                <label>Width<input type="number" min="240" max="7680" value={customWidth} onChange={(event) => setCustomWidth(Number(event.target.value))} /><small>px</small></label>
                <label>Height<input type="number" min="240" max="4320" value={customHeight} onChange={(event) => setCustomHeight(Number(event.target.value))} /><small>px</small></label>
                <label>FPS<input type="number" min="1" max="120" value={customFps} onChange={(event) => setCustomFps(Number(event.target.value))} /></label>
                <label>Codec<select value={customCodec} onChange={(event) => setCustomCodec(event.target.value as "h264" | "h265")}><option value="h264">H.264 (compatible)</option><option value="h265">H.265 (smaller)</option></select></label>
                <label className="wide">Video bitrate<input type="number" min="100" max="100000" step="100" value={customBitrate} onChange={(event) => setCustomBitrate(Number(event.target.value))} /><small>kbps</small></label>
              </div>
            ) : (
              <>
                <div><label>Output format</label><span>MP4 (H.264 / AAC)</span></div>
                <div><label>Output size</label><span>{selectedPreset.hint}</span></div>
              </>
            )}
          </div>
        )}

        {error && <div className="error-box"><X size={18} /><span>{error}</span></div>}
        {progress && (
          <div className={`progress-card ${progress.stage}`}>
            <div><strong>{progress.message}</strong><span>{Math.round(progress.percent)}%</span></div>
            <div className="progress-track"><span style={{ width: `${progress.percent}%` }} /></div>
            {progress.outputPath && <small title={progress.outputPath}>Saved to: {progress.outputPath}</small>}
          </div>
        )}

        <div className="action-area">
          {processing ? (
            <button className="cancel-button" onClick={cancel}><X size={18} /> Cancel processing</button>
          ) : (
            <button className="export-button" disabled={!video} onClick={exportVideo}><Sparkles size={19} /> Create video</button>
          )}
          <p><ShieldCheck size={15} /> Your original video is never modified</p>
        </div>
      </section>

      <footer><span><HardDrive size={15} /> Local processing</span><span>Free · No watermark · No upload</span><button onClick={() => { setVideo(null); setProgress(null); setError(null); }}><RotateCcw size={14} /> Reset</button></footer>
    </main>
  );
}
