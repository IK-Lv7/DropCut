mod highlight;
mod llm;
mod models;
mod reframe;
mod subtitles;
mod tools;
mod transcript;

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
    time::{sleep, Duration},
};
use tools::Tool;

const SUPPORTED_EXTENSIONS: [&str; 5] = ["mp4", "mov", "webm", "mkv", "avi"];

#[derive(Default)]
struct JobManager(Mutex<HashMap<String, Arc<AtomicBool>>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VideoInfo {
    path: String,
    name: String,
    duration_seconds: f64,
    width: u64,
    height: u64,
    size_bytes: u64,
    codec: String,
    has_audio: bool,
    fps: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportSettings {
    preset: String,
    quality: String,
    max_size_mb: Option<f64>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    codec: String,
    bitrate_kbps: Option<u32>,
    #[serde(default)]
    subtitles: SubtitleSettings,
    #[serde(default)]
    silence_removal: SilenceRemovalSettings,
    #[serde(default)]
    audio_normalization: AudioNormalizationSettings,
    #[serde(default)]
    noise_reduction: NoiseReductionSettings,
    #[serde(default)]
    reframe: ReframeSettings,
    #[serde(default)]
    auto_zoom: AutoZoomSettings,
    /// Sections (source timeline, seconds) the user removed by editing the
    /// transcript or accepting filler-word suggestions.
    #[serde(default)]
    cut_ranges: Vec<TimeRange>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NoiseReductionSettings {
    enabled: bool,
    strength: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReframeSettings {
    enabled: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoZoomSettings {
    template: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleSettings {
    enabled: bool,
    model_path: Option<String>,
    language: String,
    burn_in: bool,
    #[serde(default)]
    style: subtitles::SubtitleStyleSettings,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SilenceRemovalSettings {
    enabled: bool,
    threshold_db: f64,
    minimum_duration_seconds: f64,
    padding_seconds: f64,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioNormalizationSettings {
    enabled: bool,
    target_lufs: f64,
    loudness_range: f64,
    true_peak_db: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LoudnessMeasurements {
    input_i: f64,
    input_lra: f64,
    input_tp: f64,
    input_thresh: f64,
    target_offset: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
struct TimeRange {
    start: f64,
    end: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolStatus {
    available: bool,
    executable: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    job_id: String,
    stage: String,
    percent: f64,
    message: String,
    output_path: Option<String>,
}

fn friendly_error(message: &str) -> String {
    eprintln!("[DropCut] {message}");
    "The video could not be processed. Check that the file is valid and FFmpeg is available.".into()
}

fn validate_input(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return Err("This file type is not supported.".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "The video file could not be found.".to_string())?;
    if !canonical.is_file() {
        return Err("The selected path is not a video file.".into());
    }
    Ok(canonical)
}

#[tauri::command]
async fn check_ffmpeg(app: AppHandle) -> bool {
    let (ffmpeg, ffprobe) = tokio::join!(
        tools::resolve(&app, Tool::Ffmpeg),
        tools::resolve(&app, Tool::Ffprobe)
    );
    ffmpeg.is_some() && ffprobe.is_some()
}

async fn supports_subtitle_burn_in(app: &AppHandle) -> bool {
    let Some(ffmpeg) = tools::resolve(app, Tool::Ffmpeg).await else {
        return false;
    };
    let output = tools::command(&ffmpeg)
        .args(["-hide_banner", "-filters"])
        .stderr(Stdio::null())
        .output()
        .await;
    output.is_ok_and(|result| {
        result.status.success()
            && String::from_utf8_lossy(&result.stdout)
                .lines()
                .any(|line| line.split_whitespace().nth(1) == Some("subtitles"))
    })
}

#[tauri::command]
async fn check_subtitle_burn_in(app: AppHandle) -> bool {
    supports_subtitle_burn_in(&app).await
}

async fn find_whisper_cli(app: &AppHandle) -> Option<PathBuf> {
    tools::resolve(app, Tool::Whisper).await
}

#[tauri::command]
async fn check_whisper(app: AppHandle) -> ToolStatus {
    let executable = find_whisper_cli(&app).await;
    ToolStatus {
        available: executable.is_some(),
        executable: executable.map(|path| path.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
async fn list_whisper_models(app: AppHandle) -> Result<Vec<models::ModelInfo>, String> {
    models::list_models(&app).await
}

#[tauri::command]
async fn download_whisper_model(
    app: AppHandle,
    jobs: State<'_, JobManager>,
    model_id: String,
) -> Result<Option<String>, String> {
    let job_id = format!("model-{model_id}");
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut map = jobs
            .0
            .lock()
            .map_err(|_| "The model download could not be started.".to_string())?;
        if map.contains_key(&job_id) {
            return Err("This model is already being downloaded.".into());
        }
        map.insert(job_id.clone(), cancelled.clone());
    }
    let result = models::download_model(&app, &model_id, &cancelled).await;
    jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
    match result? {
        models::DownloadResult::Complete(path) => Ok(Some(path.to_string_lossy().into_owned())),
        models::DownloadResult::Cancelled => Ok(None),
    }
}

#[tauri::command]
fn cancel_model_download(jobs: State<'_, JobManager>, model_id: String) -> bool {
    let Ok(map) = jobs.0.lock() else {
        return false;
    };
    let job_id = format!("model-{model_id}");
    let Some(flag) = map.get(&job_id) else {
        return false;
    };
    flag.store(true, Ordering::Relaxed);
    true
}

#[tauri::command]
async fn delete_whisper_model(
    app: AppHandle,
    jobs: State<'_, JobManager>,
    model_id: String,
) -> Result<(), String> {
    let job_id = format!("model-{model_id}");
    if jobs
        .0
        .lock()
        .map_err(|_| "The model state is unavailable.".to_string())?
        .iter()
        .any(|(running_id, _)| running_id == &job_id || running_id.starts_with("export-"))
    {
        return Err("A model cannot be removed while it is downloading.".into());
    }
    models::delete_model(&app, &model_id).await
}

#[tauri::command]
async fn probe_video(app: AppHandle, path: String) -> Result<VideoInfo, String> {
    let input = validate_input(&path)?;
    let ffprobe = tools::resolve(&app, Tool::Ffprobe)
        .await
        .ok_or_else(|| "FFprobe is not available.".to_string())?;
    let output = tools::command(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height,codec_name,avg_frame_rate:stream_side_data=rotation:stream_tags=rotate:format=duration",
            "-of",
            "json",
        ])
        .arg(&input)
        .output()
        .await
        .map_err(|error| friendly_error(&error.to_string()))?;
    if !output.status.success() {
        return Err(friendly_error(&String::from_utf8_lossy(&output.stderr)));
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| friendly_error(&error.to_string()))?;
    let streams = json["streams"]
        .as_array()
        .ok_or_else(|| "No media tracks were found.".to_string())?;
    let stream = streams
        .iter()
        .find(|item| item["codec_type"].as_str() == Some("video"))
        .ok_or_else(|| "No video track was found.".to_string())?;
    let duration = json["format"]["duration"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0);
    let metadata = input
        .metadata()
        .map_err(|error| friendly_error(&error.to_string()))?;
    let (width, height) = display_size(
        stream["width"].as_u64().unwrap_or(0),
        stream["height"].as_u64().unwrap_or(0),
        stream_rotation(stream),
    );
    Ok(VideoInfo {
        path: input.to_string_lossy().into_owned(),
        name: input
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("video")
            .to_string(),
        duration_seconds: duration,
        width,
        height,
        size_bytes: metadata.len(),
        codec: stream["codec_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        has_audio: streams
            .iter()
            .any(|item| item["codec_type"].as_str() == Some("audio")),
        fps: parse_frame_rate(stream["avg_frame_rate"].as_str().unwrap_or("")),
    })
}

/// Parses ffprobe's `30000/1001` style rates; 0 when unknown.
fn parse_frame_rate(value: &str) -> f64 {
    let (numerator, denominator) = value.split_once('/').unwrap_or((value, "1"));
    match (numerator.parse::<f64>(), denominator.parse::<f64>()) {
        (Ok(numerator), Ok(denominator)) if denominator > 0.0 && numerator.is_finite() => {
            numerator / denominator
        }
        _ => 0.0,
    }
}

/// Rotation in degrees that players apply on display (phones store portrait
/// video as rotated landscape frames).
fn stream_rotation(stream: &serde_json::Value) -> i64 {
    stream["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|item| item["rotation"].as_f64())
        .or_else(|| {
            stream["tags"]["rotate"]
                .as_str()
                .and_then(|value| value.parse().ok())
        })
        .map_or(0, |value| value.round() as i64)
}

fn display_size(width: u64, height: u64, rotation: i64) -> (u64, u64) {
    if rotation.rem_euclid(180) == 90 {
        (height, width)
    } else {
        (width, height)
    }
}

fn unique_output(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("video");
    for suffix in 0..10_000 {
        let name = if suffix == 0 {
            format!("{stem}_edited.mp4")
        } else {
            format!("{stem}_edited_{suffix}.mp4")
        };
        let candidate = parent.join(name);
        if !candidate.exists()
            && !candidate.with_extension("srt").exists()
            && !candidate.with_extension("ass").exists()
        {
            return candidate;
        }
    }
    parent.join(format!("{stem}_edited_new.mp4"))
}

enum ProcessResult {
    Completed(ExitStatus),
    Cancelled,
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.as_std_mut().creation_flags(0x08000000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

async fn run_cancelable(
    command: &mut Command,
    cancelled: &AtomicBool,
) -> Result<ProcessResult, String> {
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    hide_console(command);
    let mut child = command
        .spawn()
        .map_err(|error| friendly_error(&error.to_string()))?;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            return Ok(ProcessResult::Cancelled);
        }
        match child
            .try_wait()
            .map_err(|error| friendly_error(&error.to_string()))?
        {
            Some(status) => return Ok(ProcessResult::Completed(status)),
            None => sleep(Duration::from_millis(150)).await,
        }
    }
}

fn validate_silence_settings(settings: &SilenceRemovalSettings) -> Result<(), String> {
    if !(-60.0..=-10.0).contains(&settings.threshold_db) {
        return Err("The silence threshold must be between -60 dB and -10 dB.".into());
    }
    if !(0.1..=10.0).contains(&settings.minimum_duration_seconds) {
        return Err("The minimum silence duration must be between 0.1 and 10 seconds.".into());
    }
    if !(0.0..=2.0).contains(&settings.padding_seconds) {
        return Err("Silence padding must be between 0 and 2 seconds.".into());
    }
    Ok(())
}

fn parse_silence_timestamp(line: &str, marker: &str) -> Option<f64> {
    let value = line.split_once(marker)?.1.split_whitespace().next()?;
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn build_keep_ranges(
    silences: &[TimeRange],
    duration: f64,
    padding: f64,
) -> Result<Vec<TimeRange>, String> {
    let mut keep = Vec::new();
    let mut cursor = 0.0_f64;
    let mut removed_anything = false;
    for silence in silences {
        let start = silence.start.clamp(0.0, duration);
        let end = silence.end.clamp(start, duration);
        let removal_start = (start + padding).min(end);
        let removal_end = (end - padding).max(start);
        if removal_end - removal_start < 0.02 {
            continue;
        }
        removed_anything = true;
        if removal_start - cursor >= 0.02 {
            keep.push(TimeRange {
                start: cursor,
                end: removal_start,
            });
        }
        cursor = cursor.max(removal_end);
    }
    if duration - cursor >= 0.02 {
        keep.push(TimeRange {
            start: cursor,
            end: duration,
        });
    }
    if !removed_anything {
        return Ok(Vec::new());
    }
    if keep.is_empty() {
        return Err("The entire video was detected as silence. Try a weaker setting.".into());
    }
    if keep.len() > MAX_KEEP_RANGES {
        return Err(
            "Too many silence cuts were detected. Try a weaker setting or a longer minimum duration."
                .into(),
        );
    }
    Ok(keep)
}

enum SilenceDetectionResult {
    Complete(Vec<TimeRange>),
    Cancelled,
}

async fn detect_silence(
    app: &AppHandle,
    input: &Path,
    duration: f64,
    settings: &SilenceRemovalSettings,
    cancelled: &AtomicBool,
    job_id: &str,
) -> Result<SilenceDetectionResult, String> {
    validate_silence_settings(settings)?;
    let ffmpeg = tools::resolve(app, Tool::Ffmpeg)
        .await
        .ok_or_else(|| "FFmpeg is not bundled and was not found on PATH.".to_string())?;
    emit_progress(
        app,
        job_id,
        "detectingSilence",
        3.0,
        "Detecting quiet sections",
        None,
    );
    let mut command = tools::command(&ffmpeg);
    command
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(input)
        .args(["-vn", "-af"])
        .arg(format!(
            "silencedetect=noise={}dB:d={}",
            settings.threshold_db, settings.minimum_duration_seconds
        ))
        .args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| friendly_error(&error.to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| friendly_error("FFmpeg silence detection output is unavailable"))?;
    let mut lines = BufReader::new(stderr).lines();
    let mut silence_start = None;
    let mut silences = Vec::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            return Ok(SilenceDetectionResult::Cancelled);
        }
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(text)) => {
                    if let Some(start) = parse_silence_timestamp(&text, "silence_start:") {
                        silence_start = Some(start);
                    }
                    if let Some(end) = parse_silence_timestamp(&text, "silence_end:") {
                        if let Some(start) = silence_start.take() {
                            if end > start {
                                silences.push(TimeRange { start, end });
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(error) => return Err(friendly_error(&error.to_string())),
            },
            _ = sleep(Duration::from_millis(150)) => {}
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|error| friendly_error(&error.to_string()))?;
    if !status.success() {
        return Err(
            "Silence detection failed. Check that the video has a readable audio track.".into(),
        );
    }
    if let Some(start) = silence_start {
        silences.push(TimeRange {
            start,
            end: duration,
        });
    }
    let keep = build_keep_ranges(&silences, duration, settings.padding_seconds)?;
    Ok(SilenceDetectionResult::Complete(keep))
}

const MAX_KEEP_RANGES: usize = 600;

struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn validate_job_id(job_id: &str) -> Result<(), String> {
    if job_id.len() > 80
        || !job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("Invalid job identifier.".into());
    }
    Ok(())
}

fn register_job(jobs: &JobManager, job_id: &str) -> Result<Arc<AtomicBool>, String> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut map = jobs
        .0
        .lock()
        .map_err(|_| "The job could not be started.".to_string())?;
    if map.contains_key(job_id) {
        return Err("A job with the same identifier is already running.".into());
    }
    map.insert(job_id.to_string(), cancelled.clone());
    Ok(cancelled)
}

fn validated_model(path: Option<&str>) -> Result<PathBuf, String> {
    let model = path
        .ok_or_else(|| "Choose a whisper.cpp model before enabling subtitles.".to_string())?;
    let model = Path::new(model)
        .canonicalize()
        .map_err(|_| "The selected whisper.cpp model could not be found.".to_string())?;
    if !model.is_file()
        || model
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("bin"))
    {
        return Err("The selected whisper.cpp model is not a valid .bin file.".into());
    }
    Ok(model)
}

fn validated_language(language: &str) -> Result<&str, String> {
    match language {
        "ja" | "en" | "auto" => Ok(language),
        _ => Err("Unsupported transcription language.".into()),
    }
}

/// Gentle cleanup so quiet or rumbly speech is easier for whisper to pick up.
const SPEECH_AUDIO_FILTER: &str = "highpass=f=80,dynaudnorm=f=150:g=15";
const VAD_MODEL: &[u8] = include_bytes!("../resources/models/ggml-silero-v5.1.2.bin");

/// Accuracy-oriented whisper options shared by subtitle and transcript runs:
/// beam search, no context carry-over (stops repeated hallucinations), and
/// Silero VAD so silence and background noise are not transcribed.
async fn add_accuracy_args(
    command: &mut Command,
    work_dir: &Path,
    use_vad: bool,
) -> Result<(), String> {
    command.args(["-bs", "5", "-mc", "0"]);
    if !use_vad {
        return Ok(());
    }
    let vad_path = work_dir.join("vad.bin");
    tokio::fs::write(&vad_path, VAD_MODEL)
        .await
        .map_err(|error| friendly_error(&error.to_string()))?;
    command.args(["--vad", "-vm"]).arg(&vad_path);
    Ok(())
}

/// Writes 16 kHz mono PCM for whisper, optionally only the kept sections.
async fn extract_audio_wav(
    ffmpeg: &Path,
    input: &Path,
    keep_ranges: &[TimeRange],
    audio_path: &Path,
    cancelled: &AtomicBool,
) -> Result<ProcessResult, String> {
    let mut extract = tools::command(ffmpeg);
    extract.args(["-hide_banner", "-y", "-i"]).arg(input);
    if keep_ranges.is_empty() {
        extract.args(["-vn", "-af", SPEECH_AUDIO_FILTER]);
    } else {
        let mut filters = Vec::with_capacity(keep_ranges.len() + 1);
        let mut inputs = String::new();
        for (index, range) in keep_ranges.iter().enumerate() {
            filters.push(format!(
                "[0:a]atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS[a{index}]",
                range.start, range.end
            ));
            inputs.push_str(&format!("[a{index}]"));
        }
        filters.push(format!(
            "{inputs}concat=n={}:v=0:a=1,{SPEECH_AUDIO_FILTER}[cut_audio]",
            keep_ranges.len()
        ));
        extract.args(["-filter_complex", &filters.join(";"), "-map", "[cut_audio]"]);
    }
    extract
        .args(["-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(audio_path);
    run_cancelable(&mut extract, cancelled).await
}

enum SubtitleResult {
    Created(PathBuf),
    Cancelled,
}

async fn create_subtitles(
    app: &AppHandle,
    input: &Path,
    output: &Path,
    settings: &SubtitleSettings,
    keep_ranges: &[TimeRange],
    cancelled: &AtomicBool,
    job_id: &str,
) -> Result<SubtitleResult, String> {
    let model = validated_model(settings.model_path.as_deref())?;
    let whisper = find_whisper_cli(app)
        .await
        .ok_or_else(|| "whisper-cli is not bundled and was not found on PATH.".to_string())?;
    let ffmpeg = tools::resolve(app, Tool::Ffmpeg)
        .await
        .ok_or_else(|| "FFmpeg is not bundled and was not found on PATH.".to_string())?;
    let work_dir = std::env::temp_dir().join("dropcut").join(job_id);
    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(|error| friendly_error(&error.to_string()))?;
    let audio_path = work_dir.join("audio.wav");
    emit_progress(
        app,
        job_id,
        "extractingAudio",
        20.0,
        "Extracting audio for subtitles",
        None,
    );
    let extract_result = match extract_audio_wav(&ffmpeg, input, keep_ranges, &audio_path, cancelled).await {
        Ok(result) => result,
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&work_dir).await;
            return Err(error);
        }
    };
    match extract_result {
        ProcessResult::Cancelled => {
            let _ = tokio::fs::remove_dir_all(&work_dir).await;
            return Ok(SubtitleResult::Cancelled);
        }
        ProcessResult::Completed(status) if !status.success() => {
            let _ = tokio::fs::remove_dir_all(&work_dir).await;
            return Err("Audio extraction for subtitle generation failed.".into());
        }
        ProcessResult::Completed(_) => {}
    }
    emit_progress(
        app,
        job_id,
        "transcribing",
        25.0,
        "Transcribing locally with whisper.cpp",
        None,
    );
    let output_base = output
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".dropcut-{job_id}-subtitles"));
    let language = match settings.language.as_str() {
        "ja" | "en" | "auto" => settings.language.as_str(),
        _ => {
            let _ = tokio::fs::remove_dir_all(&work_dir).await;
            return Err("Unsupported transcription language.".into());
        }
    };
    let subtitle_path = output_base.with_extension("srt");
    // VAD can classify a quiet or noisy recording as all non-speech; retry without it.
    for use_vad in [true, false] {
    let _ = tokio::fs::remove_file(&subtitle_path).await;
    let mut transcribe = tools::command(&whisper);
    transcribe
        .args(["-m"])
        .arg(&model)
        .args(["-f"])
        .arg(&audio_path)
        .args(["-l", language, "-osrt", "-of"])
        .arg(&output_base);
    if let Err(error) = add_accuracy_args(&mut transcribe, &work_dir, use_vad).await {
        let _ = tokio::fs::remove_dir_all(&work_dir).await;
        return Err(error);
    }
    let transcription_result = match run_cancelable(&mut transcribe, cancelled).await {
        Ok(result) => result,
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&work_dir).await;
            let _ = tokio::fs::remove_file(output_base.with_extension("srt")).await;
            return Err(error);
        }
    };
    match transcription_result {
        ProcessResult::Cancelled => {
            let _ = tokio::fs::remove_dir_all(&work_dir).await;
            return Ok(SubtitleResult::Cancelled);
        }
        ProcessResult::Completed(status) if !status.success() => {
            let _ = tokio::fs::remove_dir_all(&work_dir).await;
            let _ = tokio::fs::remove_file(output_base.with_extension("srt")).await;
            return Err("Local transcription failed. Check the model and try again.".into());
        }
        ProcessResult::Completed(_) => {}
    }
    let metadata = match tokio::fs::metadata(&subtitle_path).await {
        Ok(metadata) => metadata,
        Err(_) => {
            let _ = tokio::fs::remove_file(&subtitle_path).await;
            let _ = tokio::fs::remove_dir_all(&work_dir).await;
            return Err("whisper-cli did not produce an SRT file.".into());
        }
    };
    if metadata.len() == 0 {
        if use_vad {
            continue;
        }
        let _ = tokio::fs::remove_file(&subtitle_path).await;
        let _ = tokio::fs::remove_dir_all(&work_dir).await;
        return Err("whisper-cli produced an empty SRT file.".into());
    }
    break;
    }
    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    Ok(SubtitleResult::Created(subtitle_path))
}

fn parse_ffmpeg_time(line: &str) -> Option<f64> {
    if let Some(value) = line.strip_prefix("out_time_ms=") {
        return value
            .trim()
            .parse::<f64>()
            .ok()
            .map(|microseconds| microseconds / 1_000_000.0);
    }
    let marker = "time=";
    let start = line.find(marker)? + marker.len();
    let value = line[start..].split_whitespace().next()?;
    let mut parts = value.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn emit_progress(
    app: &AppHandle,
    job_id: &str,
    stage: &str,
    percent: f64,
    message: &str,
    output: Option<&Path>,
) {
    let _ = app.emit(
        "export-progress",
        ProgressEvent {
            job_id: job_id.into(),
            stage: stage.into(),
            percent,
            message: message.into(),
            output_path: output.map(|path| path.to_string_lossy().into_owned()),
        },
    );
}

fn validate_audio_normalization(settings: &AudioNormalizationSettings) -> Result<(), String> {
    if !(-24.0..=-5.0).contains(&settings.target_lufs) {
        return Err("Target loudness must be between -24 and -5 LUFS.".into());
    }
    if !(1.0..=20.0).contains(&settings.loudness_range) {
        return Err("Loudness range must be between 1 and 20 LU.".into());
    }
    if !(-9.0..=0.0).contains(&settings.true_peak_db) {
        return Err("True peak must be between -9 and 0 dB.".into());
    }
    Ok(())
}

fn parse_loudness_measurements(output: &str) -> Result<LoudnessMeasurements, String> {
    let start = output
        .rfind('{')
        .ok_or_else(|| "FFmpeg did not return loudness measurements.".to_string())?;
    let end = output
        .rfind('}')
        .filter(|end| *end > start)
        .ok_or_else(|| "FFmpeg returned incomplete loudness measurements.".to_string())?;
    let json: serde_json::Value = serde_json::from_str(&output[start..=end])
        .map_err(|_| "FFmpeg returned invalid loudness measurements.".to_string())?;
    let number = |key: &str| -> Result<f64, String> {
        let value = json[key]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("FFmpeg did not return a valid {key} measurement."))?;
        Ok(value)
    };
    Ok(LoudnessMeasurements {
        input_i: number("input_i")?,
        input_lra: number("input_lra")?,
        input_tp: number("input_tp")?,
        input_thresh: number("input_thresh")?,
        target_offset: number("target_offset")?,
    })
}

enum LoudnessAnalysisResult {
    Complete(LoudnessMeasurements),
    Cancelled,
}

async fn analyze_loudness(
    app: &AppHandle,
    input: &Path,
    settings: &AudioNormalizationSettings,
    noise_filter: Option<&str>,
    keep_ranges: &[TimeRange],
    cancelled: &AtomicBool,
    job_id: &str,
) -> Result<LoudnessAnalysisResult, String> {
    validate_audio_normalization(settings)?;
    let ffmpeg = tools::resolve(app, Tool::Ffmpeg)
        .await
        .ok_or_else(|| "FFmpeg is not bundled and was not found on PATH.".to_string())?;
    emit_progress(
        app,
        job_id,
        "analyzingAudio",
        8.0,
        "Analyzing audio loudness",
        None,
    );
    let loudnorm = format!(
        "{}aformat=sample_rates=48000:channel_layouts=stereo,loudnorm=I={:.2}:LRA={:.2}:TP={:.2}:print_format=json",
        noise_filter.map(|filter| format!("{filter},")).unwrap_or_default(),
        settings.target_lufs, settings.loudness_range, settings.true_peak_db
    );
    let mut command = tools::command(&ffmpeg);
    command.args(["-hide_banner", "-nostats", "-i"]).arg(input);
    if keep_ranges.is_empty() {
        command.args(["-vn", "-af", &loudnorm]);
    } else {
        let mut graph = Vec::with_capacity(keep_ranges.len() + 2);
        let mut inputs = String::new();
        for (index, range) in keep_ranges.iter().enumerate() {
            graph.push(format!(
                "[0:a]atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS[a{index}]",
                range.start, range.end
            ));
            inputs.push_str(&format!("[a{index}]"));
        }
        graph.push(format!(
            "{inputs}concat=n={}:v=0:a=1,{SPEECH_AUDIO_FILTER}[cut_audio]",
            keep_ranges.len()
        ));
        graph.push(format!("[cut_audio]{loudnorm}[analysis_audio]"));
        command.args([
            "-filter_complex",
            &graph.join(";"),
            "-map",
            "[analysis_audio]",
        ]);
    }
    command
        .args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| friendly_error(&error.to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| friendly_error("FFmpeg loudness analysis output is unavailable"))?;
    let mut lines = BufReader::new(stderr).lines();
    let mut output = String::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            return Ok(LoudnessAnalysisResult::Cancelled);
        }
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(text)) => {
                    if output.len() < 1_000_000 {
                        output.push_str(&text);
                        output.push('\n');
                    }
                }
                Ok(None) => break,
                Err(error) => return Err(friendly_error(&error.to_string())),
            },
            _ = sleep(Duration::from_millis(150)) => {}
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|error| friendly_error(&error.to_string()))?;
    if !status.success() {
        return Err("Audio loudness analysis failed. Try exporting without normalization.".into());
    }
    Ok(LoudnessAnalysisResult::Complete(
        parse_loudness_measurements(&output)?,
    ))
}

/// Everything the final encode does besides scaling and subtitles.
#[derive(Clone, Copy, Default)]
struct EditPlan<'a> {
    keep_ranges: &'a [TimeRange],
    loudness: Option<&'a LoudnessMeasurements>,
    noise_filter: Option<&'a str>,
    reframe: Option<&'a reframe::ReframePlan>,
    zoom_filter: Option<&'a str>,
}

fn build_video_args(
    settings: &ExportSettings,
    duration: f64,
    subtitle_filename: Option<&str>,
    plan: &EditPlan,
) -> Result<Vec<String>, String> {
    let EditPlan {
        keep_ranges,
        loudness,
        noise_filter,
        reframe,
        zoom_filter,
    } = *plan;
    let mut args = Vec::new();
    let mut video_filters = Vec::new();
    if reframe.is_some() && matches!(settings.preset.as_str(), "youtube" | "x") {
        return Err(
            "Vertical conversion needs a portrait destination such as Shorts, TikTok, or Instagram."
                .into(),
        );
    }
    let normalization_filter = if settings.audio_normalization.enabled {
        validate_audio_normalization(&settings.audio_normalization)?;
        let measured = loudness.ok_or_else(|| {
            "Audio loudness measurements are required before normalization.".to_string()
        })?;
        Some(format!(
            "aformat=sample_rates=48000:channel_layouts=stereo,loudnorm=I={:.2}:LRA={:.2}:TP={:.2}:measured_I={:.2}:measured_LRA={:.2}:measured_TP={:.2}:measured_thresh={:.2}:offset={:.2}:linear=true,aresample=48000,aformat=sample_rates=48000:channel_layouts=stereo",
            settings.audio_normalization.target_lufs,
            settings.audio_normalization.loudness_range,
            settings.audio_normalization.true_peak_db,
            measured.input_i,
            measured.input_lra,
            measured.input_tp,
            measured.input_thresh,
            measured.target_offset,
        ))
    } else {
        None
    };
    let audio_filter = match (noise_filter, normalization_filter) {
        (Some(noise), Some(normalization)) => Some(format!("{noise},{normalization}")),
        (Some(noise), None) => Some(noise.to_string()),
        (None, normalization) => normalization,
    };
    let resize_filter = match settings.preset.as_str() {
        "youtube" => Some("scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2".to_string()),
        "shorts" | "tiktok" => Some("scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920".to_string()),
        "x" => Some("scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2".to_string()),
        "instagram" => Some("scale=1080:1350:force_original_aspect_ratio=increase,crop=1080:1350".to_string()),
        "custom" => match (settings.width, settings.height) {
            (Some(width), Some(height)) if (240..=7680).contains(&width) && (240..=4320).contains(&height) => Some(format!("scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2")),
            _ => return Err("Custom dimensions must be 240–7680 pixels wide and 240–4320 pixels high.".into()),
        },
        "original" | "discord" => None,
        _ => return Err("Unknown export preset.".into()),
    };
    if let Some(filter) = resize_filter {
        video_filters.push(filter);
    }
    if let Some(filename) = subtitle_filename {
        if !filename
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
        {
            return Err("The temporary subtitle filename is invalid.".into());
        }
        video_filters.push(format!("subtitles=filename={filename}"));
    }
    // Reframing and zooming follow the source timeline, so they run before any trimming.
    let mut source_filters = Vec::new();
    if let Some(reframe) = reframe {
        source_filters.push(reframe.filter());
    }
    if let Some(zoom) = zoom_filter {
        source_filters.push(zoom.to_string());
    }
    if keep_ranges.is_empty() {
        video_filters.splice(0..0, source_filters);
        if !video_filters.is_empty() {
            args.extend(["-vf".into(), video_filters.join(",")]);
        }
        if let Some(filter) = audio_filter {
            args.extend(["-af".into(), filter]);
        }
    } else {
        let mut graph = Vec::with_capacity(keep_ranges.len() * 2 + 3);
        let mut concat_inputs = String::new();
        let prefiltered = !source_filters.is_empty();
        if prefiltered {
            let outputs: String = (0..keep_ranges.len()).map(|index| format!("[r{index}]")).collect();
            graph.push(format!("[0:v]{},split={}{outputs}", source_filters.join(","), keep_ranges.len()));
        }
        for (index, range) in keep_ranges.iter().enumerate() {
            let source = if prefiltered {
                format!("[r{index}]")
            } else {
                "[0:v]".to_string()
            };
            graph.push(format!(
                "{source}trim=start={:.6}:end={:.6},setpts=PTS-STARTPTS[v{index}]",
                range.start, range.end
            ));
            graph.push(format!(
                "[0:a]atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS[a{index}]",
                range.start, range.end
            ));
            concat_inputs.push_str(&format!("[v{index}][a{index}]"));
        }
        graph.push(format!(
            "{concat_inputs}concat=n={}:v=1:a=1[cut_video][cut_audio]",
            keep_ranges.len()
        ));
        let video_output = if video_filters.is_empty() {
            "[cut_video]"
        } else {
            graph.push(format!(
                "[cut_video]{}[final_video]",
                video_filters.join(",")
            ));
            "[final_video]"
        };
        let audio_output = if let Some(filter) = audio_filter {
            graph.push(format!("[cut_audio]{filter}[final_audio]"));
            "[final_audio]"
        } else {
            "[cut_audio]"
        };
        args.extend([
            "-filter_complex".into(),
            graph.join(";"),
            "-map".into(),
            video_output.into(),
            "-map".into(),
            audio_output.into(),
        ]);
    }
    if let Some(fps) = settings.fps.filter(|value| (1.0..=120.0).contains(value)) {
        args.extend(["-r".into(), fps.to_string()]);
    }
    let codec = match settings.codec.as_str() {
        "h265" => "libx265",
        "h264" => "libx264",
        _ => return Err("Unknown video codec.".into()),
    };
    args.extend([
        "-c:v".into(),
        codec.into(),
        "-preset".into(),
        "medium".into(),
    ]);
    if settings.preset == "discord" {
        let size_mb = settings.max_size_mb.unwrap_or(25.0).clamp(1.0, 10_000.0);
        let bitrate =
            (((size_mb * 8192.0 * 0.90) / duration.max(1.0)) - 128.0).clamp(250.0, 50_000.0) as u32;
        args.extend([
            "-b:v".into(),
            format!("{bitrate}k"),
            "-maxrate".into(),
            format!("{}k", bitrate * 2),
            "-bufsize".into(),
            format!("{}k", bitrate * 4),
        ]);
    } else if let Some(bitrate) = settings
        .bitrate_kbps
        .filter(|value| (100..=100_000).contains(value))
    {
        args.extend(["-b:v".into(), format!("{bitrate}k")]);
    } else {
        let crf = match settings.quality.as_str() {
            "small" => "28",
            "high" => "18",
            _ => "23",
        };
        args.extend(["-crf".into(), crf.into()]);
    }
    args.extend([
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-movflags".into(),
        "+faststart".into(),
    ]);
    Ok(args)
}

#[tauri::command]
async fn start_export(
    app: AppHandle,
    jobs: State<'_, JobManager>,
    job_id: String,
    input_path: String,
    settings: ExportSettings,
) -> Result<String, String> {
    validate_job_id(&job_id)?;
    let input = validate_input(&input_path)?;
    let cancelled = register_job(&jobs, &job_id)?;
    let result = run_export(&app, &jobs, &job_id, input, settings, &cancelled).await;
    jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
    result
}

async fn run_export(
    app: &AppHandle,
    jobs: &JobManager,
    job_id: &str,
    input: PathBuf,
    settings: ExportSettings,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    let app = app.clone();
    let job_id = job_id.to_string();
    let info = probe_video(app.clone(), input.to_string_lossy().into_owned()).await?;
    let output = unique_output(&input);
    let temp = output.with_file_name(format!(
        ".{}.dropcut-{}.mp4",
        output
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("output"),
        job_id
    ));
    let cancelled = cancelled.clone();

    if (settings.silence_removal.enabled
        || settings.audio_normalization.enabled
        || settings.noise_reduction.enabled
        || !settings.cut_ranges.is_empty())
        && !info.has_audio
    {
        return Err("The selected audio processing options require an audio track.".into());
    }
    let noise_filter = noise_reduction_filter(&settings.noise_reduction)?;
    let keep_ranges = if settings.silence_removal.enabled {
        match detect_silence(
            &app,
            &input,
            info.duration_seconds,
            &settings.silence_removal,
            &cancelled,
            &job_id,
        )
        .await
        {
            Ok(SilenceDetectionResult::Complete(ranges)) => ranges,
            Ok(SilenceDetectionResult::Cancelled) => {
                emit_progress(
                    &app,
                    &job_id,
                    "cancelled",
                    3.0,
                    "Processing cancelled",
                    None,
                );
                jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
                return Ok(String::new());
            }
            Err(error) => {
                jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
                return Err(error);
            }
        }
    } else {
        Vec::new()
    };
    let keep_ranges = {
        let cuts = transcript::normalize_cut_ranges(&settings.cut_ranges, info.duration_seconds)?;
        let ranges = transcript::subtract_cuts(&keep_ranges, &cuts, info.duration_seconds)?;
        if ranges.len() > MAX_KEEP_RANGES {
            return Err(
                "Too many sections were cut. Try removing fewer words or using a weaker silence setting."
                    .into(),
            );
        }
        ranges
    };
    let mut reframe_plan = None;
    let mut _reframe_file = None;
    if settings.reframe.enabled {
        if let Some((crop_width, crop_height)) = reframe::portrait_crop_size(info.width, info.height)
        {
            let track = match reframe::track_faces(
                &app,
                &input,
                info.width,
                info.height,
                info.duration_seconds,
                &cancelled,
                &job_id,
            )
            .await?
            {
                reframe::TrackResult::Complete(track) => track,
                reframe::TrackResult::Cancelled => {
                    emit_progress(&app, &job_id, "cancelled", 4.0, "Processing cancelled", None);
                    return Ok(String::new());
                }
            };
            let source_width = info.width as u32;
            let (script, initial_x) = match &track {
                Some(track) => reframe::build_crop_commands(
                    track,
                    info.duration_seconds,
                    source_width,
                    crop_width,
                ),
                None => (String::new(), (source_width - crop_width) / 2 & !1),
            };
            let command_file = if script.is_empty() {
                None
            } else {
                let name = format!(".dropcut-{job_id}-reframe.cmd");
                let path = output.with_file_name(&name);
                tokio::fs::write(&path, script)
                    .await
                    .map_err(|error| friendly_error(&error.to_string()))?;
                _reframe_file = Some(TempFile(path));
                Some(name)
            };
            reframe_plan = Some(reframe::ReframePlan {
                crop_width,
                crop_height,
                initial_x,
                command_file,
            });
        }
    }
    let output_duration = if keep_ranges.is_empty() {
        info.duration_seconds
    } else {
        keep_ranges
            .iter()
            .map(|range| range.end - range.start)
            .sum()
    };
    let loudness = if settings.audio_normalization.enabled {
        match analyze_loudness(
            &app,
            &input,
            &settings.audio_normalization,
            noise_filter.as_deref(),
            &keep_ranges,
            &cancelled,
            &job_id,
        )
        .await
        {
            Ok(LoudnessAnalysisResult::Complete(measurements)) => Some(measurements),
            Ok(LoudnessAnalysisResult::Cancelled) => {
                emit_progress(
                    &app,
                    &job_id,
                    "cancelled",
                    8.0,
                    "Processing cancelled",
                    None,
                );
                jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
                return Ok(String::new());
            }
            Err(error) => {
                jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
                return Err(error);
            }
        }
    } else {
        None
    };
    let frame_size = reframe_plan
        .as_ref()
        .map(|plan| (plan.crop_width, plan.crop_height))
        .unwrap_or((info.width as u32, info.height as u32));
    let zoom_filter = auto_zoom_filter(&settings.auto_zoom, frame_size.0, frame_size.1, info.fps)?;
    let edit_plan = EditPlan {
        keep_ranges: &keep_ranges,
        loudness: loudness.as_ref(),
        noise_filter: noise_filter.as_deref(),
        reframe: reframe_plan.as_ref(),
        zoom_filter: zoom_filter.as_deref(),
    };
    let mut video_args = match build_video_args(&settings, output_duration, None, &edit_plan) {
        Ok(args) => args,
        Err(error) => {
            jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
            return Err(error);
        }
    };

    let subtitle_temp = if settings.subtitles.enabled {
        match create_subtitles(
            &app,
            &input,
            &output,
            &settings.subtitles,
            &keep_ranges,
            &cancelled,
            &job_id,
        )
        .await
        {
            Ok(SubtitleResult::Created(path)) => Some(path),
            Ok(SubtitleResult::Cancelled) => {
                emit_progress(
                    &app,
                    &job_id,
                    "cancelled",
                    15.0,
                    "Processing cancelled",
                    None,
                );
                jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
                return Ok(String::new());
            }
            Err(error) => {
                emit_progress(
                    &app,
                    &job_id,
                    "error",
                    15.0,
                    "Subtitle generation failed",
                    None,
                );
                jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
                return Err(error);
            }
        }
    } else {
        None
    };
    // Styled subtitles are laid out for the real output size and saved as ASS.
    let _srt_guard = subtitle_temp.clone().map(TempFile);
    let ass_temp = match &subtitle_temp {
        Some(srt_path) => {
            let content = tokio::fs::read(srt_path)
                .await
                .map_err(|error| friendly_error(&error.to_string()))?;
            let cues = subtitles::parse_srt(&String::from_utf8_lossy(&content));
            let (width, height) = output_dimensions(
                &settings,
                (info.width, info.height),
                reframe_plan
                    .as_ref()
                    .map(|plan| (plan.crop_width, plan.crop_height)),
            );
            let ass = subtitles::build_ass(&cues, &settings.subtitles.style, width, height)?;
            let name = format!(".dropcut-{job_id}-subtitles.ass");
            let path = output.with_file_name(&name);
            tokio::fs::write(&path, ass)
                .await
                .map_err(|error| friendly_error(&error.to_string()))?;
            Some((TempFile(path), name))
        }
        None => None,
    };
    if settings.subtitles.enabled && settings.subtitles.burn_in {
        if !supports_subtitle_burn_in(&app).await {
            if let Some(path) = &subtitle_temp {
                let _ = tokio::fs::remove_file(path).await;
            }
            jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
            return Err(
                "This FFmpeg build does not include the subtitles filter required for burn-in. Export an SRT file instead."
                    .into(),
            );
        }
        let subtitle_filename = ass_temp
            .as_ref()
            .map(|(_, name)| name.as_str())
            .ok_or_else(|| "The subtitle file could not be prepared for burn-in.".to_string())?;
        video_args = match build_video_args(
            &settings,
            output_duration,
            Some(subtitle_filename),
            &edit_plan,
        ) {
            Ok(args) => args,
            Err(error) => {
                if let Some(path) = &subtitle_temp {
                    let _ = tokio::fs::remove_file(path).await;
                }
                jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
                return Err(error);
            }
        };
    }

    let preprocessing_percent = if subtitle_temp.is_some() {
        60.0
    } else if settings.silence_removal.enabled {
        12.0
    } else if settings.audio_normalization.enabled {
        10.0
    } else {
        2.0
    };
    emit_progress(
        &app,
        &job_id,
        "preparing",
        preprocessing_percent,
        "Preparing export",
        None,
    );
    let ffmpeg = tools::resolve(&app, Tool::Ffmpeg)
        .await
        .ok_or_else(|| "FFmpeg is not bundled and was not found on PATH.".to_string())?;
    let mut command = tools::command(&ffmpeg);
    command
        .args([
            "-hide_banner",
            "-y",
            "-nostats",
            "-progress",
            "pipe:2",
            "-i",
        ])
        .arg(&input);
    command.current_dir(output.parent().unwrap_or_else(|| Path::new(".")));
    command.args(video_args);
    command
        .arg(&temp)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
            return Err(friendly_error(&error.to_string()));
        }
    };
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| friendly_error("FFmpeg stderr unavailable"))?;
    let mut lines = BufReader::new(stderr).lines();
    let mut last_percent = preprocessing_percent;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            let _ = tokio::fs::remove_file(&temp).await;
            if let Some(path) = &subtitle_temp {
                let _ = tokio::fs::remove_file(path).await;
            }
            emit_progress(
                &app,
                &job_id,
                "cancelled",
                last_percent,
                "Processing cancelled",
                None,
            );
            jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
            return Ok(String::new());
        }
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(text)) => if let Some(elapsed) = parse_ffmpeg_time(&text) {
                    let start = preprocessing_percent + 1.0;
                    let percent = (elapsed / output_duration.max(1.0) * (97.0 - start) + start).clamp(last_percent, 97.0);
                    if percent - last_percent >= 0.3 { last_percent = percent; emit_progress(&app, &job_id, "encoding", percent, "Encoding video", None); }
                },
                Ok(None) => break,
                Err(error) => { eprintln!("[DropCut] Failed to read FFmpeg output: {error}"); break; }
            },
            _ = sleep(Duration::from_millis(150)) => {}
        }
    }
    let status = match child.wait().await {
        Ok(status) => status,
        Err(error) => {
            let _ = tokio::fs::remove_file(&temp).await;
            if let Some(path) = &subtitle_temp {
                let _ = tokio::fs::remove_file(path).await;
            }
            jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
            return Err(friendly_error(&error.to_string()));
        }
    };
    jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
    if !status.success() {
        let _ = tokio::fs::remove_file(&temp).await;
        if let Some(path) = &subtitle_temp {
            let _ = tokio::fs::remove_file(path).await;
        }
        emit_progress(
            &app,
            &job_id,
            "error",
            last_percent,
            "Video export failed",
            None,
        );
        return Err("Video export failed. Change the settings or try another video.".into());
    }
    emit_progress(&app, &job_id, "verifying", 98.0, "Verifying output", None);
    let temp_meta = tokio::fs::metadata(&temp)
        .await
        .map_err(|error| friendly_error(&error.to_string()))?;
    if temp_meta.len() == 0 {
        let _ = tokio::fs::remove_file(&temp).await;
        if let Some(path) = &subtitle_temp {
            let _ = tokio::fs::remove_file(path).await;
        }
        return Err("The exported video could not be verified.".into());
    }
    if settings.preset == "discord" {
        let limit_bytes = (settings.max_size_mb.unwrap_or(25.0) * 1024.0 * 1024.0) as u64;
        if temp_meta.len() > limit_bytes {
            let _ = tokio::fs::remove_file(&temp).await;
            if let Some(path) = &subtitle_temp {
                let _ = tokio::fs::remove_file(path).await;
            }
            emit_progress(
                &app,
                &job_id,
                "error",
                98.0,
                "The export exceeded the size limit and was not saved",
                None,
            );
            return Err(
                "The export exceeded the requested size. Choose a larger limit and try again."
                    .into(),
            );
        }
    }
    let final_subtitle = output.with_extension("srt");
    if let Some(path) = &subtitle_temp {
        tokio::fs::rename(path, &final_subtitle)
            .await
            .map_err(|error| friendly_error(&error.to_string()))?;
    }
    let final_styled = output.with_extension("ass");
    if let Some((guard, _)) = &ass_temp {
        if let Err(error) = tokio::fs::rename(&guard.0, &final_styled).await {
            if let Some(path) = &subtitle_temp {
                let _ = tokio::fs::rename(&final_subtitle, path).await;
            }
            return Err(friendly_error(&error.to_string()));
        }
    }
    if let Err(error) = tokio::fs::rename(&temp, &output).await {
        if let Some(path) = &subtitle_temp {
            let _ = tokio::fs::rename(&final_subtitle, path).await;
        }
        let _ = tokio::fs::remove_file(&final_styled).await;
        return Err(friendly_error(&error.to_string()));
    }
    emit_progress(
        &app,
        &job_id,
        "done",
        100.0,
        if subtitle_temp.is_some() {
            "Video and subtitles ready"
        } else {
            "Video ready"
        },
        Some(&output),
    );
    Ok(output.to_string_lossy().into_owned())
}

