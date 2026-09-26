use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Clone, Copy)]
struct ModelSpec {
    id: &'static str,
    name: &'static str,
    filename: &'static str,
    description: &'static str,
    size_bytes: u64,
    sha256: &'static str,
    recommended: bool,
}

const MODELS: [ModelSpec; 5] = [
    ModelSpec {
        id: "tiny",
        name: "Whisper Tiny",
        filename: "ggml-tiny.bin",
        description: "Fastest, suitable for drafts",
        size_bytes: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        recommended: false,
    },
    ModelSpec {
        id: "base",
        name: "Whisper Base",
        filename: "ggml-base.bin",
        description: "Balanced speed and accuracy",
        size_bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        recommended: false,
    },
    ModelSpec {
        id: "small",
        name: "Whisper Small",
        filename: "ggml-small.bin",
        description: "More accurate, slower on CPU",
        size_bytes: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        recommended: false,
    },
    ModelSpec {
        id: "medium",
        name: "Whisper Medium",
        filename: "ggml-medium.bin",
        description: "Highest accuracy, requires more memory",
        size_bytes: 1_533_763_059,
        sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        recommended: false,
    },
    ModelSpec {
        id: "large-v3-turbo",
        name: "Whisper Large v3 Turbo",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        description: "Best accuracy for Japanese, compressed (q5_0)",
        size_bytes: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        recommended: true,
    },
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    size_bytes: u64,
    recommended: bool,
    installed: bool,
    path: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    model_id: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
    percent: f64,
    status: &'static str,
    message: String,
}

pub enum DownloadResult {
    Complete(PathBuf),
    Cancelled,
}

fn model_spec(id: &str) -> Result<ModelSpec, String> {
    MODELS
        .iter()
        .find(|model| model.id == id)
        .copied()
        .ok_or_else(|| "Unknown whisper.cpp model.".to_string())
}

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("models").join("whisper"))
        .map_err(|error| format!("The model directory is unavailable: {error}"))
}

fn emit_progress(
    app: &AppHandle,
    model: ModelSpec,
    downloaded_bytes: u64,
    status: &'static str,
    message: impl Into<String>,
) {
    let percent = if model.size_bytes == 0 {
        0.0
    } else {
        (downloaded_bytes as f64 / model.size_bytes as f64 * 100.0).clamp(0.0, 100.0)
    };
    let _ = app.emit(
        "model-download-progress",
        DownloadProgress {
            model_id: model.id,
            downloaded_bytes,
            total_bytes: model.size_bytes,
            percent,
            status,
            message: message.into(),
        },
    );
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub async fn list_models(app: &AppHandle) -> Result<Vec<ModelInfo>, String> {
    let directory = models_dir(app)?;
    let mut models = Vec::with_capacity(MODELS.len());
    for model in MODELS {
        let path = directory.join(model.filename);
        let installed = tokio::fs::metadata(&path)
            .await
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == model.size_bytes);
        models.push(ModelInfo {
            id: model.id,
            name: model.name,
            description: model.description,
            size_bytes: model.size_bytes,
            recommended: model.recommended,
            installed,
            path: installed.then(|| path.to_string_lossy().into_owned()),
        });
    }
    Ok(models)
}

