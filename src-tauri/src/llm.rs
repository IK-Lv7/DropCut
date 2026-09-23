//! Local LLM helpers: prompt building and answer parsing for re-ranking
//! highlight candidates with a bundled llama.cpp + GGUF model. Everything runs
//! on this machine; transcript text is never logged.

use crate::transcript::TranscriptWord;

const MAX_EXCERPT_CHARS: usize = 1200;
const MAX_TITLE_CHARS: usize = 60;

/// Joins words into readable text: a space only between Latin-script words.
pub fn excerpt(words: &[TranscriptWord], start: f64, end: f64) -> String {
    let mut text = String::new();
    for word in words.iter().filter(|w| w.start >= start && w.end <= end && w.filler != Some("high")) {
        let piece = word.text.trim();
        if piece.is_empty() {
            continue;
        }
        if !text.is_empty() && piece.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            text.push(' ');
        }
        text.push_str(piece);
        if text.chars().count() >= MAX_EXCERPT_CHARS {
            break;
        }
    }
    text.chars().take(MAX_EXCERPT_CHARS).collect()
}

/// ChatML prompt (the format Qwen models are trained on).
pub fn build_prompt(excerpt: &str) -> String {
    format!(
        "<|im_start|>system\nYou judge short video clips from their transcript. \
Reply in exactly two lines and nothing else.<|im_end|>\n\
<|im_start|>user\nTranscript of a clip:\n\"\"\"\n{excerpt}\n\"\"\"\n\n\
Rate how engaging this clip is as a standalone short video, from 0 (boring, incomplete, small talk) \
to 10 (surprising, useful or emotional, and makes sense on its own). \
Then write a short title in the same language as the transcript.\n\
Format:\nSCORE: <number>\nTITLE: <title><|im_end|>\n<|im_start|>assistant\n"
    )
}

#[derive(Debug, PartialEq)]
pub struct Judgement {
    pub score: f64,
    pub title: Option<String>,
}

/// Reads `SCORE: n` / `TITLE: ...` from the model output. Returns `None` when
/// no usable score is present so the caller can fall back to the heuristic.
pub fn parse_answer(output: &str) -> Option<Judgement> {
    let output = output.replace("[end of text]", "");
    let mut score = None;
    let mut title = None;
    for line in output.lines() {
        let line = line.trim();
        let upper = line.to_uppercase();
        if let Some(rest) = upper.strip_prefix("SCORE:") {
            let digits: String = rest
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            score = digits.parse::<f64>().ok().filter(|s| (0.0..=10.0).contains(s));
        } else if line.len() >= 6 && upper.starts_with("TITLE:") {
            let cleaned = line[6..].trim().trim_matches(|c| c == '"' || c == '「' || c == '」');
            if !cleaned.is_empty() {
                title = Some(cleaned.chars().take(MAX_TITLE_CHARS).collect::<String>());
            }
        }
    }
    score.map(|score| Judgement { score, title })
}

/// Blends the model's opinion with the signal-based score (both scaled 0..1).
pub fn blend(heuristic: f64, llm_score: f64) -> f64 {
    0.4 * heuristic + 0.6 * (llm_score / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(start: f64, text: &str) -> TranscriptWord {
        TranscriptWord { start, end: start + 0.5, text: text.into(), filler: None }
    }

    #[test]
    fn joins_latin_with_spaces_and_japanese_without() {
        let words = vec![word(0.0, "Hello"), word(1.0, "world"), word(2.0, "今日は"), word(3.0, "晴れ"), word(20.0, "late")];
        assert_eq!(excerpt(&words, 0.0, 10.0), "Hello world今日は晴れ");
    }

    #[test]
    fn skips_hesitations_and_limits_length() {
        let mut words = vec![word(0.0, "um"), word(1.0, "yes")];
        words[0].filler = Some("high");
        assert_eq!(excerpt(&words, 0.0, 5.0), "yes");
        let long: Vec<_> = (0..2000).map(|i| word(i as f64 * 0.1, "abc")).collect();
        assert!(excerpt(&long, 0.0, 1e6).chars().count() <= MAX_EXCERPT_CHARS);
    }

    #[test]
    fn parses_score_and_title() {
        let out = "SCORE: 8\nTITLE: \"Why I quit\"\n";
        assert_eq!(parse_answer(out), Some(Judgement { score: 8.0, title: Some("Why I quit".into()) }));
        assert_eq!(parse_answer("score: 7.5/10").map(|j| j.score), Some(7.5));
    }

    #[test]
    fn rejects_out_of_range_or_missing_scores() {
        assert_eq!(parse_answer("SCORE: 42"), None);
        assert_eq!(parse_answer("I think it is great"), None);
    }

    #[test]
    fn prompt_contains_the_transcript_once() {
        let prompt = build_prompt("hello there");
        assert_eq!(prompt.matches("hello there").count(), 1);
        assert!(prompt.ends_with("<|im_start|>assistant\n"));
    }
}
