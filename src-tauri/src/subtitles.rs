//! Subtitle styling: turns whisper's SRT into an ASS file whose size, margins
//! and line breaks are computed for the real output resolution.
//!
//! Only timing and layout are handled here; the text is never logged.

use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleStyleSettings {
    pub template: String,
    pub font: Option<String>,
    /// Text height as a percentage of the video's shorter side.
    pub size_percent: Option<f64>,
    /// Stroke width, in pixels of a 1080p frame.
    pub outline: Option<f64>,
    /// Shadow distance, in pixels of a 1080p frame.
    pub shadow: Option<f64>,
    pub position: Option<String>,
    pub background: Option<bool>,
    /// Longest line, in full-width characters (Latin letters count as half).
    pub max_chars: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

struct Template {
    size_percent: f64,
    primary: &'static str,
    outline_colour: &'static str,
    bold: bool,
    boxed: bool,
    outline: f64,
    shadow: f64,
}

const WHITE: &str = "&H00FFFFFF";
const BLACK: &str = "&H00000000";
const BOX: &str = "&H99000000";

fn template(name: &str) -> Result<Template, String> {
    Ok(match name {
        "" | "simple" => Template {
            size_percent: 5.0,
            primary: WHITE,
            outline_colour: BLACK,
            bold: false,
            boxed: false,
            outline: 2.5,
            shadow: 0.0,
        },
        "youtube" => Template {
            size_percent: 4.6,
            primary: WHITE,
            outline_colour: BOX,
            bold: false,
            boxed: true,
            outline: 6.0,
            shadow: 0.0,
        },
        "tiktok" => Template {
            size_percent: 6.2,
            primary: WHITE,
            outline_colour: BLACK,
            bold: true,
            boxed: false,
            outline: 4.5,
            shadow: 1.0,
        },
        "gaming" => Template {
            size_percent: 6.0,
            primary: "&H0000E5FF",
            outline_colour: BLACK,
            bold: true,
            boxed: false,
            outline: 4.5,
            shadow: 2.5,
        },
        "minimal" => Template {
            size_percent: 4.2,
            primary: WHITE,
            outline_colour: BLACK,
            bold: false,
            boxed: false,
            outline: 0.0,
            shadow: 1.5,
        },
        "pop" => Template {
            size_percent: 6.4,
            primary: WHITE,
            outline_colour: "&H00B469FF",
            bold: true,
            boxed: false,
            outline: 5.5,
            shadow: 3.0,
        },
        _ => return Err("Unknown subtitle style.".into()),
    })
}

fn valid_font(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-')
}

fn timestamp(value: &str) -> Option<f64> {
    let (clock, millis) = value.trim().split_once(',').or_else(|| value.trim().split_once('.'))?;
    let mut parts = clock.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    let millis: f64 = millis.trim().parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds + millis / 1000.0)
}

pub fn parse_srt(content: &str) -> Vec<Cue> {
    let content = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in content.split("\n\n") {
        let mut lines = block.lines().filter(|line| !line.trim().is_empty());
        let mut header = lines.next();
        if header.is_some_and(|line| !line.contains("-->")) {
            header = lines.next();
        }
        let Some((start, end)) = header.and_then(|line| line.split_once("-->")) else {
            continue;
        };
        let (Some(start), Some(end)) = (timestamp(start), timestamp(end)) else {
            continue;
        };
        let text = strip_tags(&lines.collect::<Vec<_>>().join("\n"));
        if !text.trim().is_empty() && end > start {
            cues.push(Cue {
                start,
                end,
                text,
            });
        }
    }
    cues
}

fn srt_time(seconds: f64) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    )
}

pub fn to_srt(cues: &[Cue]) -> String {
    cues.iter()
        .enumerate()
        .map(|(index, cue)| {
            format!(
                "{}\n{} --> {}\n{}\n\n",
                index + 1,
                srt_time(cue.start),
                srt_time(cue.end),
                cue.text
            )
        })
        .collect()
}

