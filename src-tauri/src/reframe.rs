//! Automatic 9:16 reframing: local face detection, a smoothed camera track, and
//! the FFmpeg `sendcmd` script that moves a `crop` window along that track.
//!
//! Only "where is a face" is computed. Nothing is identified or stored, and no
//! frame ever leaves this process.

use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tauri::AppHandle;

use crate::{
    emit_progress, friendly_error,
    tools::{self, Tool},
};

const FACE_MODEL: &[u8] = include_bytes!("../resources/models/seeta_fd_frontal_v1.0.bin");
const SAMPLE_FPS: f64 = 4.0;
const ANALYSIS_WIDTH: u32 = 480;
const COMMAND_RATE: f64 = 10.0;
/// Name FFmpeg uses to address the reframing `crop` filter from `sendcmd`.
pub const CROP_INSTANCE: &str = "crop@rf";

/// Smoothed horizontal face position, as a fraction of the frame width, sampled
/// at a fixed rate starting from time zero.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceTrack {
    pub fps: f64,
    pub centers: Vec<f64>,
}

pub enum TrackResult {
    Complete(Option<FaceTrack>),
    Cancelled,
}

/// The crop window applied on the source timeline, before any cutting.
#[derive(Debug, Clone, PartialEq)]
pub struct ReframePlan {
    pub crop_width: u32,
    pub crop_height: u32,
    pub initial_x: u32,
    pub command_file: Option<String>,
}

impl ReframePlan {
    /// Filter chain that crops to 9:16 and lets `sendcmd` steer the window.
    pub fn filter(&self) -> String {
        let crop = format!(
            "{}=w={}:h={}:x={}:y=0",
            CROP_INSTANCE, self.crop_width, self.crop_height, self.initial_x
        );
        match &self.command_file {
            Some(file) => format!("sendcmd=f={file},{crop}"),
            None => crop,
        }
    }
}

