//! Word-level transcripts from whisper.cpp, filler-word detection, and the
//! range arithmetic that turns removed words into video cuts.
//!
//! Transcript text is returned to the UI only; it is never logged.

use serde::Serialize;

use crate::TimeRange;

const MIN_PIECE_SECONDS: f64 = 0.05;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptWord {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// `"high"` for hesitations that are almost never content, `"low"` for
    /// words that are often real speech ("その", "like") and need review.
    pub filler: Option<&'static str>,
}

/// JSON written by whisper-cli may contain a UTF-8 character split across two
/// tokens, which is invalid on its own. Invalid bytes are mapped into a private
/// use range so the document parses, then restored when tokens are joined.
const RAW_BYTE_BASE: u32 = 0xF700;

fn escape_invalid_utf8(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                output.push_str(valid);
                return output;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                output.push_str(std::str::from_utf8(valid).unwrap_or_default());
                let invalid = error.error_len().unwrap_or(after.len());
                for byte in &after[..invalid] {
                    output.push(char::from_u32(RAW_BYTE_BASE + u32::from(*byte)).unwrap_or('\u{FFFD}'));
                }
                rest = &after[invalid..];
            }
        }
    }
}

fn token_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len());
    for character in text.chars() {
        let code = character as u32;
        if (RAW_BYTE_BASE..RAW_BYTE_BASE + 256).contains(&code) {
            bytes.push((code - RAW_BYTE_BASE) as u8);
        } else {
            let mut buffer = [0_u8; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        }
    }
    bytes
}

struct Piece {
    start: f64,
    end: f64,
    bytes: Vec<u8>,
}

fn is_special_token(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with("[_") && trimmed.ends_with(']')) || trimmed.starts_with("<|")
}

fn is_punctuation_only(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.chars().all(|c| !c.is_alphanumeric() && c != 'ー')
}

/// Joins whisper tokens into words: English words start at a leading space,
/// Japanese tokens stand alone, and punctuation sticks to the word before it.
fn tokens_to_words(pieces: Vec<Piece>) -> Vec<TranscriptWord> {
    let mut words: Vec<TranscriptWord> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut pending_start = 0.0;
    for piece in pieces {
        if pending.is_empty() {
            pending_start = piece.start;
        }
        pending.extend_from_slice(&piece.bytes);
        let Ok(text) = std::str::from_utf8(&pending) else {
            if pending.len() < 8 {
                continue;
            }
            pending.clear();
            continue;
        };
        let text = text.to_string();
        pending.clear();
        let starts_word = text.starts_with(char::is_whitespace)
            || text.chars().next().is_some_and(|c| !c.is_ascii());
        let attaches = words.last().is_some_and(|last| {
            is_punctuation_only(&text)
                || (!starts_word
                    && last
                        .text
                        .chars()
                        .last()
                        .is_some_and(|c| c.is_ascii_alphanumeric()))
        });
        let start = pending_start;
        if attaches {
            if let Some(last) = words.last_mut() {
                last.text.push_str(text.trim_end());
                last.end = last.end.max(piece.end);
            }
        } else if !text.trim().is_empty() {
            words.push(TranscriptWord {
                start,
                end: piece.end.max(start),
                text: text.trim().to_string(),
                filler: None,
            });
        }
    }
    words
}

pub fn parse_whisper_json(raw: &[u8]) -> Result<Vec<TranscriptWord>, String> {
    let document: serde_json::Value = serde_json::from_str(&escape_invalid_utf8(raw))
        .map_err(|_| "The transcript could not be read.".to_string())?;
    let segments = document["transcription"]
        .as_array()
        .ok_or_else(|| "The transcript could not be read.".to_string())?;
    let mut pieces = Vec::new();
    for segment in segments {
        for token in segment["tokens"].as_array().into_iter().flatten() {
            let Some(text) = token["text"].as_str() else {
                continue;
            };
            if is_special_token(text) {
                continue;
            }
            let from = token["offsets"]["from"].as_f64().unwrap_or(0.0) / 1000.0;
            let to = token["offsets"]["to"].as_f64().unwrap_or(from * 1000.0) / 1000.0;
            pieces.push(Piece {
                start: from,
                end: to.max(from),
                bytes: token_bytes(text),
            });
        }
    }
    let mut words = tokens_to_words(pieces);
    // Whisper sometimes reports a token ending after the next one starts.
    for index in 1..words.len() {
        let next_start = words[index].start;
        if words[index - 1].end > next_start {
            words[index - 1].end = next_start.max(words[index - 1].start);
        }
    }
    mark_fillers(&mut words);
    Ok(words)
}

const HIGH_CONFIDENCE_FILLERS: [&str; 19] = [
    "えー", "えーと", "えっと", "えーっと", "えと", "ええと", "あー", "あーの", "あのー", "あのう",
    "そのー", "うーん", "んー", "うー", "まー", "um", "umm", "uh", "uhh",
];
const LOW_CONFIDENCE_FILLERS: [&str; 6] = ["あの", "その", "まあ", "you know", "like", "i mean"];

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || *c == 'ー')
        .flat_map(char::to_lowercase)
        .collect()
}

