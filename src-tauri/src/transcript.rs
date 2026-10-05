//! Word-level transcripts from whisper.cpp, filler-word detection, and the
//! range arithmetic that turns removed words into video cuts.
//!
//! Transcript text is returned to the UI only; it is never logged.

use serde::Serialize;

use crate::subtitles::Cue;
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
                // Punctuation is often stamped at the next speech; it must
                // not stretch the word across the pause.
                if !is_punctuation_only(&text) {
                    last.end = last.end.max(piece.end);
                }
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

/// One speech span that whisper.cpp kept with `--vad`: where it starts in the
/// original audio and in the condensed audio whisper actually transcribed.
#[derive(Clone, Debug, PartialEq)]
pub struct VadSegment {
    pub orig_start: f64,
    pub orig_end: f64,
    pub vad_start: f64,
}

/// Silence whisper.cpp inserts between condensed speech spans.
const VAD_GAP_SECONDS: f64 = 0.1;

/// Reads the `vad_segment_info` lines whisper-cli writes to stderr.
pub fn parse_vad_segments(log: &str) -> Vec<VadSegment> {
    let mut segments: Vec<VadSegment> = log
        .lines()
        .filter_map(|line| {
            let info = line.split_once("vad_segment_info:")?.1;
            let value = |key: &str| -> Option<f64> {
                let rest = info.split_once(key)?.1;
                rest.split(|c: char| c == ',' || c.is_whitespace())
                    .find(|part| !part.is_empty())?
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
            };
            Some(VadSegment {
                orig_start: value("orig_start:")?,
                orig_end: value("orig_end:")?,
                vad_start: value("vad_start:")?,
            })
        })
        .collect();
    segments.sort_by(|a, b| a.vad_start.total_cmp(&b.vad_start));
    segments
}

/// whisper.cpp 1.9 maps segment times back from condensed audio by
/// interpolating across the 0.1 s gaps, so a time inside a gap lands anywhere
/// in the real pause (the next line shows up early), and token times are not
/// mapped at all. Inside a span the offset is exact; in a gap a start snaps
/// forward to the next speech and an end back to the previous one.
fn map_vad_time(time: f64, segments: &[VadSegment], is_start: bool) -> f64 {
    let Some(index) = segments.iter().rposition(|segment| segment.vad_start <= time) else {
        return segments.first().map_or(time, |segment| segment.orig_start);
    };
    let segment = &segments[index];
    let offset = time - segment.vad_start;
    let Some(next) = segments.get(index + 1) else {
        return segment.orig_start + offset;
    };
    let span = (next.vad_start - VAD_GAP_SECONDS - segment.vad_start).max(0.0);
    if offset < span {
        (segment.orig_start + offset).min(next.orig_start)
    } else if is_start {
        next.orig_start
    } else {
        (segment.orig_start + span).min(next.orig_start)
    }
}

