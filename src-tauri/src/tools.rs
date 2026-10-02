use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tokio::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
    Whisper,
    Llama,
}

impl Tool {
    fn base_name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
            Self::Whisper => "whisper-cli",
            Self::Llama => "llama-completion",
        }
    }

    fn probe_argument(self) -> &'static str {
        match self {
            Self::Ffmpeg | Self::Ffprobe => "-version",
            Self::Whisper => "-h",
            Self::Llama => "--version",
        }
    }

    fn filename(self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.base_name())
        } else {
            self.base_name().to_string()
        }
    }
}

fn bundled_candidates(app: &AppHandle, tool: Tool) -> Vec<PathBuf> {
    let filename = tool.filename();
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("tools").join(&filename));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("tools")
            .join(filename),
    );
    candidates
}

async fn responds_to_probe(path: &Path, tool: Tool) -> bool {
    Command::new(path)
        .arg(tool.probe_argument())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

pub async fn resolve(app: &AppHandle, tool: Tool) -> Option<PathBuf> {
    for candidate in bundled_candidates(app, tool) {
        if candidate.is_file() && responds_to_probe(&candidate, tool).await {
            return Some(candidate);
        }
    }
    let path_command = PathBuf::from(tool.filename());
    responds_to_probe(&path_command, tool)
        .await
        .then_some(path_command)
}

/// Bundled GGUF model used to re-rank highlights.
pub const LLM_MODEL_FILE: &str = "qwen2.5-1.5b-instruct-q4_k_m.gguf";

pub fn bundled_llm_model(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("llm").join(LLM_MODEL_FILE));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("llm")
            .join(LLM_MODEL_FILE),
    );
    candidates.into_iter().find(|path| path.is_file())
}

pub fn command(path: &Path) -> Command {
    Command::new(path)
}

/// A GPU video encoder FFmpeg can use instead of the CPU (`libx264`/`libx265`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HwEncoder {
    Nvenc,
    Qsv,
    Amf,
    VideoToolbox,
}

impl HwEncoder {
    /// The FFmpeg encoder name for the requested codec ("h264" or "h265").
    pub fn encoder_name(self, codec: &str) -> &'static str {
        let hevc = codec == "h265";
        match (self, hevc) {
            (Self::Nvenc, false) => "h264_nvenc",
            (Self::Nvenc, true) => "hevc_nvenc",
            (Self::Qsv, false) => "h264_qsv",
            (Self::Qsv, true) => "hevc_qsv",
            (Self::Amf, false) => "h264_amf",
            (Self::Amf, true) => "hevc_amf",
            (Self::VideoToolbox, false) => "h264_videotoolbox",
            (Self::VideoToolbox, true) => "hevc_videotoolbox",
        }
    }
}

async fn encoder_works(ffmpeg: &Path, encoder: &str) -> bool {
    Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=128x128:d=0.1",
            "-frames:v",
            "1",
            "-c:v",
            encoder,
            "-f",
            "null",
            "-",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

/// Probes for a working GPU encoder by actually running a tiny test encode
/// (FFmpeg can be built with an encoder listed that still fails without the
/// matching driver, e.g. on a machine with no NVIDIA GPU).
async fn detect_hw_encoder(ffmpeg: &Path) -> Option<HwEncoder> {
    let candidates: &[(HwEncoder, &str)] = if cfg!(target_os = "macos") {
        &[(HwEncoder::VideoToolbox, "h264_videotoolbox")]
    } else {
        &[
            (HwEncoder::Nvenc, "h264_nvenc"),
            (HwEncoder::Qsv, "h264_qsv"),
            (HwEncoder::Amf, "h264_amf"),
        ]
    };
    for (encoder, name) in candidates {
        if encoder_works(ffmpeg, name).await {
            return Some(*encoder);
        }
    }
    None
}

static HW_ENCODER: tokio::sync::OnceCell<Option<HwEncoder>> = tokio::sync::OnceCell::const_new();

/// The machine's best available GPU encoder, detected once and cached for
/// the life of the app.
pub async fn cached_hw_encoder(ffmpeg: &Path) -> Option<HwEncoder> {
    *HW_ENCODER.get_or_init(|| detect_hw_encoder(ffmpeg)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_fixed_and_argument_free() {
        for tool in [Tool::Ffmpeg, Tool::Ffprobe, Tool::Whisper] {
            let name = tool.filename();
            assert!(!name.contains('/') && !name.contains('\\'));
            assert!(!name.contains(' '));
            assert!(tool.probe_argument().starts_with('-'));
        }
    }
}