fn is_japanese(character: char) -> bool {
    matches!(character, '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{9FFF}' | '\u{FF66}'..='\u{FF9F}')
}

/// Whisper's notes for non-speech, e.g. "(笑)", "[Laughter]", "*laughs*".
fn is_laughter_note(inner: &str) -> bool {
    let inner = inner.trim().to_lowercase();
    inner.contains('笑') || ["laugh", "chuckl", "giggl"].iter().any(|word| inner.contains(word))
}

fn closing_bracket(open: char) -> Option<char> {
    Some(match open {
        '(' => ')',
        '（' => '）',
        '[' => ']',
        '【' => '】',
        '*' => '*',
        _ => return None,
    })
}

/// Length in chars of a spoken Japanese laugh at the start of `chars`, such as
/// "ハハハ", "あはは" or "はっはっは". Two syllables need a leading あ/わ so
/// that words like "はは" (mother) are left alone.
fn japanese_laugh_len(chars: &[char]) -> Option<usize> {
    let mut index = 0;
    let prefixed = chars.first().is_some_and(|c| matches!(c, 'あ' | 'ア' | 'わ' | 'ワ'));
    if prefixed {
        index += 1;
    }
    let mut syllables = 0;
    let mut end = 0;
    while let Some(&character) = chars.get(index) {
        match character {
            'は' | 'ハ' | 'ひ' | 'ヒ' | 'ふ' | 'フ' | 'へ' | 'ヘ' | 'ﾊ' => {
                syllables += 1;
                end = index + 1;
            }
            'っ' | 'ッ' | 'ー' | '〜' | '～' if syllables > 0 => end = index + 1,
            _ => break,
        }
        index += 1;
    }
    (syllables >= 3 || (prefixed && syllables >= 2)).then_some(end)
}

/// "haha", "Hahaha", "ahaha", "hehe" (one written word).
fn is_english_laugh_word(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    let body = word.strip_prefix('a').unwrap_or(&word);
    let body = body.strip_suffix('h').unwrap_or(body);
    body.len() >= 4
        && body.len() % 2 == 0
        && body.as_bytes().chunks(2).all(|pair| matches!(pair, b"ha" | b"he" | b"hi"))
}

/// The written laugh for subtitles: "www" for Japanese, "lol" for English.
/// With "auto" the script of the existing cues decides.
pub fn laugh_word(language: &str, cues: &[Cue]) -> &'static str {
    let japanese = match language {
        "ja" => true,
        "en" => false,
        _ => cues.iter().any(|cue| cue.text.chars().any(is_japanese)),
    };
    if japanese { "www" } else { "lol" }
}

/// True when `text` contains laughter (a note such as "(笑)" or a spoken laugh).
pub fn has_laughter(text: &str) -> bool {
    laughter_to_text(text, "en") != text
}

