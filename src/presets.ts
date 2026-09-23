import type { PresetId } from "./types";

export const PRESETS: Array<{ id: PresetId; label: string; hint: string; ratio?: string }> = [
  { id: "original", label: "Original", hint: "Keep size and orientation" },
  { id: "youtube", label: "YouTube", hint: "Landscape · 1080p", ratio: "16:9" },
  { id: "shorts", label: "Shorts", hint: "Portrait · 1080p", ratio: "9:16" },
  { id: "tiktok", label: "TikTok", hint: "Portrait · 1080p", ratio: "9:16" },
  { id: "discord", label: "Discord", hint: "Fit a size limit" },
  { id: "x", label: "X", hint: "Sharing · 720p", ratio: "16:9" },
  { id: "instagram", label: "Instagram", hint: "Feed · 4:5", ratio: "4:5" },
  { id: "custom", label: "Custom", hint: "Choose every setting" },
];

export const SUPPORTED_EXTENSIONS = ["mp4", "mov", "webm", "mkv", "avi"];
