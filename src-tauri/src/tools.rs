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
