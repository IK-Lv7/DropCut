//! Local highlight detection: scores every second of a video from speech
//! density, loudness and keywords, then picks the best non-overlapping windows.
//! No models or network are involved, so results are deterministic and offline.

use crate::transcript::TranscriptWord;
use serde::Serialize;

pub const LEVEL_SAMPLE_RATE: u32 = 8000;
const SNAP_SECONDS: f64 = 5.0;
const MIN_TARGET: f64 = 10.0;
const SENTENCE_END: &[char] = &['。', '.', '!', '?', '！', '？'];

const KEYWORDS: &[&str] = &[
    "important", "amazing", "incredible", "secret", "never", "always", "best", "worst",
    "biggest", "finally", "crazy", "wow", "warning", "tip", "how to", "why", "surprise",
    "重要", "大事", "すごい", "凄い", "やばい", "ヤバい", "最高", "最悪", "実は", "結論",
    "ポイント", "コツ", "秘密", "絶対", "驚", "衝撃", "なぜ", "方法",
];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub start: f64,
    pub end: f64,
    pub score: f64,
    pub reason: &'static str,
    /// Short title suggested by the local model, when it was used.
    pub title: Option<String>,
}

/// Loudness in dBFS for each whole second of 16-bit mono PCM.
pub fn audio_levels(pcm: &[u8], sample_rate: u32) -> Vec<f64> {
    let per_second = sample_rate as usize * 2;
    pcm.chunks(per_second)
        .map(|chunk| {
            let samples = chunk.len() / 2;
            if samples == 0 {
                return -90.0;
            }
            let sum: f64 = chunk
                .chunks_exact(2)
                .map(|b| {
                    let v = i16::from_le_bytes([b[0], b[1]]) as f64 / 32768.0;
                    v * v
                })
                .sum();
            (10.0 * (sum / samples as f64).max(1e-9).log10()).max(-90.0)
        })
        .collect()
}

fn z_scores(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    let deviation = variance.sqrt();
    if deviation < 1e-6 {
        return vec![0.0; values.len()];
    }
    values.iter().map(|v| (v - mean) / deviation).collect()
}

fn keyword_hits(text: &str) -> usize {
    let lower = text.to_lowercase();
    KEYWORDS.iter().filter(|keyword| lower.contains(*keyword)).count()
}

struct Seconds {
    speech: Vec<f64>,
    loudness: Vec<f64>,
    keywords: Vec<f64>,
}

impl Seconds {
    fn total(&self, index: usize) -> f64 {
        self.speech[index] + 0.7 * self.loudness[index] + self.keywords[index]
    }
}

fn per_second_features(words: &[TranscriptWord], levels: &[f64], seconds: usize) -> Seconds {
    let mut density = vec![0.0; seconds];
    let mut keywords = vec![0.0; seconds];
    for word in words {
        if word.filler == Some("high") {
            continue;
        }
        let index = (word.start.max(0.0) as usize).min(seconds - 1);
        density[index] += 1.0;
        keywords[index] += 1.5 * keyword_hits(&word.text) as f64;
    }
    let mut loudness: Vec<f64> = (0..seconds)
        .map(|i| levels.get(i).copied().unwrap_or(-90.0))
        .collect();
    // Near-silence must not look like a quiet-but-average moment.
    for (i, value) in loudness.iter_mut().enumerate() {
        if density[i] == 0.0 {
            *value = value.min(-60.0);
        }
    }
    Seconds {
        speech: z_scores(&density),
        loudness: z_scores(&loudness),
        keywords,
    }
}

fn ends_sentence(word: &TranscriptWord) -> bool {
    word.text.trim_end().ends_with(SENTENCE_END)
}

/// Moves the start to the beginning of a nearby sentence and the end to the end
/// of a sentence, so clips do not open or close mid-word.
fn snap(words: &[TranscriptWord], start: f64, end: f64) -> (f64, f64) {
    let mut new_start = start;
    let mut new_end = end;
    for (i, word) in words.iter().enumerate() {
        if word.start < start || word.start > start + SNAP_SECONDS {
            continue;
        }
        let opens = i == 0
            || ends_sentence(&words[i - 1])
            || word.start - words[i - 1].end > 0.5;
        if opens {
            new_start = word.start;
            break;
        }
    }
    let mut fallback = None;
    for (i, word) in words.iter().enumerate().rev() {
        if word.end > end || word.end < end - SNAP_SECONDS || word.end <= new_start {
            continue;
        }
        fallback.get_or_insert(word.end);
        let closes = ends_sentence(word)
            || words.get(i + 1).map_or(true, |next| next.start - word.end > 0.5);
        if closes {
            new_end = word.end;
            fallback = None;
            break;
        }
    }
    if let Some(end) = fallback {
        new_end = end;
    }
    if new_end - new_start < MIN_TARGET / 2.0 {
        return (start, end);
    }
    (new_start, new_end)
}