/// Size of the 9:16 window for a source, or `None` when the source is already
/// as narrow as 9:16 and needs no reframing.
pub fn portrait_crop_size(width: u64, height: u64) -> Option<(u32, u32)> {
    if width == 0 || height < 2 || width * 16 <= height * 9 {
        return None;
    }
    let crop_height = (height & !1) as u32;
    let crop_width = ((u64::from(crop_height) * 9 / 16) & !1) as u32;
    (crop_width >= 2 && u64::from(crop_width) < width).then_some((crop_width, crop_height))
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

/// Turns noisy per-sample detections into a calm camera path.
///
/// Missing samples are interpolated, outliers are removed with a median filter,
/// and the camera only moves once the face leaves a small dead zone, easing
/// toward it with a capped speed so the crop never jumps.
pub fn smooth_track(raw: &[Option<f64>], fps: f64) -> Vec<f64> {
    let known: Vec<(usize, f64)> = raw
        .iter()
        .enumerate()
        .filter_map(|(index, value)| value.map(|value| (index, value.clamp(0.0, 1.0))))
        .collect();
    if known.is_empty() {
        return Vec::new();
    }
    let mut filled = Vec::with_capacity(raw.len());
    let mut next_known = 0;
    for index in 0..raw.len() {
        while next_known < known.len() && known[next_known].0 < index {
            next_known += 1;
        }
        let after = known.get(next_known).copied();
        let before = next_known.checked_sub(1).map(|position| known[position]);
        filled.push(match (before, after) {
            (Some(_), Some((after_index, after_value))) if after_index == index => after_value,
            (Some((before_index, before_value)), Some((after_index, after_value))) => {
                let span = (after_index - before_index) as f64;
                before_value + (after_value - before_value) * (index - before_index) as f64 / span
            }
            (Some((_, value)), None) | (None, Some((_, value))) => value,
            (None, None) => 0.5,
        });
    }

    let despiked: Vec<f64> = (0..filled.len())
        .map(|index| {
            let start = index.saturating_sub(2);
            let end = (index + 3).min(filled.len());
            median(&mut filled[start..end].to_vec())
        })
        .collect();

    let dt = 1.0 / fps;
    let easing = 1.0 - (-dt / 0.5).exp();
    let dead_zone = 0.03;
    let max_step = 0.2 * dt;
    let mut camera = despiked[0];
    let mut track = Vec::with_capacity(despiked.len());
    track.push(camera);
    for &face in &despiked[1..] {
        let offset = face - camera;
        let target = if offset.abs() > dead_zone {
            face - offset.signum() * dead_zone
        } else {
            camera
        };
        camera += ((target - camera) * easing).clamp(-max_step, max_step);
        track.push(camera);
    }
    track
}

fn sample_at(track: &FaceTrack, time: f64) -> f64 {
    let position = (time * track.fps).max(0.0);
    let index = position.floor() as usize;
    let last = track.centers.len() - 1;
    if index >= last {
        return track.centers[last];
    }
    let fraction = position - index as f64;
    track.centers[index] + (track.centers[index + 1] - track.centers[index]) * fraction
}

fn crop_x(center: f64, source_width: u32, crop_width: u32) -> u32 {
    let max_x = f64::from(source_width - crop_width);
    let x = center * f64::from(source_width) - f64::from(crop_width) / 2.0;
    ((x.clamp(0.0, max_x) / 2.0).round() as u32 * 2).min(max_x as u32 & !1)
}

/// Builds the `sendcmd` script and the initial crop position. A command is only
/// written when the window actually moves.
pub fn build_crop_commands(
    track: &FaceTrack,
    duration: f64,
    source_width: u32,
    crop_width: u32,
) -> (String, u32) {
    let initial_x = crop_x(sample_at(track, 0.0), source_width, crop_width);
    let mut script = String::new();
    let mut last_x = initial_x;
    let steps = (duration * COMMAND_RATE).ceil() as u64;
    for step in 1..=steps {
        let time = step as f64 / COMMAND_RATE;
        let x = crop_x(sample_at(track, time), source_width, crop_width);
        if x != last_x {
            script.push_str(&format!("{time:.3} {CROP_INSTANCE} x {x};\n"));
            last_x = x;
        }
    }
    (script, initial_x)
}

fn pick_face(faces: &[(f64, f64)], previous: Option<f64>) -> Option<f64> {
    let largest = faces.iter().map(|face| face.1).fold(0.0, f64::max);
    faces
        .iter()
        .filter(|face| face.1 >= largest * 0.5)
        .min_by(|a, b| match previous {
            Some(previous) => (a.0 - previous).abs().total_cmp(&(b.0 - previous).abs()),
            None => b.1.total_cmp(&a.1),
        })
        .map(|face| face.0)
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// Reads consecutive gray frames and returns the horizontal position of the
/// chosen face in each (`None` when no face was found). `Ok(None)` means the
/// job was cancelled.
fn detect_positions<R: Read>(
    reader: &mut R,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
    mut on_frame: impl FnMut(usize),
) -> Result<Option<Vec<Option<f64>>>, std::io::Error> {
    let model = rustface::read_model(FACE_MODEL)?;
    let mut detector = rustface::create_detector_with_model(model);
    detector.set_min_face_size(24);
    detector.set_score_thresh(2.0);
    detector.set_pyramid_scale_factor(0.8);
    detector.set_slide_window_step(4, 4);

    let mut frame = vec![0_u8; (width * height) as usize];
    let mut raw: Vec<Option<f64>> = Vec::new();
    let mut previous = None;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        match reader.read_exact(&mut frame) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        }
        let faces: Vec<(f64, f64)> = detector
            .detect(&rustface::ImageData::new(&frame, width, height))
            .iter()
            .map(|face| {
                let bbox = face.bbox();
                let center = f64::from(bbox.x()) + f64::from(bbox.width()) / 2.0;
                (
                    (center / f64::from(width)).clamp(0.0, 1.0),
                    f64::from(bbox.width()) * f64::from(bbox.height()),
                )
            })
            .collect();
        let chosen = pick_face(&faces, previous);
        if chosen.is_some() {
            previous = chosen;
        }
        raw.push(chosen);
        on_frame(raw.len());
    }
    Ok(Some(raw))
}

