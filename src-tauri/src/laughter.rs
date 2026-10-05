//! Finds laughter in the audio itself. Whisper rarely writes laughter down and
//! the VAD drops it before Whisper hears it, so subtitles would otherwise
//! never show it. Laughter is a run of short, evenly spaced loudness bursts
//! ("ha-ha-ha"); that rhythm is what is looked for, and only in stretches where
//! no subtitle exists, so speech is never overwritten.

pub const SAMPLE_RATE: usize = 16_000;
const HOP: usize = SAMPLE_RATE / 100; // 10 ms
const MIN_PEAKS: usize = 4;
const MIN_PROMINENCE_DB: f64 = 6.0;
const MIN_ABOVE_FLOOR_DB: f64 = 12.0;
const MIN_LEVEL_DB: f64 = -45.0;
const MIN_PEAK_GAP: usize = 8; // 80 ms
const BURST_GAP: std::ops::RangeInclusive<usize> = 10..=40; // 100-400 ms
const MAX_IRREGULARITY: f64 = 0.4;
const JOIN_SECONDS: f64 = 1.0;
const MIN_SEGMENT_SECONDS: f64 = 0.5;

/// Samples of a 16-bit mono PCM WAV file, or `None` when it is not one.
pub fn wav_samples(bytes: &[u8]) -> Option<Vec<i16>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut position = 12;
    while position + 8 <= bytes.len() {
        let id = &bytes[position..position + 4];
        let size = u32::from_le_bytes(bytes[position + 4..position + 8].try_into().ok()?) as usize;
        let start = position + 8;
        if id == b"data" {
            // Streamed WAVs may claim an impossible size; trust the file length.
            let end = start.saturating_add(size).min(bytes.len());
            return Some(
                bytes[start..end]
                    .chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                    .collect(),
            );
        }
        position = start.saturating_add(size).saturating_add(size & 1);
    }
    None
}

/// Loudness in dB for every 10 ms, lightly smoothed.
fn envelope(samples: &[i16]) -> Vec<f64> {
    let frames = samples.len() / HOP;
    let power: Vec<f64> = (0..frames)
        .map(|index| {
            samples[index * HOP..(index + 1) * HOP]
                .iter()
                .map(|&sample| {
                    let value = sample as f64 / 32768.0;
                    value * value
                })
                .sum::<f64>()
                / HOP as f64
        })
        .collect();
    (0..frames)
        .map(|index| {
            let low = index.saturating_sub(1);
            let high = (index + 2).min(frames);
            let mean = power[low..high].iter().sum::<f64>() / (high - low) as f64;
            10.0 * mean.max(1e-10).log10()
        })
        .collect()
}

/// Frame indexes of distinct loudness bursts.
fn burst_peaks(env: &[f64]) -> Vec<usize> {
    if env.len() < 3 {
        return Vec::new();
    }
    let mut sorted = env.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let floor = sorted[sorted.len() / 10];
    let threshold = (floor + MIN_ABOVE_FLOOR_DB).max(MIN_LEVEL_DB);
    let mut peaks: Vec<usize> = Vec::new();
    for index in 1..env.len() - 1 {
        if env[index] < env[index - 1] || env[index] <= env[index + 1] || env[index] < threshold {
            continue;
        }
        let before = env[index.saturating_sub(20)..=index].iter().copied().fold(f64::MAX, f64::min);
        let after = env[index..(index + 21).min(env.len())].iter().copied().fold(f64::MAX, f64::min);
        if env[index] - before.max(after) < MIN_PROMINENCE_DB {
            continue;
        }
        match peaks.last().copied() {
            Some(last) if index - last < MIN_PEAK_GAP => {
                if env[index] > env[last] {
                    *peaks.last_mut().unwrap() = index;
                }
            }
            _ => peaks.push(index),
        }
    }
    peaks
}

fn is_regular(run: &[usize]) -> bool {
    let gaps: Vec<f64> = run.windows(2).map(|pair| (pair[1] - pair[0]) as f64).collect();
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    let variance = gaps.iter().map(|gap| (gap - mean).powi(2)).sum::<f64>() / gaps.len() as f64;
    variance.sqrt() / mean <= MAX_IRREGULARITY
}

