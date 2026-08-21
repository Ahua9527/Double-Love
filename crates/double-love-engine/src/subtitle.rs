//! 字幕投影：WordAnchor + Output Time Map → Cue，再由同一项目级样式生成 ASS。

use std::collections::HashMap;

use uuid::Uuid;

use crate::contracts::{SubtitleCue, SubtitleStyle, TimelineIRv2, WordAnchor};

#[derive(Debug, Clone)]
struct TimedWord {
    word_id: String,
    text: String,
    speaker_id: Option<String>,
    start_frame: i64,
    end_frame: i64,
}

fn map_frame(
    segment_start: i64,
    segment_end: i64,
    source_start: i64,
    source_end: i64,
    source: i64,
    ceil: bool,
) -> i64 {
    let source_len = (source_end - source_start) as i128;
    let output_len = (segment_end - segment_start) as i128;
    let offset = (source - source_start) as i128;
    let numerator = offset * output_len;
    let base = numerator.div_euclid(source_len);
    let rounded = if ceil && numerator.rem_euclid(source_len) != 0 {
        base + 1
    } else {
        base
    };
    segment_start + rounded as i64
}

fn text_needs_space(previous: &str, next: &str) -> bool {
    previous
        .chars()
        .last()
        .zip(next.chars().next())
        .is_some_and(|(left, right)| left.is_ascii_alphanumeric() && right.is_ascii_alphanumeric())
}

fn terminal_punctuation(text: &str) -> bool {
    text.chars()
        .last()
        .is_some_and(|character| matches!(character, '。' | '！' | '？' | '.' | '!' | '?'))
}

fn wrap_cue_text(text: &str, target_characters: usize, max_lines: usize) -> String {
    if target_characters == 0 || max_lines <= 1 || text.chars().count() <= target_characters {
        return text.to_string();
    }
    let characters = text.chars().collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut start = 0;
    while start < characters.len() && lines.len() + 1 < max_lines {
        let tentative = (start + target_characters).min(characters.len());
        let break_at = (start..tentative)
            .rev()
            .find(|index| {
                matches!(
                    characters[*index],
                    '，' | '。' | '！' | '？' | ',' | '.' | '!' | '?' | ' '
                )
            })
            .map(|index| index + 1)
            .filter(|index| *index > start)
            .unwrap_or(tentative);
        lines.push(characters[start..break_at].iter().collect::<String>());
        start = break_at;
    }
    if start < characters.len() {
        lines.push(characters[start..].iter().collect::<String>());
    }
    lines.join("\n")
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF)
}

fn add_cjk_spacing(text: &str) -> String {
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len() + 8);
    for (index, character) in characters.iter().enumerate() {
        if let Some(previous) = index.checked_sub(1).and_then(|value| characters.get(value)) {
            let boundary = (is_cjk(*previous) && character.is_ascii_alphanumeric())
                || (previous.is_ascii_alphanumeric() && is_cjk(*character));
            if boundary && !output.ends_with(' ') {
                output.push(' ');
            }
        }
        output.push(*character);
    }
    output
}

fn complete_cue(mut cue: SubtitleCue, style: &SubtitleStyle) -> SubtitleCue {
    if style.cjk_spacing {
        cue.text = add_cjk_spacing(&cue.text);
    }
    cue.text = wrap_cue_text(
        &cue.text,
        style.target_characters_per_line.max(1) as usize,
        style.max_lines.max(1) as usize,
    );
    cue
}

/// 将全部源素材的词投影到输出时间。相同源素材在主轨重复出现时，词会在每个片段中各投影一次。
pub fn build_subtitle_cues(
    timeline: &TimelineIRv2,
    source_words: &HashMap<String, Vec<WordAnchor>>,
    style: &SubtitleStyle,
) -> Vec<SubtitleCue> {
    let mut timed = Vec::new();
    for map in &timeline.output_map {
        let Some(words) = source_words.get(&map.source_asset_id) else {
            continue;
        };
        for word in words {
            let start = word.start_sample.max(map.src_start_sample);
            let end = word.end_sample.min(map.src_end_sample);
            if end <= start {
                continue;
            }
            let start_frame = map_frame(
                map.out_start_frame,
                map.out_end_frame,
                map.src_start_sample,
                map.src_end_sample,
                start,
                false,
            );
            let end_frame = map_frame(
                map.out_start_frame,
                map.out_end_frame,
                map.src_start_sample,
                map.src_end_sample,
                end,
                true,
            )
            .max(start_frame + 1);
            timed.push(TimedWord {
                word_id: word.word_id.clone(),
                text: word.display_text.clone(),
                speaker_id: word
                    .speaker_assignments
                    .first()
                    .map(|item| item.speaker_id.clone()),
                start_frame,
                end_frame,
            });
        }
    }
    timed.sort_by_key(|word| (word.start_frame, word.end_frame, word.word_id.clone()));

    let target = style.target_characters_per_line.max(1) as usize * style.max_lines.max(1) as usize;
    let mut cues = Vec::new();
    let mut current: Option<SubtitleCue> = None;
    for word in timed {
        let needs_break = current.as_ref().is_some_and(|cue| {
            cue.speaker_id != word.speaker_id
                || cue.text.chars().count() >= target
                || terminal_punctuation(&cue.text)
        });
        if needs_break {
            cues.push(complete_cue(
                current.take().expect("current cue exists"),
                style,
            ));
        }
        match current.as_mut() {
            Some(cue) => {
                if text_needs_space(&cue.text, &word.text) {
                    cue.text.push(' ');
                }
                cue.text.push_str(&word.text);
                cue.end_frame = cue.end_frame.max(word.end_frame);
                cue.source_word_ids.push(word.word_id);
            }
            None => {
                current = Some(SubtitleCue {
                    id: Uuid::new_v4().to_string(),
                    source_word_ids: vec![word.word_id],
                    speaker_id: word.speaker_id,
                    speaker_name: None,
                    start_frame: word.start_frame,
                    end_frame: word.end_frame,
                    text: word.text,
                });
            }
        }
    }
    if let Some(cue) = current {
        cues.push(complete_cue(cue, style));
    }
    cues
}

