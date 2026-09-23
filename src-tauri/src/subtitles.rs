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