/// Reads token pieces per whisper segment, with times on the original timeline.
fn parse_segments(raw: &[u8], vad: &[VadSegment]) -> Result<Vec<Vec<Piece>>, String> {
    let document: serde_json::Value = serde_json::from_str(&escape_invalid_utf8(raw))
        .map_err(|_| "The transcript could not be read.".to_string())?;
    let segments = document["transcription"]
        .as_array()
        .ok_or_else(|| "The transcript could not be read.".to_string())?;
    let mut result = Vec::with_capacity(segments.len());
    let mut has_dtw = true;
    for segment in segments {
        let mut pieces = Vec::new();
        for token in segment["tokens"].as_array().into_iter().flatten() {
            let Some(text) = token["text"].as_str() else {
                continue;
            };
            if is_special_token(text) {
                continue;
            }
            let from = token["offsets"]["from"].as_f64().unwrap_or(0.0) / 1000.0;
            let to = token["offsets"]["to"].as_f64().unwrap_or(from * 1000.0) / 1000.0;
            // `t_dtw` (centiseconds, -1 when absent) is only written with `--dtw`.
            match token["t_dtw"].as_f64().filter(|value| *value >= 0.0) {
                Some(dtw) => {
                    let start = dtw / 100.0;
                    pieces.push(Piece {
                        start,
                        end: start + (to - from).max(0.0),
                        bytes: token_bytes(text),
                    });
                }
                None => {
                    has_dtw = false;
                    pieces.push(Piece {
                        start: from,
                        end: to.max(from),
                        bytes: token_bytes(text),
                    });
                }
            }
        }
        result.push(pieces);
    }
    has_dtw &= result.iter().any(|pieces| !pieces.is_empty());
    // A whisper build that already maps token times would report times past
    // the condensed audio; leave those alone instead of mapping twice.
    let condensed_end = vad
        .last()
        .map(|last| last.vad_start + (last.orig_end - last.orig_start) + 1.0);
    let already_mapped = condensed_end.is_some_and(|limit| {
        result.iter().flatten().any(|piece| piece.end > limit)
    });
    if !vad.is_empty() && !already_mapped {
        for piece in result.iter_mut().flatten() {
            piece.start = map_vad_time(piece.start, vad, true);
            piece.end = map_vad_time(piece.end, vad, false).max(piece.start);
        }
    }
    if has_dtw {
        end_dtw_pieces(&mut result, vad);
    }
    Ok(result)
}

/// DTW gives each token only its start. A token lasts until the next spoken
/// token starts, but not past the end of its VAD speech span, so pauses stay
/// visible. Without VAD, whisper's own token length caps it instead.
fn end_dtw_pieces(segments: &mut [Vec<Piece>], vad: &[VadSegment]) {
    let mut next_start = f64::INFINITY;
    for piece in segments.iter_mut().rev().flat_map(|pieces| pieces.iter_mut().rev()) {
        let cap = if vad.is_empty() {
            piece.end
        } else {
            vad.iter()
                .rev()
                .find(|segment| segment.orig_start <= piece.start + 1e-9)
                .map_or(piece.end, |segment| segment.orig_end)
        };
        piece.end = next_start.min(cap).max(piece.start);
        if !std::str::from_utf8(&piece.bytes).is_ok_and(is_punctuation_only) {
            next_start = piece.start;
        }
    }
}