/// 将匿名说话人 ID 投影为项目级展示名。身份映射是局部元数据，不改写词或时间。
pub fn apply_speaker_names(cues: &mut [SubtitleCue], speaker_names: &HashMap<String, String>) {
    for cue in cues {
        cue.speaker_name = cue
            .speaker_id
            .as_ref()
            .and_then(|speaker_id| speaker_names.get(speaker_id).cloned());
    }
}

fn parse_css_color(value: &str) -> (u8, u8, u8, u8) {
    let value = value.trim().trim_start_matches('#');
    let hex = match value.len() {
        6 => format!("{value}FF"),
        8 => value.to_string(),
        _ => "FFFFFFFF".to_string(),
    };
    let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).unwrap_or(255);
    (parse(0..2), parse(2..4), parse(4..6), parse(6..8))
}

fn ass_color(value: &str) -> String {
    let (red, green, blue, alpha) = parse_css_color(value);
    // ASS alpha 与 CSS 相反：00 为不透明，FF 为透明。
    format!(
        "&H{:02X}{blue:02X}{green:02X}{red:02X}",
        255_u8.saturating_sub(alpha)
    )
}

fn ass_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('\n', "\\N")
}

fn ass_time(frame: i64, timeline: &TimelineIRv2) -> String {
    let rate = timeline.rate.rational();
    let centiseconds = (frame as i128 * rate.den as i128 * 100).div_euclid(rate.num as i128) as i64;
    let hours = centiseconds / 360_000;
    let minutes = (centiseconds / 6_000) % 60;
    let seconds = (centiseconds / 100) % 60;
    let fraction = centiseconds % 100;
    format!("{hours}:{minutes:02}:{seconds:02}.{fraction:02}")
}