fn reason(features: &Seconds, from: usize, to: usize) -> &'static str {
    let sum = |values: &[f64]| values[from..to].iter().sum::<f64>();
    let speech = sum(&features.speech);
    let loud = 0.7 * sum(&features.loudness);
    let keys = sum(&features.keywords);
    if keys >= speech.max(loud) && keys > 0.0 {
        "Key words"
    } else if loud > speech {
        "Loud, energetic moment"
    } else {
        "Lively conversation"
    }
}

/// Picks up to `count` non-overlapping windows of about `target` seconds,
/// best first.
pub fn find_highlights(
    words: &[TranscriptWord],
    levels: &[f64],
    duration: f64,
    target: f64,
    count: usize,
) -> Vec<Highlight> {
    let seconds = duration.floor() as usize;
    let window = target.max(MIN_TARGET) as usize;
    if seconds < window || count == 0 {
        return Vec::new();
    }
    let features = per_second_features(words, levels, seconds);
    let totals: Vec<f64> = (0..seconds).map(|i| features.total(i)).collect();
    let mut prefix = vec![0.0; seconds + 1];
    for (i, value) in totals.iter().enumerate() {
        prefix[i + 1] = prefix[i] + value;
    }
    let mut scored: Vec<(usize, f64)> = (0..=seconds - window)
        .map(|s| (s, (prefix[s + window] - prefix[s]) / window as f64))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut chosen: Vec<(usize, f64)> = Vec::new();
    for (start, score) in scored {
        if chosen.len() >= count {
            break;
        }
        if chosen.iter().all(|(other, _)| start + window <= *other || *other + window <= start) {
            chosen.push((start, score));
        }
    }
    let mut result: Vec<Highlight> = chosen
        .into_iter()
        .map(|(start, score)| {
            let (s, e) = snap(words, start as f64, (start + window) as f64);
            Highlight {
                start: s,
                end: e.min(duration),
                score: (score * 100.0).round() / 100.0,
                reason: reason(&features, start, start + window),
                title: None,
            }
        })
        .collect();
    result.sort_by(|a, b| b.score.total_cmp(&a.score));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(start: f64, text: &str) -> TranscriptWord {
        TranscriptWord { start, end: start + 0.4, text: text.into(), filler: None }
    }

    #[test]
    fn computes_levels_per_second() {
        let mut pcm = Vec::new();
        for _ in 0..8000 {
            pcm.extend_from_slice(&16384i16.to_le_bytes());
        }
        pcm.extend(vec![0u8; 16000]);
        let levels = audio_levels(&pcm, 8000);
        assert_eq!(levels.len(), 2);
        assert!((levels[0] + 6.02).abs() < 0.1);
        assert_eq!(levels[1], -90.0);
    }

    #[test]
    fn picks_the_dense_and_loud_window() {
        // 60 s video; busy speech and loud audio between 30 and 45 s.
        let mut words = Vec::new();
        for t in 0..60 {
            let n = if (30..45).contains(&t) { 4 } else { 1 };
            for k in 0..n {
                words.push(word(t as f64 + k as f64 * 0.2, "word"));
            }
        }
        let levels: Vec<f64> = (0..60).map(|t| if (30..45).contains(&t) { -12.0 } else { -30.0 }).collect();
        let found = find_highlights(&words, &levels, 60.0, 15.0, 1);
        assert_eq!(found.len(), 1);
        assert!(found[0].start >= 29.0 && found[0].start <= 35.0, "{:?}", found[0]);
        assert!(found[0].end <= 46.0);
    }

    #[test]
    fn highlights_do_not_overlap_and_keywords_matter() {
        let mut words: Vec<_> = (0..120).map(|t| word(t as f64, "hello")).collect();
        words[100] = word(100.0, "This is the secret");
        let levels = vec![-25.0; 120];
        let found = find_highlights(&words, &levels, 120.0, 20.0, 3);
        assert_eq!(found.len(), 3);
        assert!(found[0].start <= 100.0 && found[0].end >= 100.0);
        for a in 0..found.len() {
            for b in a + 1..found.len() {
                assert!(found[a].end <= found[b].start || found[b].end <= found[a].start);
            }
        }
    }

    #[test]
    fn short_videos_yield_nothing() {
        assert!(find_highlights(&[], &[], 5.0, 30.0, 3).is_empty());
    }

    #[test]
    fn snaps_to_sentence_boundaries() {
        let words = vec![
            word(0.0, "a"), word(1.0, "end."), word(3.0, "Start"), word(4.0, "middle"),
            word(9.0, "done."), word(9.6, "x"), word(10.2, "y"),
        ];
        let (s, e) = snap(&words, 1.5, 10.0);
        assert_eq!(s, 3.0);
        assert!((e - 9.4).abs() < 1e-9);
    }
}