async fn download_to_temp(
    app: &AppHandle,
    model: ModelSpec,
    temporary_path: &Path,
    cancelled: &AtomicBool,
) -> Result<DownloadResult, String> {
    let url = format!("{MODEL_BASE_URL}/{}", model.filename);
    let response = reqwest::Client::builder()
        .user_agent("DropCut/0.1")
        .build()
        .map_err(|error| format!("The model downloader could not start: {error}"))?
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("The model download failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length != model.size_bytes)
    {
        return Err("The server returned an unexpected model size.".into());
    }
    let mut file = tokio::fs::File::create(temporary_path)
        .await
        .map_err(|error| format!("The model file could not be created: {error}"))?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0_u64;
    let mut hasher = Sha256::new();
    let mut last_reported_percent = -1_i32;
    while let Some(chunk) = stream.next().await {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(DownloadResult::Cancelled);
        }
        let chunk =
            chunk.map_err(|error| format!("The model download was interrupted: {error}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("The model file could not be written: {error}"))?;
        hasher.update(&chunk);
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > model.size_bytes {
            return Err("The model download exceeded the expected size.".into());
        }
        let percent = (downloaded as f64 / model.size_bytes as f64 * 100.0) as i32;
        if percent != last_reported_percent {
            last_reported_percent = percent;
            emit_progress(app, model, downloaded, "downloading", "Downloading model");
        }
    }
    file.flush()
        .await
        .map_err(|error| format!("The model file could not be finalized: {error}"))?;
    file.sync_all()
        .await
        .map_err(|error| format!("The model file could not be finalized: {error}"))?;
    drop(file);
    if downloaded != model.size_bytes {
        return Err(format!(
            "The model size is invalid. Expected {} bytes but received {downloaded}.",
            model.size_bytes
        ));
    }
    emit_progress(
        app,
        model,
        downloaded,
        "verifying",
        "Verifying model integrity",
    );
    let actual_hash = hex_encode(&hasher.finalize());
    if actual_hash != model.sha256 {
        return Err("The model checksum is invalid. The downloaded file was rejected.".into());
    }
    Ok(DownloadResult::Complete(temporary_path.to_path_buf()))
}

pub async fn download_model(
    app: &AppHandle,
    id: &str,
    cancelled: &AtomicBool,
) -> Result<DownloadResult, String> {
    let model = model_spec(id)?;
    let directory = models_dir(app)?;
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| format!("The model directory could not be created: {error}"))?;
    let final_path = directory.join(model.filename);
    if tokio::fs::metadata(&final_path)
        .await
        .is_ok_and(|metadata| metadata.len() == model.size_bytes)
    {
        return Ok(DownloadResult::Complete(final_path));
    }
    let temporary_path = directory.join(format!("{}.part", model.filename));
    let _ = tokio::fs::remove_file(&temporary_path).await;
    emit_progress(app, model, 0, "starting", "Starting model download");
    let result = download_to_temp(app, model, &temporary_path, cancelled).await;
    match result {
        Ok(DownloadResult::Complete(_)) => {
            tokio::fs::rename(&temporary_path, &final_path)
                .await
                .map_err(|error| format!("The model could not be installed: {error}"))?;
            emit_progress(app, model, model.size_bytes, "done", "Model ready");
            Ok(DownloadResult::Complete(final_path))
        }
        Ok(DownloadResult::Cancelled) => {
            let _ = tokio::fs::remove_file(&temporary_path).await;
            emit_progress(app, model, 0, "cancelled", "Model download cancelled");
            Ok(DownloadResult::Cancelled)
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary_path).await;
            emit_progress(app, model, 0, "error", "Model download failed");
            Err(error)
        }
    }
}

pub async fn delete_model(app: &AppHandle, id: &str) -> Result<(), String> {
    let model = model_spec(id)?;
    let path = models_dir(app)?.join(model.filename);
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("The model could not be removed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_unique_safe_model_names() {
        for (index, model) in MODELS.iter().enumerate() {
            assert!(model.filename.starts_with("ggml-"));
            assert!(model.filename.ends_with(".bin"));
            assert_eq!(model.sha256.len(), 64);
            assert!(model.size_bytes > 0);
            assert!(!MODELS[..index].iter().any(|other| other.id == model.id));
        }
    }

    #[test]
    fn unknown_models_are_rejected() {
        assert!(model_spec("../../untrusted").is_err());
    }

    #[test]
    fn digest_bytes_are_encoded_as_lowercase_hex() {
        assert_eq!(hex_encode(&[0x00, 0x7f, 0xa5, 0xff]), "007fa5ff");
    }
}
