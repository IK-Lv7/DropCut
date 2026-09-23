export type PresetId = "original" | "youtube" | "shorts" | "tiktok" | "discord" | "x" | "instagram" | "custom";
export type Quality = "small" | "recommended" | "high";

export interface VideoInfo {
  path: string;
  name: string;
  durationSeconds: number;
  width: number;
  height: number;
  sizeBytes: number;
  codec: string;
  hasAudio: boolean;
  fps: number;
}

export interface ExportSettings {
  preset: PresetId;
  quality: Quality;
  maxSizeMb: number | null;
  width: number | null;
  height: number | null;
  fps: number | null;
  codec: "h264" | "h265";
  bitrateKbps: number | null;
  subtitles: SubtitleSettings;
  silenceRemoval: SilenceRemovalSettings;
  audioNormalization: AudioNormalizationSettings;
  noiseReduction: NoiseReductionSettings;
  reframe: ReframeSettings;
  cutRanges: CutRange[];
  autoZoom: AutoZoomSettings;
}

export type AutoZoomTemplate = "none" | "natural" | "youtube" | "shorts";

export interface AutoZoomSettings {
  template: AutoZoomTemplate;
}

export type SubtitleTemplate = "simple" | "youtube" | "tiktok" | "gaming" | "minimal" | "pop";

/** Any null field falls back to the template's own value. */
export interface SubtitleStyle {
  template: SubtitleTemplate;
  font: string | null;
  sizePercent: number | null;
  outline: number | null;
  shadow: number | null;
  position: "bottom" | "middle" | "top";
  background: boolean | null;
  maxChars: number | null;
}

export type Strength = "weak" | "normal" | "strong";

export interface NoiseReductionSettings {
  enabled: boolean;
  strength: Strength;
}

export interface ReframeSettings {
  enabled: boolean;
}

/** A section of the source video, in seconds, that will be removed. */
export interface CutRange {
  start: number;
  end: number;
}

export interface TranscriptWord {
  start: number;
  end: number;
  text: string;
  /** "high": hesitation sound. "low": may be real speech, needs review. */
  filler: "high" | "low" | null;
}

export interface Highlight {
  start: number;
  end: number;
  score: number;
  reason: string;
  title: string | null;
}

export interface SilenceRemovalSettings {
  enabled: boolean;
  thresholdDb: number;
  minimumDurationSeconds: number;
  paddingSeconds: number;
}

export interface AudioNormalizationSettings {
  enabled: boolean;
  targetLufs: number;
  loudnessRange: number;
  truePeakDb: number;
}

export interface SubtitleSettings {
  enabled: boolean;
  modelPath: string | null;
  language: "auto" | "ja" | "en";
  burnIn: boolean;
  style: SubtitleStyle;
}

export interface WhisperModel {
  id: string;
  name: string;
  description: string;
  sizeBytes: number;
  recommended: boolean;
  installed: boolean;
  path: string | null;
}

export interface ModelDownloadProgress {
  modelId: string;
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
  status: "starting" | "downloading" | "verifying" | "done" | "cancelled" | "error";
  message: string;
}

export interface ToolStatus {
  available: boolean;
  executable: string | null;
}

export interface ProgressEvent {
  jobId: string;
  stage: "preparing" | "detectingSilence" | "trackingFaces" | "analyzingAudio" | "analyzing" | "extractingAudio" | "transcribing" | "encoding" | "verifying" | "done" | "cancelled" | "error";
  percent: number;
  message: string;
  outputPath?: string;
}