/// ASS 是字幕样式的完整交付格式；NLE 只消费 Cue 的文字和时间。
pub fn export_ass(timeline: &TimelineIRv2, style: &SubtitleStyle, cues: &[SubtitleCue]) -> String {
    let margin_v = ((1.0 - style.position_y.clamp(0.0, 1.0)) * timeline.canvas.height as f64)
        .round()
        .max(0.0) as i64;
    let margin_lr = ((1.0 - style.max_width_ratio.clamp(0.1, 1.0)) * timeline.canvas.width as f64
        / 2.0)
        .round()
        .max(0.0) as i64;
    let alignment = 2; // bottom centre
    let (_, _, _, background_alpha) = parse_css_color(&style.background_color);
    let border_style = if background_alpha > 0 { 3 } else { 1 };
    let back_color = if background_alpha > 0 {
        ass_color(&style.background_color)
    } else {
        ass_color(&style.shadow_color)
    };
    let position_x =
        (style.position_x.clamp(0.0, 1.0) * timeline.canvas.width as f64).round() as i64;
    let position_y =
        (style.position_y.clamp(0.0, 1.0) * timeline.canvas.height as f64).round() as i64;
    let mut output = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {}\nPlayResY: {}\n\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: DoubleLove,{},{:.1},{},{},{},{},{},0,0,0,100,100,0,0,{},{:.1},{:.1},{},{},{},{},1\n\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\n",
        timeline.canvas.width,
        timeline.canvas.height,
        style.font_family.replace(',', " "),
        style.font_size,
        ass_color(&style.text_color),
        ass_color(&style.text_color),
        ass_color(&style.outline_color),
        back_color,
        if style.font_weight >= 600 { -1 } else { 0 },
        border_style,
        style.outline_width,
        (style.shadow_blur.max(style.shadow_offset_y.abs()) / 2.0).max(0.0),
        alignment,
        margin_lr,
        margin_lr,
        margin_v,
    );
    for cue in cues {
        let speaker = if style.show_speaker {
            cue.speaker_name
                .as_deref()
                .or(cue.speaker_id.as_deref())
                .map(|speaker| format!("{speaker}："))
                .unwrap_or_default()
        } else {
            String::new()
        };
        output.push_str(&format!(
            "Dialogue: 0,{},{},DoubleLove,,0,0,0,,{{\\pos({position_x},{position_y})}}{}{}\n",
            ass_time(cue.start_frame, timeline),
            ass_time(cue.end_frame, timeline),
            ass_escape(&speaker),
            ass_escape(&cue.text),
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{CanvasSpec, OutputMapSegment, TimelineSource};
    use crate::rational::FrameRate;

    fn word(id: &str, text: &str, start: i64, end: i64, speaker: &str) -> WordAnchor {
        WordAnchor {
            word_id: id.to_string(),
            asset_id: "a".to_string(),
            ordinal: 0,
            raw_text: text.to_string(),
            display_text: text.to_string(),
            language: Some("zh".to_string()),
            start_sample: start,
            end_sample: end,
            confidence: Some(0.99),
            synthetic: false,
            source_word_ids: None,
            speaker_assignments: vec![crate::contracts::SpeakerAssignment {
                speaker_id: speaker.to_string(),
                confidence: Some(0.9),
                evidence: "diarization".to_string(),
            }],
        }
    }

    fn timeline() -> TimelineIRv2 {
        TimelineIRv2 {
            schema_version: 2,
            name: "test".to_string(),
            rate: FrameRate::Fps25,
            canvas: CanvasSpec::default(),
            sources: vec![TimelineSource {
                asset_id: "a".to_string(),
                display_name: "a.mov".to_string(),
                original_path: "/tmp/a.mov".to_string(),
                rate: FrameRate::Fps25,
                source_duration_frames: 250,
                audio_sample_rate: 48_000,
                audio_channels: Some(2),
                width: Some(1920),
                height: Some(1080),
                source_tc_start_frame: Some(0),
                source_tc_is_drop_frame: false,
            }],
            source_cuts: Vec::new(),
            clips: vec![],
            output_duration_frames: 250,
            output_map: vec![OutputMapSegment {
                source_asset_id: "a".to_string(),
                clip_id: "clip-a".to_string(),
                src_start_sample: 0,
                src_end_sample: 480_000,
                out_start_frame: 0,
                out_end_frame: 250,
            }],
        }
    }

    #[test]
    fn cue_engine_breaks_on_speaker_change_and_punctuation() {
        let mut words = HashMap::new();
        words.insert(
            "a".to_string(),
            vec![
                word("w1", "你好，", 0, 24_000, "s1"),
                word("w2", "大家好。", 24_000, 48_000, "s1"),
                word("w3", "我是小王。", 48_000, 96_000, "s2"),
            ],
        );
        let cues = build_subtitle_cues(&timeline(), &words, &SubtitleStyle::default());
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].speaker_id.as_deref(), Some("s1"));
        assert_eq!(cues[0].text, "你好，大家好。");
        assert_eq!(cues[1].speaker_id.as_deref(), Some("s2"));
    }

    #[test]
    fn ass_contains_project_style_and_output_time() {
        let cues = vec![SubtitleCue {
            id: "c1".to_string(),
            source_word_ids: vec!["w1".to_string()],
            speaker_id: Some("访谈者".to_string()),
            speaker_name: Some("访谈者".to_string()),
            start_frame: 25,
            end_frame: 50,
            text: "你好".to_string(),
        }];
        let mut style = SubtitleStyle::default();
        style.show_speaker = true;
        let ass = export_ass(&timeline(), &style, &cues);
        assert!(ass.contains("PlayResX: 1920"));
        assert!(ass.contains("0:00:01.00"));
        assert!(ass.contains("访谈者：你好"));
        assert!(ass.contains("{\\pos(960,950)}"));
    }

    #[test]
    fn wraps_cues_to_the_project_line_target() {
        let mut style = SubtitleStyle::default();
        style.target_characters_per_line = 4;
        style.max_lines = 2;
        assert_eq!(
            wrap_cue_text("这是一段需要换行的字幕", 4, 2),
            "这是一段\n需要换行的字幕"
        );
        let mut words = HashMap::new();
        words.insert(
            "a".to_string(),
            vec![
                word("w1", "这是一段", 0, 24_000, "s1"),
                word("w2", "需要换行", 24_000, 48_000, "s1"),
            ],
        );
        assert!(
            build_subtitle_cues(&timeline(), &words, &style)[0]
                .text
                .contains('\n')
        );
    }

    #[test]
    fn cjk_spacing_only_separates_cjk_and_latin_boundaries() {
        assert_eq!(add_cjk_spacing("今天用Qwen3测试"), "今天用 Qwen3 测试");
        assert_eq!(add_cjk_spacing("纯中文内容"), "纯中文内容");
        assert_eq!(add_cjk_spacing("two words"), "two words");
    }
}