/// Replaces laughter in a cue with the internet-style laugh viewers expect:
/// "www" for Japanese and "lol" for English. `language` is the transcription
/// language; with "auto" the cue's own script decides.
pub fn laughter_to_text(text: &str, language: &str) -> String {
    const MARK: char = '\u{E000}';
    let japanese = match language {
        "ja" => true,
        "en" => false,
        _ => text.chars().any(is_japanese),
    };
    let chars: Vec<char> = text.chars().collect();
    let mut marked = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        if let Some(close) = closing_bracket(character) {
            if let Some(length) = chars[index + 1..].iter().position(|&c| c == close) {
                let inner: String = chars[index + 1..index + 1 + length].iter().collect();
                if is_laughter_note(&inner) {
                    marked.push(MARK);
                    index += length + 2;
                    continue;
                }
            }
        }
        if let Some(length) = japanese_laugh_len(&chars[index..]) {
            marked.push(MARK);
            index += length;
            continue;
        }
        let starts_word = index == 0 || !chars[index - 1].is_ascii_alphanumeric();
        if character.is_ascii_alphabetic() && starts_word {
            let length = chars[index..].iter().take_while(|c| c.is_ascii_alphabetic()).count();
            let word: String = chars[index..index + length].iter().collect();
            // A lone "ha" is laughter only when another laugh word follows.
            let repeated_ha = word.eq_ignore_ascii_case("ha") && {
                let rest = &chars[index + length..];
                let skip = rest.iter().take_while(|c| matches!(c, ' ' | ',')).count();
                let next: String = rest[skip..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
                let follows_laugh = marked.trim_end_matches([' ', ',']).ends_with(MARK)
                    && marked.ends_with([' ', ',']);
                follows_laugh
                    || skip > 0 && (next.eq_ignore_ascii_case("ha") || is_english_laugh_word(&next))
            };
            if repeated_ha || is_english_laugh_word(&word) {
                marked.push(MARK);
                index += length;
                continue;
            }
        }
        marked.push(character);
        index += 1;
    }
    if !marked.contains(MARK) {
        return text.to_string();
    }
    // Merge laughs separated only by spaces or commas ("Ha ha, haha").
    let mut merged = String::with_capacity(marked.len());
    let mut pending = String::new();
    for character in marked.chars() {
        if merged.ends_with(MARK) && matches!(character, ' ' | ',' | '、' | '，' | '　') {
            pending.push(character);
            continue;
        }
        if character != MARK || !merged.ends_with(MARK) {
            merged.push_str(&pending);
            merged.push(character);
        }
        pending.clear();
    }
    let laugh = if japanese { "www" } else { "lol" };
    let mut output = String::with_capacity(merged.len());
    let mut characters = merged.chars().peekable();
    while let Some(character) = characters.next() {
        if character != MARK {
            output.push(character);
            continue;
        }
        if japanese {
            // "面白い (笑)。" reads as "面白いwww".
            while output.ends_with([' ', '　']) {
                output.pop();
            }
            while characters.peek().is_some_and(|c| matches!(c, '。' | '、' | '.' | ',' | '，' | '．')) {
                characters.next();
            }
        } else if output.chars().last().is_some_and(|c| c.is_alphanumeric()) {
            output.push(' ');
        }
        output.push_str(laugh);
        if !japanese && characters.peek().is_some_and(|c| c.is_alphanumeric()) {
            output.push(' ');
        }
    }
    output.trim().to_string()
}

fn strip_tags(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
}

fn char_width(character: char) -> f64 {
    if character.is_ascii() {
        0.5
    } else {
        1.0
    }
}

fn cannot_start_line(character: char) -> bool {
    "、。，．,.!?！？）)」』】〉》ー・…:;".contains(character)
}

/// Breaks `text` so no line is wider than `max_width` full-width characters,
/// preferring spaces and never starting a line with closing punctuation.
pub fn wrap_text(text: &str, max_width: f64) -> Vec<String> {
    let mut flattened = String::new();
    for character in text.trim().chars() {
        if character == '\n' {
            let previous_ascii = flattened.chars().last().is_none_or(|c| c.is_ascii());
            if previous_ascii {
                flattened.push(' ');
            }
        } else {
            flattened.push(character);
        }
    }
    let mut lines = Vec::new();
    let mut line: Vec<char> = Vec::new();
    let mut width = 0.0;
    for character in flattened.chars() {
        if character == ' ' && line.is_empty() {
            continue;
        }
        let next_width = width + char_width(character);
        if next_width > max_width && character != ' ' && !cannot_start_line(character) {
            match line.iter().rposition(|c| *c == ' ') {
                Some(space) if space > 0 => {
                    let tail: Vec<char> = line.split_off(space + 1);
                    while line.last() == Some(&' ') {
                        line.pop();
                    }
                    lines.push(line.iter().collect());
                    line = tail;
                    width = line.iter().map(|c| char_width(*c)).sum();
                }
                _ => {
                    while line.last() == Some(&' ') {
                        line.pop();
                    }
                    lines.push(line.iter().collect());
                    line.clear();
                    width = 0.0;
                }
            }
        }
        line.push(character);
        width += char_width(character);
    }
    while line.last() == Some(&' ') {
        line.pop();
    }
    if !line.is_empty() {
        lines.push(line.iter().collect());
    }
    lines
}