pub fn parse_whisper_json(raw: &[u8], vad: &[VadSegment]) -> Result<Vec<TranscriptWord>, String> {
    let pieces = parse_segments(raw, vad)?.into_iter().flatten().collect();
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

/// A pause this long inside one whisper segment starts a new subtitle, so the
/// words after it are not shown before they are spoken.
const CUE_GAP_SECONDS: f64 = 0.8;
/// Short cues stay up at least this long when the next cue allows it.
const MIN_CUE_SECONDS: f64 = 0.8;
/// Token end times run slightly early, so cues linger briefly after speech.
const CUE_HOLD_SECONDS: f64 = 0.3;
/// How far a cue start may move back to the speech onset before it.
const ONSET_SNAP_SECONDS: f64 = 0.6;

/// Builds subtitle cues timed by the spoken tokens rather than whisper's
/// segment bounds, which often cover the silence before speech starts.
pub fn subtitle_cues(raw: &[u8], vad: &[VadSegment]) -> Result<Vec<Cue>, String> {
    let mut cues = Vec::new();
    for pieces in parse_segments(raw, vad)? {
        let mut bytes: Vec<u8> = Vec::new();
        let mut start = 0.0;
        let mut end = 0.0;
        for piece in pieces {
            let punctuation = std::str::from_utf8(&piece.bytes).is_ok_and(is_punctuation_only);
            let at_boundary = std::str::from_utf8(&bytes).is_ok();
            if !bytes.is_empty() && at_boundary && !punctuation && piece.start - end > CUE_GAP_SECONDS {
                push_cue(&mut cues, &bytes, start, end);
                bytes.clear();
            }
            if bytes.is_empty() {
                start = piece.start;
                end = piece.end;
            }
            bytes.extend_from_slice(&piece.bytes);
            if !punctuation {
                end = f64::max(end, piece.end);
            }
        }
        push_cue(&mut cues, &bytes, start, end);
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    snap_to_speech_onsets(&mut cues, vad);
    for index in 0..cues.len() {
        let next_start = cues.get(index + 1).map_or(f64::INFINITY, |next| next.start);
        let cue = &mut cues[index];
        cue.end = (cue.end + CUE_HOLD_SECONDS)
            .max(cue.start + MIN_CUE_SECONDS)
            .min(next_start);
    }
    cues.retain(|cue| cue.end > cue.start);
    Ok(cues)
}

/// Token times (DTW ones especially) land a little after the voice begins.
/// A cue that starts shortly after a VAD speech onset starts on it instead,
/// unless the previous cue is still speaking there.
fn snap_to_speech_onsets(cues: &mut [Cue], vad: &[VadSegment]) {
    let mut previous_end = f64::NEG_INFINITY;
    for cue in cues.iter_mut() {
        if let Some(onset) = vad
            .iter()
            .map(|segment| segment.orig_start)
            .filter(|onset| {
                *onset <= cue.start && cue.start - onset <= ONSET_SNAP_SECONDS && *onset >= previous_end
            })
            .reduce(f64::max)
        {
            cue.start = onset;
        }
        previous_end = cue.end;
    }
}

fn push_cue(cues: &mut Vec<Cue>, bytes: &[u8], start: f64, end: f64) {
    let text = String::from_utf8_lossy(bytes).trim().to_string();
    if !text.is_empty() && !is_punctuation_only(&text) {
        cues.push(Cue { start, end, text });
    }
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
        let words = parse_whisper_json(&raw, &[]).unwrap();
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
        let words = parse_whisper_json(&raw, &[]).unwrap();
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
        let words = parse_whisper_json(&raw, &[]).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "猫");
        assert_eq!((words[0].start, words[0].end), (0.0, 0.4));
    }

    const VAD_LOG: &str = "\
whisper_vad: detected 2 speech segments
whisper_vad: vad_segment_info: orig_start: 0.29, orig_end: 2.24, vad_start: 0.00, vad_end: 1.95
whisper_vad: vad_segment_info: orig_start: 10.08, orig_end: 12.48, vad_start: 2.15, vad_end: 4.55
";

    #[test]
    fn reads_vad_spans_from_the_whisper_log() {
        let segments = parse_vad_segments(VAD_LOG);
        assert_eq!(segments.len(), 2);
        assert_eq!(
            segments[1],
            VadSegment { orig_start: 10.08, orig_end: 12.48, vad_start: 2.15 }
        );
        assert!(parse_vad_segments("no vad here").is_empty());
    }

    #[test]
    fn maps_condensed_times_and_snaps_gap_times_to_speech() {
        let segments = parse_vad_segments(VAD_LOG);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // Inside a span: plain offset.
        assert!(close(map_vad_time(1.0, &segments, true), 1.29));
        assert!(close(map_vad_time(3.0, &segments, true), 10.93));
        // In the inserted gap: starts wait for the next speech, ends stay put.
        assert!(close(map_vad_time(2.1, &segments, true), 10.08));
        assert!(close(map_vad_time(2.1, &segments, false), 2.34));
    }

    #[test]
    fn subtitle_cues_follow_speech_not_segment_bounds() {
        let raw = br#"{"transcription":[
            {"offsets":{"from":0,"to":13430},"tokens":[
                {"text":"[_BEG_]","offsets":{"from":0,"to":0}},
                {"text":" Hello","offsets":{"from":100,"to":900}},
                {"text":" there","offsets":{"from":900,"to":1800}},
                {"text":".","offsets":{"from":1800,"to":1900}},
                {"text":" Next","offsets":{"from":2120,"to":2600}},
                {"text":" line","offsets":{"from":2600,"to":3000}}
            ]}]}"#;
        let cues = subtitle_cues(raw, &parse_vad_segments(VAD_LOG)).unwrap();
        let texts: Vec<&str> = cues.iter().map(|cue| cue.text.as_str()).collect();
        assert_eq!(texts, ["Hello there.", "Next line"]);
        // 0.39 is just after the speech onset at 0.29, so it snaps to it.
        assert!((cues[0].start - 0.29).abs() < 1e-9);
        assert!(cues[0].end <= 2.34 + CUE_HOLD_SECONDS + 1e-9);
        assert!((cues[1].start - 10.08).abs() < 1e-9);
    }

    #[test]
    fn dtw_times_place_words_where_they_are_spoken() {
        // Plain offsets put " Next" inside the first span; DTW puts it in the second.
        let raw = br#"{"transcription":[
            {"offsets":{"from":0,"to":4550},"tokens":[
                {"text":" Hello","offsets":{"from":0,"to":600},"t_dtw":30},
                {"text":" there","offsets":{"from":600,"to":1200},"t_dtw":90},
                {"text":".","offsets":{"from":1200,"to":1300},"t_dtw":180},
                {"text":" Next","offsets":{"from":1300,"to":1700},"t_dtw":240},
                {"text":" line","offsets":{"from":1700,"to":2300},"t_dtw":300}
            ]}]}"#;
        let cues = subtitle_cues(raw, &parse_vad_segments(VAD_LOG)).unwrap();
        let texts: Vec<&str> = cues.iter().map(|cue| cue.text.as_str()).collect();
        assert_eq!(texts, ["Hello there.", "Next line"]);
        assert!((cues[0].start - 0.29).abs() < 1e-9);
        // The first cue ends with its speech span, not when "Next" starts.
        assert!((cues[0].end - (2.24 + CUE_HOLD_SECONDS)).abs() < 1e-9);
        // DTW said 10.33; the onset at 10.08 is close enough to snap to.
        assert!((cues[1].start - 10.08).abs() < 1e-9);
    }

    #[test]
    fn onset_snap_never_reaches_into_the_previous_cue() {
        let vad = [VadSegment { orig_start: 1.0, orig_end: 5.0, vad_start: 0.0 }];
        let mut cues = vec![
            Cue { start: 1.2, end: 2.0, text: "a".into() },
            Cue { start: 2.1, end: 3.0, text: "b".into() },
        ];
        snap_to_speech_onsets(&mut cues, &vad);
        assert_eq!((cues[0].start, cues[1].start), (1.0, 2.1));
    }

    #[test]
    fn transcript_words_use_the_original_timeline_with_vad() {
        let raw = document(vec![token(" one", 100, 500), token(" two", 2200, 2600)]);
        let words = parse_whisper_json(&raw, &parse_vad_segments(VAD_LOG)).unwrap();
        assert!((words[0].start - 0.39).abs() < 1e-9);
        assert!((words[1].start - 10.13).abs() < 1e-9);
    }

    #[test]
    fn rejects_documents_without_a_transcript() {
        assert!(parse_whisper_json(b"{}", &[]).is_err());
        assert!(parse_whisper_json(b"not json", &[]).is_err());
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

    /// Manual check against real whisper-cli output (`-ojf`, stderr saved as the log):
    /// `DROPCUT_TEST_WHISPER_JSON=out.json DROPCUT_TEST_WHISPER_LOG=err.log cargo test parses_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn parses_real_whisper_output() {
        let raw = std::fs::read(std::env::var("DROPCUT_TEST_WHISPER_JSON").unwrap()).unwrap();
        let log = std::env::var("DROPCUT_TEST_WHISPER_LOG")
            .map(|path| std::fs::read_to_string(path).unwrap())
            .unwrap_or_default();
        let vad = parse_vad_segments(&log);
        for cue in subtitle_cues(&raw, &vad).unwrap() {
            println!("cue {:6.2}-{:6.2} {}", cue.start, cue.end, cue.text);
        }
        let words = parse_whisper_json(&raw, &vad).unwrap();
        for word in &words {
            println!("{:6.2}-{:6.2} {}", word.start, word.end, word.text);
        }
        assert!(words.len() > 5);
        assert!(words.windows(2).all(|pair| pair[0].end <= pair[1].start + 1e-9));
    }
}