fn classify(candidate: &str) -> Option<&'static str> {
    if HIGH_CONFIDENCE_FILLERS.contains(&candidate) {
        Some("high")
    } else if LOW_CONFIDENCE_FILLERS.contains(&candidate) {
        Some("low")
    } else {
        None
    }
}

fn mark_fillers(words: &mut [TranscriptWord]) {
    let normalized: Vec<String> = words.iter().map(|word| normalize(&word.text)).collect();
    let mut index = 0;
    while index < words.len() {
        let mut matched = None;
        for span in (1..=3).rev() {
            if index + span > words.len() {
                continue;
            }
            let slice = &normalized[index..index + span];
            let joined = slice.concat();
            let spaced = slice.join(" ");
            if let Some(level) = classify(&joined).or_else(|| classify(&spaced)) {
                matched = Some((span, level));
                break;
            }
        }
        match matched {
            Some((span, level)) => {
                for word in &mut words[index..index + span] {
                    word.filler = Some(level);
                }
                index += span;
            }
            None => index += 1,
        }
    }
}

/// Validates ranges chosen in the UI and merges overlaps.
pub fn normalize_cut_ranges(cuts: &[TimeRange], duration: f64) -> Result<Vec<TimeRange>, String> {
    if cuts.len() > 5000 {
        return Err("Too many sections were selected for removal.".into());
    }
    let mut ranges = Vec::with_capacity(cuts.len());
    for cut in cuts {
        if !cut.start.is_finite() || !cut.end.is_finite() || cut.start < 0.0 || cut.end <= cut.start
        {
            return Err("A section selected for removal is invalid.".into());
        }
        let start = cut.start.min(duration);
        let end = cut.end.min(duration);
        if end > start {
            ranges.push(TimeRange { start, end });
        }
    }
    ranges.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut merged: Vec<TimeRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    Ok(merged)
}