/// Laughing stretches as (start, end) seconds that do not overlap `occupied`
/// (the time ranges of existing subtitles).
pub fn find_laughter(samples: &[i16], occupied: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let peaks = burst_peaks(&envelope(samples));
    let frame = |index: usize| index as f64 * HOP as f64 / SAMPLE_RATE as f64;
    let mut segments: Vec<(f64, f64)> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let mut flush = |run: &mut Vec<usize>, segments: &mut Vec<(f64, f64)>| {
        if run.len() >= MIN_PEAKS && is_regular(run) {
            segments.push((
                (frame(run[0]) - 0.1).max(0.0),
                frame(*run.last().unwrap()) + 0.25,
            ));
        }
        run.clear();
    };
    for &peak in &peaks {
        if run.last().is_some_and(|&last| !BURST_GAP.contains(&(peak - last))) {
            flush(&mut run, &mut segments);
        }
        run.push(peak);
    }
    flush(&mut run, &mut segments);

    let mut joined: Vec<(f64, f64)> = Vec::new();
    for segment in segments {
        match joined.last_mut() {
            Some(last) if segment.0 - last.1 < JOIN_SECONDS => last.1 = segment.1,
            _ => joined.push(segment),
        }
    }
    let duration = samples.len() as f64 / SAMPLE_RATE as f64;
    let mut free = Vec::new();
    for (start, end) in joined {
        let mut pieces = vec![(start, end.min(duration))];
        for &(cue_start, cue_end) in occupied {
            pieces = pieces
                .into_iter()
                .flat_map(|(a, b)| {
                    if cue_end <= a || cue_start >= b {
                        vec![(a, b)]
                    } else {
                        vec![(a, cue_start.max(a)), (cue_end.min(b), b)]
                    }
                })
                .filter(|(a, b)| b - a > 0.0)
                .collect();
        }
        free.extend(pieces.into_iter().filter(|(a, b)| b - a >= MIN_SEGMENT_SECONDS));
    }
    free
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 150 Hz tone on a quiet noise floor; `gate(t)` says when it sounds.
    fn signal(seconds: f64, gate: impl Fn(f64) -> bool) -> Vec<i16> {
        (0..(seconds * SAMPLE_RATE as f64) as usize)
            .map(|i| {
                let t = i as f64 / SAMPLE_RATE as f64;
                let noise = ((i * 7919) % 200) as f64 - 100.0;
                let tone = if gate(t) { 9000.0 * (t * 150.0 * std::f64::consts::TAU).sin() } else { 0.0 };
                (tone + noise) as i16
            })
            .collect()
    }

    // 5 bursts per second, 80 ms on.
    fn laughing(t: f64) -> bool {
        (1.0..2.6).contains(&t) && (t * 5.0).fract() < 0.4
    }

    #[test]
    fn finds_rhythmic_bursts() {
        let found = find_laughter(&signal(4.0, laughing), &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0 >= 0.8 && found[0].0 <= 1.2, "{found:?}");
        assert!(found[0].1 >= 2.5 && found[0].1 <= 3.0, "{found:?}");
    }

    #[test]
    fn ignores_steady_sound_and_few_bursts() {
        assert!(find_laughter(&signal(4.0, |t| (1.0..3.0).contains(&t)), &[]).is_empty());
        assert!(find_laughter(&signal(4.0, |t| (1.0..1.5).contains(&t) && (t * 5.0).fract() < 0.4), &[]).is_empty());
        assert!(find_laughter(&signal(4.0, |_| false), &[]).is_empty());
    }

    #[test]
    fn never_overlaps_existing_subtitles() {
        let found = find_laughter(&signal(4.0, laughing), &[(0.0, 1.8)]);
        assert!(found.iter().all(|(start, _)| *start >= 1.8), "{found:?}");
        assert!(find_laughter(&signal(4.0, laughing), &[(0.5, 3.5)]).is_empty());
    }

    #[test]
    fn reads_wav_data_chunk() {
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[0; 16]);
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&3u32.to_le_bytes());
        wav.extend_from_slice(&[1, 2, 3, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&u32::MAX.to_le_bytes());
        wav.extend_from_slice(&1000i16.to_le_bytes());
        wav.extend_from_slice(&(-2i16).to_le_bytes());
        assert_eq!(wav_samples(&wav), Some(vec![1000, -2]));
        assert_eq!(wav_samples(b"nope"), None);
    }
}