fn ass_time(seconds: f64) -> String {
    let centiseconds = (seconds.max(0.0) * 100.0).round() as u64;
    format!(
        "{}:{:02}:{:02}.{:02}",
        centiseconds / 360_000,
        centiseconds / 6000 % 60,
        centiseconds / 100 % 60,
        centiseconds % 100
    )
}

fn ass_text(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| line.replace(['{', '}'], "").replace('\\', ""))
        .collect::<Vec<_>>()
        .join("\\N")
}

pub fn build_ass(
    cues: &[Cue],
    settings: &SubtitleStyleSettings,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let template = template(&settings.template)?;
    let font = match settings.font.as_deref() {
        None | Some("") => "Arial",
        Some(name) if valid_font(name) => name,
        Some(_) => return Err("The subtitle font name is not valid.".into()),
    };
    let alignment_margin = |position: &str, portrait: bool| -> Result<(u32, f64), String> {
        match position {
            "bottom" => Ok((2, if portrait { 0.18 } else { 0.08 })),
            "middle" => Ok((5, 0.0)),
            "top" => Ok((8, 0.08)),
            _ => Err("Unknown subtitle position.".into()),
        }
    };
    let portrait = height > width;
    let (alignment, margin_ratio) =
        alignment_margin(settings.position.as_deref().unwrap_or("bottom"), portrait)?;
    let shorter = f64::from(width.min(height));
    let scale = shorter / 1080.0;
    let size = settings.size_percent.unwrap_or(template.size_percent);
    let outline = settings.outline.unwrap_or(template.outline);
    let shadow = settings.shadow.unwrap_or(template.shadow);
    let max_chars = settings
        .max_chars
        .unwrap_or(if portrait { 13.0 } else { 26.0 });
    if !(2.0..=12.0).contains(&size)
        || !(0.0..=10.0).contains(&outline)
        || !(0.0..=8.0).contains(&shadow)
        || !(6.0..=60.0).contains(&max_chars)
    {
        return Err("A subtitle style value is out of range.".into());
    }
    let boxed = settings.background.unwrap_or(template.boxed);
    let (border_style, outline_colour, back_colour, outline) = if boxed {
        (3, BOX, BOX, if outline < 3.0 { 6.0 } else { outline })
    } else {
        (1, template.outline_colour, "&H80000000", outline)
    };
    let mut ass = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {width}\nPlayResY: {height}\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n\
[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,{font},{:.1},{},&H000000FF,{outline_colour},{back_colour},{},0,0,0,100,100,0,0,{border_style},{:.1},{:.1},{alignment},{},{},{},1\n\n\
[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        size / 100.0 * shorter,
        template.primary,
        if template.bold { -1 } else { 0 },
        outline * scale,
        shadow * scale,
        (f64::from(width) * 0.05).round(),
        (f64::from(width) * 0.05).round(),
        (f64::from(height) * margin_ratio).round(),
    );
    for cue in cues {
        let lines = wrap_text(&cue.text, max_chars);
        if lines.is_empty() {
            continue;
        }
        ass.push_str(&format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,{}\n",
            ass_time(cue.start),
            ass_time(cue.end),
            ass_text(&lines)
        ));
    }
    Ok(ass)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "\u{feff}1\r\n00:00:01,000 --> 00:00:03,500\r\n<i>Hello</i> there\r\n\r\n2\r\n00:01:02,250 --> 00:01:04,000\r\n今日は\r\n暑いです\r\n\r\nbroken\r\n";

    #[test]
    fn japanese_laughter_becomes_www() {
        for (input, expected) in [
            ("面白い (笑)", "面白いwww"),
            ("（笑い声）", "www"),
            ("ハハハ、それはないでしょ", "wwwそれはないでしょ"),
            ("あははっ。本当に？", "www本当に？"),
            ("はっはっは", "www"),
            ("[Laughter]", "www"),
        ] {
            assert_eq!(laughter_to_text(input, "ja"), expected, "{input}");
        }
    }

    #[test]
    fn english_laughter_becomes_lol() {
        for (input, expected) in [
            ("That's funny (laughs)", "That's funny lol"),
            ("[LAUGHTER]", "lol"),
            ("Hahaha, no way", "lol, no way"),
            ("Ha ha ha that was great", "lol that was great"),
            ("Ha, I see", "Ha, I see"),
            ("haha haha", "lol"),
            ("*chuckles* okay", "lol okay"),
        ] {
            assert_eq!(laughter_to_text(input, "en"), expected, "{input}");
        }
    }

    #[test]
    fn laughter_follows_the_cue_script_in_auto_and_spares_real_words() {
        assert_eq!(laughter_to_text("やばい(笑)", "auto"), "やばいwww");
        assert_eq!(laughter_to_text("(laughing) Oh no", "auto"), "lol Oh no");
        for text in ["はは", "母は元気です", "Hahn said hello", "(applause)", "Hi there"] {
            assert_eq!(laughter_to_text(text, "auto"), text, "{text}");
        }
    }

    #[test]
    fn written_srt_parses_back() {
        let cues = vec![
            Cue { start: 0.5, end: 2.25, text: "Hello".into() },
            Cue { start: 3661.0, end: 3662.5, text: "今日は".into() },
        ];
        let srt = to_srt(&cues);
        assert!(srt.contains("01:01:01,000 --> 01:01:02,500"));
        assert_eq!(parse_srt(&srt), cues);
    }

    #[test]
    fn parses_srt_with_tags_bom_and_crlf() {
        let cues = parse_srt(SRT);
        assert_eq!(cues.len(), 2);
        assert_eq!((cues[0].start, cues[0].end), (1.0, 3.5));
        assert_eq!(cues[0].text, "Hello there");
        assert_eq!(cues[1].start, 62.25);
        assert_eq!(cues[1].text, "今日は\n暑いです");
    }

    #[test]
    fn wraps_japanese_without_leading_punctuation() {
        let lines = wrap_text("今日は東京に行ってきました。めちゃくちゃ暑かったです。", 10.0);
        assert!(lines.len() >= 3);
        assert!(lines.iter().all(|line| line.chars().count() <= 11));
        assert!(lines.iter().all(|line| !line.starts_with('。')));
        assert_eq!(lines.concat(), "今日は東京に行ってきました。めちゃくちゃ暑かったです。");
    }

    #[test]
    fn wraps_english_at_spaces() {
        let lines = wrap_text("Ask not what your country can do for you", 10.0);
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|line| !line.starts_with(' ') && !line.ends_with(' ')));
        assert_eq!(lines.join(" "), "Ask not what your country can do for you");
        assert!(lines.iter().all(|line| line.chars().count() as f64 * 0.5 <= 10.0));
    }

    #[test]
    fn joins_existing_line_breaks_sensibly() {
        assert_eq!(wrap_text("今日は\n暑い", 20.0), vec!["今日は暑い"]);
        assert_eq!(wrap_text("hello\nworld", 20.0), vec!["hello world"]);
        assert!(wrap_text("   ", 10.0).is_empty());
    }

    #[test]
    fn formats_ass_times() {
        assert_eq!(ass_time(0.0), "0:00:00.00");
        assert_eq!(ass_time(62.25), "0:01:02.25");
        assert_eq!(ass_time(3723.5), "1:02:03.50");
    }

    #[test]
    fn builds_a_scaled_style_for_the_output_resolution() {
        let cues = parse_srt(SRT);
        let settings = SubtitleStyleSettings {
            template: "tiktok".into(),
            ..Default::default()
        };
        let ass = build_ass(&cues, &settings, 1080, 1920).unwrap();
        assert!(ass.contains("PlayResX: 1080\nPlayResY: 1920"));
        assert!(ass.contains("Style: Default,Arial,67.0,&H00FFFFFF,"));
        assert!(ass.contains(",-1,0,0,0,100,100,0,0,1,4.5,1.0,2,54,54,346,1"));
        assert!(ass.contains("Dialogue: 0,0:00:01.00,0:00:03.50,Default,,0,0,0,,Hello there"));
        let landscape = build_ass(&cues, &settings, 1920, 1080).unwrap();
        assert!(landscape.contains("Style: Default,Arial,67.0,"));
        assert!(landscape.contains(",2,96,96,86,1"));
    }

    #[test]
    fn user_overrides_win_over_the_template() {
        let settings = SubtitleStyleSettings {
            template: "simple".into(),
            font: Some("Yu Gothic".into()),
            size_percent: Some(8.0),
            outline: Some(0.0),
            position: Some("top".into()),
            background: Some(true),
            max_chars: Some(20.0),
            ..Default::default()
        };
        let ass = build_ass(&parse_srt(SRT), &settings, 1920, 1080).unwrap();
        assert!(ass.contains("Style: Default,Yu Gothic,86.4,"));
        assert!(ass.contains(",3,6.0,0.0,8,"));
    }

    #[test]
    fn rejects_bad_styles() {
        let build = |settings: SubtitleStyleSettings| build_ass(&[], &settings, 1920, 1080);
        assert!(build(SubtitleStyleSettings {
            template: "nope".into(),
            ..Default::default()
        })
        .is_err());
        assert!(build(SubtitleStyleSettings {
            font: Some("Arial,Bold=1".into()),
            ..Default::default()
        })
        .is_err());
        assert!(build(SubtitleStyleSettings {
            size_percent: Some(40.0),
            ..Default::default()
        })
        .is_err());
        assert!(build(SubtitleStyleSettings {
            position: Some("left".into()),
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn dialogue_text_cannot_inject_override_tags() {
        let cues = [Cue {
            start: 0.0,
            end: 1.0,
            text: "{\\an8}hi\\N".into(),
        }];
        let ass = build_ass(&cues, &SubtitleStyleSettings::default(), 1920, 1080).unwrap();
        let dialogue = ass.lines().find(|line| line.starts_with("Dialogue")).unwrap();
        assert!(dialogue.ends_with(",,an8hiN"));
        assert!(!dialogue.contains('{') && !dialogue.contains('\\'));
    }

    /// Writes one ASS file per template for eyeballing with a libass FFmpeg:
    /// `DROPCUT_TEST_OUT=/some/dir cargo test writes_sample -- --ignored`
    #[test]
    #[ignore]
    fn writes_sample_subtitles() {
        let dir = std::path::PathBuf::from(std::env::var("DROPCUT_TEST_OUT").unwrap());
        let cues = parse_srt(
            "1\n00:00:00,000 --> 00:00:04,000\n今日は東京に行ってきました。めちゃくちゃ暑かったです。\n\n2\n00:00:04,000 --> 00:00:08,000\nAsk not what your country can do for you\n",
        );
        for name in ["simple", "youtube", "tiktok", "gaming", "minimal", "pop"] {
            for (label, width, height) in [("land", 1920, 1080), ("port", 1080, 1920)] {
                let settings = SubtitleStyleSettings {
                    template: name.into(),
                    font: Some("Noto Sans JP".into()),
                    ..Default::default()
                };
                let ass = build_ass(&cues, &settings, width, height).unwrap();
                std::fs::write(dir.join(format!("{name}-{label}.ass")), ass).unwrap();
            }
        }
    }
}