fn run_tracking(
    ffmpeg: std::path::PathBuf,
    app: AppHandle,
    input: std::path::PathBuf,
    source_width: u64,
    source_height: u64,
    duration: f64,
    cancelled: Arc<AtomicBool>,
    job_id: String,
) -> Result<TrackResult, String> {
    let width = ANALYSIS_WIDTH.min(source_width as u32) & !1;
    let height = ((u64::from(width) * source_height / source_width.max(1)) as u32 & !1).max(2);
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(&input)
        .args(["-an", "-sn", "-vf"])
        .arg(format!("fps={SAMPLE_FPS},scale={width}:{height},format=gray"))
        .args(["-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| friendly_error(&error.to_string()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| friendly_error("FFmpeg frame output is unavailable"))?;

    let expected_frames = (duration * SAMPLE_FPS).max(1.0);
    let positions = detect_positions(
        &mut stdout,
        width,
        height,
        &cancelled,
        |frames| {
            if frames % 20 == 0 {
                emit_progress(
                    &app,
                    &job_id,
                    "trackingFaces",
                    4.0 + (frames as f64 / expected_frames).min(1.0) * 4.0,
                    "Finding faces to keep in frame",
                    None,
                );
            }
        },
    );
    let raw = match positions {
        Ok(Some(raw)) => raw,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(TrackResult::Cancelled);
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(friendly_error(&error.to_string()));
        }
    };
    let _ = child.wait();

    let detected = raw.iter().flatten().count();
    if raw.is_empty() || detected * 20 < raw.len() {
        return Ok(TrackResult::Complete(None));
    }
    Ok(TrackResult::Complete(Some(FaceTrack {
        fps: SAMPLE_FPS,
        centers: smooth_track(&raw, SAMPLE_FPS),
    })))
}

pub async fn track_faces(
    app: &AppHandle,
    input: &Path,
    source_width: u64,
    source_height: u64,
    duration: f64,
    cancelled: &Arc<AtomicBool>,
    job_id: &str,
) -> Result<TrackResult, String> {
    let ffmpeg = tools::resolve(app, Tool::Ffmpeg)
        .await
        .ok_or_else(|| "FFmpeg is not bundled and was not found on PATH.".to_string())?;
    emit_progress(
        app,
        job_id,
        "trackingFaces",
        4.0,
        "Finding faces to keep in frame",
        None,
    );
    let app = app.clone();
    let input = input.to_path_buf();
    let cancelled = cancelled.clone();
    let job_id = job_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        run_tracking(
            ffmpeg,
            app,
            input,
            source_width,
            source_height,
            duration,
            cancelled,
            job_id,
        )
    })
    .await
    .map_err(|error| friendly_error(&error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portrait_crop_matches_nine_by_sixteen() {
        assert_eq!(portrait_crop_size(1920, 1080), Some((606, 1080)));
        assert_eq!(portrait_crop_size(1280, 720), Some((404, 720)));
    }

    #[test]
    fn portrait_sources_need_no_crop() {
        assert_eq!(portrait_crop_size(1080, 1920), None);
        assert_eq!(portrait_crop_size(720, 1280), None);
        assert_eq!(portrait_crop_size(0, 1080), None);
    }

    #[test]
    fn smoothing_ignores_small_jitter() {
        let raw: Vec<Option<f64>> = (0..40)
            .map(|index| Some(0.5 + if index % 2 == 0 { 0.01 } else { -0.01 }))
            .collect();
        let track = smooth_track(&raw, 4.0);
        assert!(track.iter().all(|value| (value - 0.5).abs() < 0.02));
        let spread = track.iter().cloned().fold(f64::MIN, f64::max)
            - track.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread < 0.02);
    }

    #[test]
    fn smoothing_follows_a_move_without_jumping() {
        let mut raw = vec![Some(0.3); 20];
        raw.extend(vec![Some(0.7); 60]);
        let track = smooth_track(&raw, 4.0);
        let step = track
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f64::max);
        assert!(step <= 0.2 / 4.0 + 1e-9);
        assert!(*track.last().unwrap() > 0.6);
    }

    #[test]
    fn smoothing_fills_gaps_and_leading_samples() {
        let raw = vec![None, None, Some(0.4), None, None, Some(0.4), None];
        let track = smooth_track(&raw, 4.0);
        assert_eq!(track.len(), raw.len());
        assert!(track.iter().all(|value| (value - 0.4).abs() < 1e-9));
        assert!(smooth_track(&[None, None], 4.0).is_empty());
    }

    #[test]
    fn crop_positions_stay_inside_the_frame_and_even() {
        for center in [0.0, 0.01, 0.5, 0.99, 1.0] {
            let x = crop_x(center, 1920, 606);
            assert!(x + 606 <= 1920);
            assert_eq!(x % 2, 0);
        }
        assert_eq!(crop_x(0.0, 1920, 606), 0);
        assert_eq!(crop_x(1.0, 1920, 606), 1314);
    }

    #[test]
    fn commands_only_appear_when_the_window_moves() {
        let still = FaceTrack {
            fps: 4.0,
            centers: vec![0.5; 40],
        };
        let (script, initial) = build_crop_commands(&still, 10.0, 1920, 606);
        assert!(script.is_empty());
        assert_eq!(initial, 658);

        let moving = FaceTrack {
            fps: 4.0,
            centers: (0..40).map(|index| 0.3 + index as f64 * 0.01).collect(),
        };
        let (script, _) = build_crop_commands(&moving, 10.0, 1920, 606);
        assert!(!script.is_empty());
        let mut previous_time = -1.0;
        for line in script.lines() {
            let parts: Vec<&str> = line.trim_end_matches(';').split_whitespace().collect();
            assert_eq!(parts[1], CROP_INSTANCE);
            assert_eq!(parts[2], "x");
            let time: f64 = parts[0].parse().unwrap();
            assert!(time > previous_time);
            previous_time = time;
        }
    }

    #[test]
    fn largest_nearby_face_wins() {
        let faces = [(0.2, 100.0), (0.8, 400.0)];
        assert_eq!(pick_face(&faces, None), Some(0.8));
        assert_eq!(pick_face(&faces, Some(0.25)), Some(0.8));
        let similar = [(0.2, 300.0), (0.8, 400.0)];
        assert_eq!(pick_face(&similar, Some(0.25)), Some(0.2));
        assert_eq!(pick_face(&[], None), None);
    }

    #[test]
    fn plan_filter_addresses_the_named_crop() {
        let plan = ReframePlan {
            crop_width: 606,
            crop_height: 1080,
            initial_x: 100,
            command_file: Some(".dropcut-a-reframe.cmd".into()),
        };
        assert_eq!(
            plan.filter(),
            "sendcmd=f=.dropcut-a-reframe.cmd,crop@rf=w=606:h=1080:x=100:y=0"
        );
        let still = ReframePlan {
            command_file: None,
            ..plan
        };
        assert_eq!(still.filter(), "crop@rf=w=606:h=1080:x=100:y=0");
    }

    #[test]
    fn finds_no_face_in_a_blank_frame() {
        let model = rustface::read_model(FACE_MODEL).unwrap();
        let mut detector = rustface::create_detector_with_model(model);
        detector.set_min_face_size(24);
        detector.set_score_thresh(2.0);
        let blank = vec![128_u8; 320 * 180];
        assert!(detector
            .detect(&rustface::ImageData::new(&blank, 320, 180))
            .is_empty());
    }

    /// Manual end-to-end check: `DROPCUT_TEST_VIDEO=clip.mp4 DROPCUT_TEST_WIDTH=1280
    /// DROPCUT_TEST_HEIGHT=720 cargo test follows_a_face -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn follows_a_face_in_a_real_clip() {
        let path = std::env::var("DROPCUT_TEST_VIDEO").expect("DROPCUT_TEST_VIDEO");
        let source_width: u32 = std::env::var("DROPCUT_TEST_WIDTH").unwrap().parse().unwrap();
        let source_height: u64 = std::env::var("DROPCUT_TEST_HEIGHT").unwrap().parse().unwrap();
        let width = ANALYSIS_WIDTH & !1;
        let height = ((u64::from(width) * source_height / u64::from(source_width)) as u32 & !1).max(2);
        let mut child = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i", &path, "-an", "-vf"])
            .arg(format!("fps={SAMPLE_FPS},scale={width}:{height},format=gray"))
            .args(["-f", "rawvideo", "-pix_fmt", "gray", "-"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let raw = detect_positions(&mut stdout, width, height, &AtomicBool::new(false), |_| {})
            .unwrap()
            .unwrap();
        let detected = raw.iter().flatten().count();
        println!("frames={} detected={detected}", raw.len());
        assert!(detected * 2 > raw.len(), "the face should be found in most frames");
        let track = FaceTrack {
            fps: SAMPLE_FPS,
            centers: smooth_track(&raw, SAMPLE_FPS),
        };
        let (crop_width, _) = portrait_crop_size(u64::from(source_width), source_height).unwrap();
        let (script, initial_x) = build_crop_commands(&track, raw.len() as f64 / SAMPLE_FPS, source_width, crop_width);
        println!("initial_x={initial_x} commands={}", script.lines().count());
        println!("{}", script.lines().step_by(8).collect::<Vec<_>>().join("\n"));
        let last_x: u32 = script.lines().last().unwrap().trim_end_matches(';').rsplit(' ').next().unwrap().parse().unwrap();
        assert!(last_x > initial_x + 200, "the crop should follow the face to the right");
    }
}