/// Removes `cuts` from what is being kept. An empty `keep` means the whole
/// video. Returns an empty list when nothing needs to be trimmed.
pub fn subtract_cuts(
    keep: &[TimeRange],
    cuts: &[TimeRange],
    duration: f64,
) -> Result<Vec<TimeRange>, String> {
    if cuts.is_empty() {
        return Ok(keep.to_vec());
    }
    let whole = [TimeRange {
        start: 0.0,
        end: duration,
    }];
    let base = if keep.is_empty() { &whole[..] } else { keep };
    let mut result = Vec::with_capacity(base.len() + cuts.len());
    for range in base {
        let mut cursor = range.start;
        for cut in cuts {
            if cut.end <= cursor || cut.start >= range.end {
                continue;
            }
            if cut.start - cursor >= MIN_PIECE_SECONDS {
                result.push(TimeRange {
                    start: cursor,
                    end: cut.start,
                });
            }
            cursor = cursor.max(cut.end);
        }
        if range.end - cursor >= MIN_PIECE_SECONDS {
            result.push(TimeRange {
                start: cursor,
                end: range.end,
            });
        }
    }
    if result.is_empty() {
        return Err("Everything would be removed. Keep at least part of the video.".into());
    }
    let untouched = result.len() == 1
        && keep.is_empty()
        && result[0].start <= 0.0
        && (duration - result[0].end).abs() < 1e-6;
    Ok(if untouched { Vec::new() } else { result })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: f64, end: f64) -> TimeRange {
        TimeRange { start, end }
    }

    fn word(text: &str, start: f64, end: f64) -> TranscriptWord {
        TranscriptWord {
            start,
            end,
            text: text.into(),
            filler: None,
        }
    }

    fn token(text: &str, from: u32, to: u32) -> serde_json::Value {
        serde_json::json!({"text": text, "offsets": {"from": from, "to": to}})
    }

    fn document(tokens: Vec<serde_json::Value>) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"transcription": [{"tokens": tokens}]})).unwrap()
    }

    #[test]
    fn groups_english_tokens_into_words_and_skips_special_tokens() {
        let raw = document(vec![
            token("[_BEG_]", 0, 0),
            token(" Ame", 100, 300),
            token("ricans", 300, 600),
            token(",", 600, 600),
            token(" um", 800, 1000),
            token("[_TT_50]", 1000, 1000),
        ]);
        let words = parse_whisper_json(&raw).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "Americans,");
        assert_eq!((words[0].start, words[0].end), (0.1, 0.6));
        assert_eq!(words[1].text, "um");
        assert_eq!(words[1].filler, Some("high"));
    }

    #[test]
    fn keeps_japanese_tokens_separate_and_attaches_punctuation() {
        let raw = document(vec![
            token("今日", 0, 400),
            token("は", 400, 500),
            token("暑", 500, 800),
            token("い", 800, 900),
            token("。", 900, 900),
        ]);
        let words = parse_whisper_json(&raw).unwrap();
        let texts: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
        assert_eq!(texts, ["今日", "は", "暑", "い。"]);
    }

    #[test]
    fn restores_characters_split_across_tokens() {
        // "猫" is E7 8C AB; whisper can emit it as two invalid halves.
        let mut raw = Vec::new();
        raw.extend_from_slice(
            br#"{"transcription":[{"tokens":[{"text":""#,
        );
        raw.extend_from_slice(&[0xE7, 0x8C]);
        raw.extend_from_slice(br#"","offsets":{"from":0,"to":200}},{"text":""#);
        raw.push(0xAB);
        raw.extend_from_slice(br#"","offsets":{"from":200,"to":400}}]}]}"#);
        let words = parse_whisper_json(&raw).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "猫");
        assert_eq!((words[0].start, words[0].end), (0.0, 0.4));
    }

    #[test]
    fn rejects_documents_without_a_transcript() {
        assert!(parse_whisper_json(b"{}").is_err());
        assert!(parse_whisper_json(b"not json").is_err());
    }

    #[test]
    fn detects_japanese_fillers_across_tokens() {
        let mut words = vec![
            word("えー", 0.0, 0.4),
            word("っと", 0.4, 0.6),
            word("今日", 0.6, 1.0),
            word("あの", 1.0, 1.2),
            word("うーん", 1.2, 1.6),
        ];
        mark_fillers(&mut words);
        assert_eq!(words[0].filler, Some("high"));
        assert_eq!(words[1].filler, Some("high"));
        assert_eq!(words[2].filler, None);
        assert_eq!(words[3].filler, Some("low"));
        assert_eq!(words[4].filler, Some("high"));
    }

    #[test]
    fn detects_multi_word_english_fillers() {
        let mut words = vec![
            word("So,", 0.0, 0.2),
            word("you", 0.2, 0.4),
            word("know,", 0.4, 0.6),
            word("Uh", 0.6, 0.8),
            word("music", 0.8, 1.2),
        ];
        mark_fillers(&mut words);
        assert_eq!(words[0].filler, None);
        assert_eq!(words[1].filler, Some("low"));
        assert_eq!(words[2].filler, Some("low"));
        assert_eq!(words[3].filler, Some("high"));
        assert_eq!(words[4].filler, None);
    }

    #[test]
    fn merges_and_validates_cut_ranges() {
        let merged = normalize_cut_ranges(
            &[range(5.0, 6.0), range(1.0, 2.0), range(1.5, 3.0), range(9.0, 20.0)],
            10.0,
        )
        .unwrap();
        assert_eq!(merged, vec![range(1.0, 3.0), range(5.0, 6.0), range(9.0, 10.0)]);
        assert!(normalize_cut_ranges(&[range(3.0, 2.0)], 10.0).is_err());
        assert!(normalize_cut_ranges(&[range(-1.0, 2.0)], 10.0).is_err());
        assert!(normalize_cut_ranges(&[range(f64::NAN, 2.0)], 10.0).is_err());
    }

    #[test]
    fn cuts_split_the_whole_video() {
        let kept = subtract_cuts(&[], &[range(2.0, 3.0), range(5.0, 6.0)], 10.0).unwrap();
        assert_eq!(kept, vec![range(0.0, 2.0), range(3.0, 5.0), range(6.0, 10.0)]);
    }

    #[test]
    fn cuts_combine_with_silence_removal() {
        let kept = subtract_cuts(
            &[range(0.0, 4.0), range(6.0, 10.0)],
            &[range(3.0, 7.0)],
            10.0,
        )
        .unwrap();
        assert_eq!(kept, vec![range(0.0, 3.0), range(7.0, 10.0)]);
    }

    #[test]
    fn cuts_can_remove_the_ends_and_drop_slivers() {
        let kept = subtract_cuts(&[], &[range(0.0, 1.0), range(9.0, 10.0)], 10.0).unwrap();
        assert_eq!(kept, vec![range(1.0, 9.0)]);
        let kept = subtract_cuts(&[], &[range(0.02, 5.0)], 10.0).unwrap();
        assert_eq!(kept, vec![range(5.0, 10.0)]);
    }

    #[test]
    fn removing_everything_is_an_error_and_no_cuts_changes_nothing() {
        assert!(subtract_cuts(&[], &[range(0.0, 10.0)], 10.0).is_err());
        assert_eq!(
            subtract_cuts(&[range(1.0, 2.0)], &[], 10.0).unwrap(),
            vec![range(1.0, 2.0)]
        );
        assert!(subtract_cuts(&[], &[], 10.0).unwrap().is_empty());
    }

    /// Manual check against real whisper-cli output (`-ojf`):
    /// `DROPCUT_TEST_WHISPER_JSON=out.json cargo test parses_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn parses_real_whisper_output() {
        let raw = std::fs::read(std::env::var("DROPCUT_TEST_WHISPER_JSON").unwrap()).unwrap();
        let words = parse_whisper_json(&raw).unwrap();
        for word in &words {
            println!("{:6.2}-{:6.2} {}", word.start, word.end, word.text);
        }
        assert!(words.len() > 5);
        assert!(words.windows(2).all(|pair| pair[0].end <= pair[1].start + 1e-9));
    }
}