/// A gentle, repeating push-in: the zoom eases up and back to 100% every
/// `period` seconds, and every other pulse is 50% stronger (for example
/// 100% → 108% → 100% → 112%). It runs on the source timeline, so it stays
/// steady across cuts.
fn auto_zoom_filter(
    settings: &AutoZoomSettings,
    width: u32,
    height: u32,
    fps: f64,
) -> Result<Option<String>, String> {
    let (amplitude, period) = match settings.template.as_str() {
        "" | "none" => return Ok(None),
        "natural" => (0.03, 9.0),
        "youtube" => (0.05, 7.0),
        "shorts" => (0.08, 3.5),
        _ => return Err("Unknown auto zoom style.".into()),
    };
    let fps = if fps.is_finite() && (1.0..=120.0).contains(&fps) {
        fps
    } else {
        30.0
    };
    Ok(Some(format!(
        "zoompan=z='1+{amplitude}*(0.5-0.5*cos(2*PI*mod(it\\,{period})/{period}))*(1+0.5*mod(floor(it/{period})\\,2))':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s={}x{}:fps={}/1000",
        width & !1,
        height & !1,
        (fps * 1000.0).round() as u64
    )))
}

/// Size of the exported frame, which subtitles are laid out for.
fn output_dimensions(
    settings: &ExportSettings,
    source: (u64, u64),
    reframed: Option<(u32, u32)>,
) -> (u32, u32) {
    match settings.preset.as_str() {
        "youtube" => (1920, 1080),
        "x" => (1280, 720),
        "shorts" | "tiktok" => (1080, 1920),
        "instagram" => (1080, 1350),
        "custom" => (
            settings.width.unwrap_or(1920),
            settings.height.unwrap_or(1080),
        ),
        _ => reframed.unwrap_or((source.0 as u32, source.1 as u32)),
    }
}

fn noise_reduction_filter(settings: &NoiseReductionSettings) -> Result<Option<String>, String> {
    if !settings.enabled {
        return Ok(None);
    }
    let filter = match settings.strength.as_str() {
        "weak" => "afftdn=nr=8:nf=-55:tn=1",
        "normal" => "highpass=f=70,afftdn=nr=14:nf=-50:tn=1",
        "strong" => "highpass=f=80,afftdn=nr=24:nf=-45:tn=1",
        _ => return Err("Unknown noise reduction strength.".into()),
    };
    Ok(Some(filter.to_string()))
}

/// Hints that make whisper keep hesitations instead of silently cleaning them up.
fn filler_prompt(language: &str) -> Option<&'static str> {
    match language {
        "ja" => Some("えーと、あのー、まあ、うーん、そうですね。えっと、その、"),
        "en" => Some("Um, uh, so, you know, I mean, like, well,"),
        _ => None,
    }
}

async fn transcribe_words(
    app: &AppHandle,
    input: &Path,
    model: &Path,
    language: &str,
    cancelled: &AtomicBool,
    job_id: &str,
) -> Result<Option<Vec<transcript::TranscriptWord>>, String> {
    let whisper = find_whisper_cli(app)
        .await
        .ok_or_else(|| "whisper-cli is not bundled and was not found on PATH.".to_string())?;
    let ffmpeg = tools::resolve(app, Tool::Ffmpeg)
        .await
        .ok_or_else(|| "FFmpeg is not bundled and was not found on PATH.".to_string())?;
    let work_dir = std::env::temp_dir().join("dropcut").join(job_id);
    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(|error| friendly_error(&error.to_string()))?;
    let result = async {
        let audio_path = work_dir.join("audio.wav");
        emit_progress(app, job_id, "extractingAudio", 10.0, "Extracting audio", None);
        match extract_audio_wav(&ffmpeg, input, &[], &audio_path, cancelled).await? {
            ProcessResult::Cancelled => return Ok(None),
            ProcessResult::Completed(status) if !status.success() => {
                return Err("Audio extraction for transcription failed.".to_string());
            }
            ProcessResult::Completed(_) => {}
        }
        emit_progress(
            app,
            job_id,
            "transcribing",
            30.0,
            "Transcribing locally with whisper.cpp",
            None,
        );
        let output_base = work_dir.join("transcript");
        let mut transcribe = tools::command(&whisper);
        transcribe
            .args(["-m"])
            .arg(model)
            .args(["-f"])
            .arg(&audio_path)
            .args(["-l", language, "-ojf", "-of"])
            .arg(&output_base);
        if let Some(prompt) = filler_prompt(language) {
            transcribe.args(["--prompt", prompt]);
        }
        add_accuracy_args(&mut transcribe, &work_dir, true).await?;
        match run_cancelable(&mut transcribe, cancelled).await? {
            ProcessResult::Cancelled => return Ok(None),
            ProcessResult::Completed(status) if !status.success() => {
                return Err("Local transcription failed. Check the model and try again.".to_string());
            }
            ProcessResult::Completed(_) => {}
        }
        let raw = tokio::fs::read(output_base.with_extension("json"))
            .await
            .map_err(|_| "whisper-cli did not produce a transcript.".to_string())?;
        transcript::parse_whisper_json(&raw).map(Some)
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    result
}

/// Transcribes the original audio so the user can review filler words and
/// remove sentences. Returns `None` when cancelled.
#[tauri::command]
async fn analyze_transcript(
    app: AppHandle,
    jobs: State<'_, JobManager>,
    job_id: String,
    input_path: String,
    model_path: String,
    language: String,
) -> Result<Option<Vec<transcript::TranscriptWord>>, String> {
    validate_job_id(&job_id)?;
    let input = validate_input(&input_path)?;
    let model = validated_model(Some(&model_path))?;
    let language = validated_language(&language)?.to_string();
    let cancelled = register_job(&jobs, &job_id)?;
    let result = transcribe_words(&app, &input, &model, &language, &cancelled, &job_id).await;
    jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
    match result {
        Ok(Some(words)) => {
            emit_progress(&app, &job_id, "done", 100.0, "Transcript ready", None);
            Ok(Some(words))
        }
        Ok(None) => {
            emit_progress(&app, &job_id, "cancelled", 30.0, "Processing cancelled", None);
            Ok(None)
        }
        Err(error) => {
            emit_progress(&app, &job_id, "error", 30.0, "Transcription failed", None);
            Err(error)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HighlightResult {
    words: Vec<transcript::TranscriptWord>,
    highlights: Vec<highlight::Highlight>,
    ai_used: bool,
}

/// Runs a child process, collects its stdout, and kills it on cancel.
/// Returns `None` when cancelled.
async fn capture_stdout(command: &mut Command, cancelled: &AtomicBool) -> Result<Option<(std::process::ExitStatus, Vec<u8>)>, String> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    hide_console(command);
    let mut child = command.spawn().map_err(|error| friendly_error(&error.to_string()))?;
    let mut stdout = child.stdout.take().ok_or("Could not read the tool output.")?;
    let mut data = Vec::new();
    let read = stdout.read_to_end(&mut data);
    tokio::pin!(read);
    loop {
        tokio::select! {
            result = &mut read => {
                result.map_err(|error| friendly_error(&error.to_string()))?;
                break;
            }
            _ = sleep(Duration::from_millis(200)) => {
                if cancelled.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    return Ok(None);
                }
            }
        }
    }
    let status = child.wait().await.map_err(|error| friendly_error(&error.to_string()))?;
    Ok(Some((status, data)))
}

/// Decodes the whole audio track to 8 kHz mono PCM and returns per-second loudness.
async fn measure_levels(ffmpeg: &Path, input: &Path, cancelled: &AtomicBool) -> Result<Option<Vec<f64>>, String> {
    let mut command = tools::command(ffmpeg);
    command
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", &highlight::LEVEL_SAMPLE_RATE.to_string(), "-f", "s16le", "-"]);
    let Some((status, pcm)) = capture_stdout(&mut command, cancelled).await? else {
        return Ok(None);
    };
    if !status.success() || pcm.is_empty() {
        return Err("This video has no audio to analyze for highlights.".to_string());
    }
    Ok(Some(highlight::audio_levels(&pcm, highlight::LEVEL_SAMPLE_RATE)))
}

/// Asks the bundled local model to rate each candidate and re-orders them.
/// Any failure for one clip just leaves that clip on its signal-based score.
async fn rerank_with_llm(
    app: &AppHandle,
    words: &[transcript::TranscriptWord],
    mut candidates: Vec<highlight::Highlight>,
    count: usize,
    cancelled: &AtomicBool,
    job_id: &str,
) -> Result<Option<Vec<highlight::Highlight>>, String> {
    let (Some(llama), Some(model)) = (tools::resolve(app, Tool::Llama).await, tools::bundled_llm_model(app)) else {
        candidates.truncate(count);
        return Ok(Some(candidates));
    };
    let best = candidates.iter().map(|c| c.score).fold(f64::MIN, f64::max);
    let worst = candidates.iter().map(|c| c.score).fold(f64::MAX, f64::min);
    let span = (best - worst).max(1e-9);
    let total = candidates.len();
    let mut ranked: Vec<(f64, highlight::Highlight)> = Vec::with_capacity(total);
    for (index, mut clip) in candidates.drain(..).enumerate() {
        let percent = 80.0 + 18.0 * index as f64 / total as f64;
        emit_progress(app, job_id, "analyzing", percent, "Judging clips with the local AI model", None);
        let heuristic = (clip.score - worst) / span;
        let excerpt = llm::excerpt(words, clip.start, clip.end);
        let mut blended = heuristic * 0.4;
        if !excerpt.trim().is_empty() {
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
            let mut command = tools::command(&llama);
            command
                .arg("-m")
                .arg(&model)
                .args(["-p", &llm::build_prompt(&excerpt), "-n", "48", "--temp", "0", "-no-cnv", "--no-display-prompt", "--no-warmup", "-c", "4096", "-t", &threads.to_string()]);
            let Some((status, output)) = capture_stdout(&mut command, cancelled).await? else {
                return Ok(None);
            };
            if status.success() {
                if let Some(judgement) = llm::parse_answer(&String::from_utf8_lossy(&output)) {
                    blended = llm::blend(heuristic, judgement.score);
                    clip.title = judgement.title;
                }
            }
        }
        ranked.push((blended, clip));
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    ranked.truncate(count);
    Ok(Some(ranked.into_iter().map(|(_, clip)| clip).collect()))
}

#[tauri::command]
async fn check_llm(app: AppHandle) -> bool {
    tools::resolve(&app, Tool::Llama).await.is_some() && tools::bundled_llm_model(&app).is_some()
}

/// Transcribes the video, then scores it for the most engaging sections.
/// Runs entirely on this machine. Returns `None` when cancelled.
#[tauri::command]
async fn find_highlights(
    app: AppHandle,
    jobs: State<'_, JobManager>,
    job_id: String,
    input_path: String,
    model_path: String,
    language: String,
    target_seconds: f64,
    count: usize,
    use_llm: bool,
) -> Result<Option<HighlightResult>, String> {
    validate_job_id(&job_id)?;
    let input = validate_input(&input_path)?;
    let model = validated_model(Some(&model_path))?;
    let language = validated_language(&language)?.to_string();
    if !(10.0..=180.0).contains(&target_seconds) || !(1..=10).contains(&count) {
        return Err("Highlight length must be 10-180 seconds and 1-10 clips.".to_string());
    }
    let cancelled = register_job(&jobs, &job_id)?;
    let result = async {
        let Some(words) = transcribe_words(&app, &input, &model, &language, &cancelled, &job_id).await? else {
            return Ok(None);
        };
        emit_progress(&app, &job_id, "analyzing", 80.0, "Finding highlights", None);
        let ffmpeg = tools::resolve(&app, Tool::Ffmpeg)
            .await
            .ok_or_else(|| "FFmpeg is not bundled and was not found on PATH.".to_string())?;
        let Some(levels) = measure_levels(&ffmpeg, &input, &cancelled).await? else {
            return Ok(None);
        };
        let wanted = if use_llm { (count * 2).min(10) } else { count };
        let mut highlights = highlight::find_highlights(&words, &levels, levels.len() as f64, target_seconds, wanted);
        let mut ai_used = false;
        if use_llm && !highlights.is_empty() {
            let Some(reranked) = rerank_with_llm(&app, &words, highlights, count, &cancelled, &job_id).await? else {
                return Ok(None);
            };
            ai_used = reranked.iter().any(|clip| clip.title.is_some());
            highlights = reranked;
        }
        Ok(Some(HighlightResult { words, highlights, ai_used }))
    }
    .await;
    jobs.0.lock().ok().map(|mut map| map.remove(&job_id));
    match result {
        Ok(Some(found)) => {
            emit_progress(&app, &job_id, "done", 100.0, "Highlights ready", None);
            Ok(Some(found))
        }
        Ok(None) => {
            emit_progress(&app, &job_id, "cancelled", 30.0, "Processing cancelled", None);
            Ok(None)
        }
        Err(error) => {
            emit_progress(&app, &job_id, "error", 30.0, "Highlight search failed", None);
            Err(error)
        }
    }
}

#[tauri::command]
fn cancel_export(jobs: State<'_, JobManager>, job_id: String) -> bool {
    let Ok(map) = jobs.0.lock() else {
        return false;
    };
    let Some(flag) = map.get(&job_id) else {
        return false;
    };
    flag.store(true, Ordering::Relaxed);
    true
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(JobManager::default())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_min_size(Some(tauri::LogicalSize::new(760.0, 640.0)));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_ffmpeg,
            check_subtitle_burn_in,
            check_whisper,
            list_whisper_models,
            download_whisper_model,
            cancel_model_download,
            delete_whisper_model,
            probe_video,
            start_export,
            analyze_transcript,
            find_highlights,
            check_llm,
            cancel_export
        ])
        .run(tauri::generate_context!())
        .expect("error while running DropCut");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_machine_readable_ffmpeg_progress() {
        assert_eq!(parse_ffmpeg_time("out_time_ms=12500000"), Some(12.5));
    }

    #[test]
    fn parses_regular_ffmpeg_progress_as_fallback() {
        assert_eq!(
            parse_ffmpeg_time("frame=2 time=00:01:02.50 speed=1x"),
            Some(62.5)
        );
    }

    #[test]
    fn rejects_unknown_presets() {
        let settings = ExportSettings {
            preset: "unknown".into(),
            quality: "recommended".into(),
            max_size_mb: None,
            width: None,
            height: None,
            fps: None,
            codec: "h264".into(),
            bitrate_kbps: None,
            subtitles: SubtitleSettings::default(),
            silence_removal: SilenceRemovalSettings::default(),
            audio_normalization: AudioNormalizationSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            reframe: ReframeSettings::default(),
            cut_ranges: Vec::new(),
            auto_zoom: AutoZoomSettings::default(),
        };
        assert!(build_video_args(&settings, 10.0, None, &EditPlan { keep_ranges: &[], loudness: None, noise_filter: None, reframe: None, ..EditPlan::default() }).is_err());
    }

    #[test]
    fn discord_bitrate_respects_target_size() {
        let settings = ExportSettings {
            preset: "discord".into(),
            quality: "recommended".into(),
            max_size_mb: Some(25.0),
            width: None,
            height: None,
            fps: None,
            codec: "h264".into(),
            bitrate_kbps: None,
            subtitles: SubtitleSettings::default(),
            silence_removal: SilenceRemovalSettings::default(),
            audio_normalization: AudioNormalizationSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            reframe: ReframeSettings::default(),
            cut_ranges: Vec::new(),
            auto_zoom: AutoZoomSettings::default(),
        };
        let args = build_video_args(&settings, 60.0, None, &EditPlan { keep_ranges: &[], loudness: None, noise_filter: None, reframe: None, ..EditPlan::default() }).unwrap();
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-b:v" && pair[1].ends_with('k')));
    }

    #[test]
    fn burn_in_is_appended_after_resize_filter() {
        let settings = ExportSettings {
            preset: "shorts".into(),
            quality: "recommended".into(),
            max_size_mb: None,
            width: None,
            height: None,
            fps: None,
            codec: "h264".into(),
            bitrate_kbps: None,
            subtitles: SubtitleSettings::default(),
            silence_removal: SilenceRemovalSettings::default(),
            audio_normalization: AudioNormalizationSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            reframe: ReframeSettings::default(),
            cut_ranges: Vec::new(),
            auto_zoom: AutoZoomSettings::default(),
        };
        let args = build_video_args(&settings, 60.0, Some(".dropcut-test-subtitles.srt"), &EditPlan { keep_ranges: &[], loudness: None, noise_filter: None, reframe: None, ..EditPlan::default() })
        .unwrap();
        let filter = args
            .windows(2)
            .find(|pair| pair[0] == "-vf")
            .map(|pair| pair[1].as_str())
            .unwrap();
        assert!(filter.starts_with("scale="));
        assert!(filter.ends_with("subtitles=filename=.dropcut-test-subtitles.srt"));
    }

    #[test]
    fn burn_in_rejects_unsafe_subtitle_filenames() {
        let settings = ExportSettings {
            preset: "original".into(),
            quality: "recommended".into(),
            max_size_mb: None,
            width: None,
            height: None,
            fps: None,
            codec: "h264".into(),
            bitrate_kbps: None,
            subtitles: SubtitleSettings::default(),
            silence_removal: SilenceRemovalSettings::default(),
            audio_normalization: AudioNormalizationSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            reframe: ReframeSettings::default(),
            cut_ranges: Vec::new(),
            auto_zoom: AutoZoomSettings::default(),
        };
        assert!(build_video_args(&settings, 60.0, Some("../unsafe.srt"), &EditPlan { keep_ranges: &[], loudness: None, noise_filter: None, reframe: None, ..EditPlan::default() }).is_err());
    }

    #[test]
    fn parses_silencedetect_timestamps() {
        assert_eq!(
            parse_silence_timestamp(
                "[silencedetect @ 0x1] silence_start: 1.250",
                "silence_start:"
            ),
            Some(1.25)
        );
        assert_eq!(
            parse_silence_timestamp(
                "[silencedetect @ 0x1] silence_end: 3.75 | silence_duration: 2.5",
                "silence_end:"
            ),
            Some(3.75)
        );
    }

    #[test]
    fn silence_padding_is_retained_in_keep_ranges() {
        let keep = build_keep_ranges(
            &[TimeRange {
                start: 1.0,
                end: 3.0,
            }],
            5.0,
            0.15,
        )
        .unwrap();
        assert_eq!(
            keep,
            vec![
                TimeRange {
                    start: 0.0,
                    end: 1.15
                },
                TimeRange {
                    start: 2.85,
                    end: 5.0
                }
            ]
        );
    }

    #[test]
    fn silence_cut_builds_synchronized_video_and_audio_graph() {
        let settings = ExportSettings {
            preset: "original".into(),
            quality: "recommended".into(),
            max_size_mb: None,
            width: None,
            height: None,
            fps: None,
            codec: "h264".into(),
            bitrate_kbps: None,
            subtitles: SubtitleSettings::default(),
            silence_removal: SilenceRemovalSettings::default(),
            audio_normalization: AudioNormalizationSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            reframe: ReframeSettings::default(),
            cut_ranges: Vec::new(),
            auto_zoom: AutoZoomSettings::default(),
        };
        let args = build_video_args(&settings, 4.0, None, &EditPlan { keep_ranges: &[
                TimeRange {
                    start: 0.0,
                    end: 1.0,
                },
                TimeRange {
                    start: 2.0,
                    end: 5.0,
                },
            ], loudness: None, noise_filter: None, reframe: None, ..EditPlan::default() })
        .unwrap();
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .unwrap();
        assert!(graph.contains("[0:v]trim=start=0.000000:end=1.000000"));
        assert!(graph.contains("[0:a]atrim=start=2.000000:end=5.000000"));
        assert!(graph.contains("concat=n=2:v=1:a=1[cut_video][cut_audio]"));
        assert!(args.windows(2).any(|pair| pair == ["-map", "[cut_video]"]));
        assert!(args.windows(2).any(|pair| pair == ["-map", "[cut_audio]"]));
    }

    #[test]
    fn normalization_uses_loudnorm_after_silence_concat() {
        let settings = ExportSettings {
            preset: "original".into(),
            quality: "recommended".into(),
            max_size_mb: None,
            width: None,
            height: None,
            fps: None,
            codec: "h264".into(),
            bitrate_kbps: None,
            subtitles: SubtitleSettings::default(),
            silence_removal: SilenceRemovalSettings::default(),
            audio_normalization: AudioNormalizationSettings {
                enabled: true,
                target_lufs: -16.0,
                loudness_range: 11.0,
                true_peak_db: -1.5,
            },
            noise_reduction: NoiseReductionSettings::default(),
            reframe: ReframeSettings::default(),
            cut_ranges: Vec::new(),
            auto_zoom: AutoZoomSettings::default(),
        };
        let ranges = [TimeRange {
            start: 0.0,
            end: 3.0,
        }];
        let measurements = LoudnessMeasurements {
            input_i: -28.13,
            input_lra: 4.7,
            input_tp: -26.61,
            input_thresh: -40.16,
            target_offset: -5.71,
        };
        let args = build_video_args(&settings, 3.0, None, &EditPlan { keep_ranges: &ranges, loudness: Some(&measurements), noise_filter: None, reframe: None, ..EditPlan::default() }).unwrap();
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .unwrap();
        assert!(graph.contains(
            "[cut_audio]aformat=sample_rates=48000:channel_layouts=stereo,loudnorm=I=-16.00:LRA=11.00:TP=-1.50:measured_I=-28.13:measured_LRA=4.70:measured_TP=-26.61:measured_thresh=-40.16:offset=-5.71:linear=true,aresample=48000,aformat=sample_rates=48000:channel_layouts=stereo[final_audio]"
        ));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-map", "[final_audio]"]));
    }

    #[test]
    fn normalization_rejects_out_of_range_target() {
        let settings = ExportSettings {
            preset: "original".into(),
            quality: "recommended".into(),
            max_size_mb: None,
            width: None,
            height: None,
            fps: None,
            codec: "h264".into(),
            bitrate_kbps: None,
            subtitles: SubtitleSettings::default(),
            silence_removal: SilenceRemovalSettings::default(),
            audio_normalization: AudioNormalizationSettings {
                enabled: true,
                target_lufs: -30.0,
                loudness_range: 11.0,
                true_peak_db: -1.5,
            },
            noise_reduction: NoiseReductionSettings::default(),
            reframe: ReframeSettings::default(),
            cut_ranges: Vec::new(),
            auto_zoom: AutoZoomSettings::default(),
        };
        assert!(build_video_args(&settings, 3.0, None, &EditPlan { keep_ranges: &[], loudness: None, noise_filter: None, reframe: None, ..EditPlan::default() }).is_err());
    }

    #[test]
    fn parses_loudnorm_json_from_ffmpeg_output() {
        let output = r#"FFmpeg log line
{
  "input_i": "-28.13",
  "input_tp": "-26.61",
  "input_lra": "4.70",
  "input_thresh": "-40.16",
  "target_offset": "-5.71"
}
"#;
        assert_eq!(
            parse_loudness_measurements(output).unwrap(),
            LoudnessMeasurements {
                input_i: -28.13,
                input_lra: 4.7,
                input_tp: -26.61,
                input_thresh: -40.16,
                target_offset: -5.71,
            }
        );
    }

    fn plain_settings(preset: &str) -> ExportSettings {
        ExportSettings {
            preset: preset.into(),
            quality: "recommended".into(),
            max_size_mb: None,
            width: None,
            height: None,
            fps: None,
            codec: "h264".into(),
            bitrate_kbps: None,
            subtitles: SubtitleSettings::default(),
            silence_removal: SilenceRemovalSettings::default(),
            audio_normalization: AudioNormalizationSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            reframe: ReframeSettings::default(),
            cut_ranges: Vec::new(),
            auto_zoom: AutoZoomSettings::default(),
        }
    }

    #[test]
    fn noise_reduction_maps_strengths_and_rejects_unknown_ones() {
        let filter = |enabled, strength: &str| {
            noise_reduction_filter(&NoiseReductionSettings {
                enabled,
                strength: strength.into(),
            })
        };
        assert_eq!(filter(false, "bogus").unwrap(), None);
        assert!(filter(true, "weak").unwrap().unwrap().contains("afftdn"));
        assert!(filter(true, "strong").unwrap().unwrap().contains("afftdn"));
        assert!(filter(true, "bogus").is_err());
    }

    #[test]
    fn noise_reduction_runs_before_loudness_normalization() {
        let mut settings = plain_settings("original");
        settings.audio_normalization = AudioNormalizationSettings {
            enabled: true,
            target_lufs: -16.0,
            loudness_range: 11.0,
            true_peak_db: -1.5,
        };
        let measurements = LoudnessMeasurements {
            input_i: -28.0,
            input_lra: 4.7,
            input_tp: -26.0,
            input_thresh: -40.0,
            target_offset: 0.5,
        };
        let args = build_video_args(&settings, 10.0, None, &EditPlan { keep_ranges: &[], loudness: Some(&measurements), noise_filter: Some("afftdn=nr=14"), reframe: None, ..EditPlan::default() })
        .unwrap();
        let filter = &args[args.iter().position(|arg| arg == "-af").unwrap() + 1];
        assert!(filter.starts_with("afftdn=nr=14,aformat="));

        let denoise_only = build_video_args(&plain_settings("original"), 10.0, None, &EditPlan { keep_ranges: &[], loudness: None, noise_filter: Some("afftdn=nr=14"), reframe: None, ..EditPlan::default() })
        .unwrap();
        assert!(denoise_only.windows(2).any(|pair| pair == ["-af", "afftdn=nr=14"]));
    }

    fn sample_plan() -> reframe::ReframePlan {
        reframe::ReframePlan {
            crop_width: 606,
            crop_height: 1080,
            initial_x: 100,
            command_file: Some(".dropcut-job-reframe.cmd".into()),
        }
    }

    #[test]
    fn reframing_runs_on_the_source_timeline_before_trimming() {
        let ranges = [
            TimeRange {
                start: 0.0,
                end: 1.0,
            },
            TimeRange {
                start: 2.0,
                end: 3.0,
            },
        ];
        let args = build_video_args(&plain_settings("shorts"), 2.0, None, &EditPlan { keep_ranges: &ranges, loudness: None, noise_filter: None, reframe: Some(&sample_plan()), ..EditPlan::default() })
        .unwrap();
        let graph = &args[args.iter().position(|arg| arg == "-filter_complex").unwrap() + 1];
        assert!(graph.starts_with(
            "[0:v]sendcmd=f=.dropcut-job-reframe.cmd,crop@rf=w=606:h=1080:x=100:y=0,split=2[r0][r1];"
        ));
        assert!(graph.contains("[r0]trim=start=0.000000:end=1.000000"));
        assert!(graph.contains("[r1]trim=start=2.000000:end=3.000000"));
        assert!(graph.contains("scale=1080:1920"));
    }

    #[test]
    fn reframing_without_cuts_uses_a_plain_video_filter() {
        let args = build_video_args(&plain_settings("tiktok"), 10.0, None, &EditPlan { keep_ranges: &[], loudness: None, noise_filter: None, reframe: Some(&sample_plan()), ..EditPlan::default() })
        .unwrap();
        let filter = &args[args.iter().position(|arg| arg == "-vf").unwrap() + 1];
        assert!(filter.starts_with("sendcmd=f=.dropcut-job-reframe.cmd,crop@rf="));
        assert!(filter.ends_with("scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920"));
    }

    #[test]
    fn reframing_rejects_landscape_destinations() {
        for preset in ["youtube", "x"] {
            assert!(build_video_args(&plain_settings(preset), 10.0, None, &EditPlan { keep_ranges: &[], loudness: None, noise_filter: None, reframe: Some(&sample_plan()), ..EditPlan::default() })
            .is_err());
        }
    }

    #[test]
    fn rotated_phone_video_reports_its_displayed_size() {
        assert_eq!(display_size(1920, 1080, 0), (1920, 1080));
        assert_eq!(display_size(1920, 1080, 90), (1080, 1920));
        assert_eq!(display_size(1920, 1080, -90), (1080, 1920));
        assert_eq!(display_size(1920, 1080, 270), (1080, 1920));
        assert_eq!(display_size(1920, 1080, 180), (1920, 1080));
        let stream = serde_json::json!({"side_data_list": [{"rotation": -90}]});
        assert_eq!(stream_rotation(&stream), -90);
        let legacy = serde_json::json!({"tags": {"rotate": "90"}});
        assert_eq!(stream_rotation(&legacy), 90);
        assert_eq!(stream_rotation(&serde_json::json!({})), 0);
    }

    #[test]
    fn language_and_job_ids_are_validated() {
        assert!(validated_language("ja").is_ok());
        assert!(validated_language("fr; rm -rf").is_err());
        assert!(validate_job_id("export-123").is_ok());
        assert!(validate_job_id("../x").is_err());
    }

    /// Manual check that the generated arguments are accepted by a real FFmpeg:
    /// `DROPCUT_TEST_VIDEO=landscape_with_audio.mp4 cargo test ffmpeg_accepts -- --ignored`
    #[test]
    #[ignore]
    fn ffmpeg_accepts_the_generated_graphs() {
        let source = std::env::var("DROPCUT_TEST_VIDEO").expect("DROPCUT_TEST_VIDEO");
        let dir = std::env::temp_dir().join("dropcut-graph-test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join(".cmds.cmd"),
            "0.5 crop@rf x 100;\n1.5 crop@rf x 300;\n2.5 crop@rf x 500;\n",
        )
        .unwrap();
        let plan = reframe::ReframePlan {
            crop_width: 404,
            crop_height: 720,
            initial_x: 0,
            command_file: Some(".cmds.cmd".into()),
        };
        let measurements = LoudnessMeasurements {
            input_i: -28.0,
            input_lra: 4.7,
            input_tp: -26.0,
            input_thresh: -40.0,
            target_offset: 0.5,
        };
        let ranges = [
            TimeRange { start: 0.0, end: 1.5 },
            TimeRange { start: 2.5, end: 4.0 },
        ];
        let mut settings = plain_settings("tiktok");
        settings.audio_normalization = AudioNormalizationSettings {
            enabled: true,
            target_lufs: -16.0,
            loudness_range: 11.0,
            true_peak_db: -1.5,
        };
        let zoom = auto_zoom_filter(
            &AutoZoomSettings {
                template: "shorts".into(),
            },
            404,
            720,
            30.0,
        )
        .unwrap();
        for (name, keep) in [("cut", &ranges[..]), ("whole", &[][..])] {
            for noise in [
                "afftdn=nr=8:nf=-55:tn=1",
                "highpass=f=70,afftdn=nr=14:nf=-50:tn=1",
                "highpass=f=80,afftdn=nr=24:nf=-45:tn=1",
            ] {
                let args = build_video_args(&settings, 3.0, None, &EditPlan { keep_ranges: keep, loudness: Some(&measurements), noise_filter: Some(noise), reframe: Some(&plan), zoom_filter: zoom.as_deref(), ..EditPlan::default() })
                .unwrap();
                let output = dir.join(format!("{name}.mp4"));
                let status = std::process::Command::new("ffmpeg")
                    .current_dir(&dir)
                    .args(["-v", "error", "-y", "-i", &source])
                    .args(&args)
                    .arg(&output)
                    .status()
                    .unwrap();
                assert!(status.success(), "{name} with {noise}");
                let probe = std::process::Command::new("ffprobe")
                    .args(["-v", "error", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
                    .arg(&output)
                    .output()
                    .unwrap();
                assert!(String::from_utf8_lossy(&probe.stdout).contains("1080,1920"));
            }
        }
    }

    #[test]
    fn auto_zoom_is_gentle_and_validated() {
        let zoom = |template: &str| {
            auto_zoom_filter(
                &AutoZoomSettings {
                    template: template.into(),
                },
                1281,
                721,
                29.97,
            )
        };
        assert_eq!(zoom("none").unwrap(), None);
        assert_eq!(zoom("").unwrap(), None);
        let filter = zoom("shorts").unwrap().unwrap();
        assert!(filter.starts_with("zoompan=z='1+0.08*"));
        assert!(filter.contains("s=1280x720:fps=29970/1000"));
        assert!(filter.contains("\\,"));
        assert!(zoom("natural").unwrap().unwrap().contains("1+0.03*"));
        assert!(zoom("wild").is_err());
        // Peak zoom is amplitude * 1.5 above 100%, never more than 12%.
        for (template, amplitude) in [("natural", 0.03), ("youtube", 0.05), ("shorts", 0.08)] {
            assert!(zoom(template).unwrap().unwrap().contains(&format!("1+{amplitude}*")));
            assert!(amplitude * 1.5 <= 0.12 + 1e-9);
        }
    }

    #[test]
    fn zoom_and_reframe_share_the_source_timeline_chain() {
        let ranges = [
            TimeRange {
                start: 0.0,
                end: 1.0,
            },
            TimeRange {
                start: 2.0,
                end: 3.0,
            },
        ];
        let plan = sample_plan();
        let args = build_video_args(
            &plain_settings("shorts"),
            2.0,
            None,
            &EditPlan {
                keep_ranges: &ranges,
                reframe: Some(&plan),
                zoom_filter: Some("zoompan=ZOOM"),
                ..EditPlan::default()
            },
        )
        .unwrap();
        let graph = &args[args.iter().position(|arg| arg == "-filter_complex").unwrap() + 1];
        assert!(graph.contains("y=0,zoompan=ZOOM,split=2[r0][r1];"));

        let zoom_only = build_video_args(
            &plain_settings("original"),
            2.0,
            None,
            &EditPlan {
                zoom_filter: Some("zoompan=ZOOM"),
                ..EditPlan::default()
            },
        )
        .unwrap();
        assert!(zoom_only.windows(2).any(|pair| pair == ["-vf", "zoompan=ZOOM"]));
    }

    #[test]
    fn subtitles_are_laid_out_for_the_output_size() {
        let settings = plain_settings("tiktok");
        assert_eq!(output_dimensions(&settings, (1920, 1080), None), (1080, 1920));
        let original = plain_settings("original");
        assert_eq!(output_dimensions(&original, (1920, 1080), None), (1920, 1080));
        assert_eq!(
            output_dimensions(&original, (1920, 1080), Some((606, 1080))),
            (606, 1080)
        );
    }

    #[test]
    fn frame_rates_are_parsed_from_ffprobe() {
        assert!((parse_frame_rate("30000/1001") - 29.97).abs() < 0.001);
        assert_eq!(parse_frame_rate("30/1"), 30.0);
        assert_eq!(parse_frame_rate("0/0"), 0.0);
        assert_eq!(parse_frame_rate(""), 0.0);
    }
}
