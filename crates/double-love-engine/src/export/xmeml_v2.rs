//! 多素材 TimelineIR v2 → XMEML（FCP7 XML）。
//!
//! 序列始终使用项目输出帧率；每个 file 定义保留源素材自己的帧率、时间码、采样率和
//! 路径。`in/out` 在源帧率域，`start/end` 在输出帧率域，不能在这里重新计算时间。

use std::collections::{HashMap, HashSet};

use crate::contracts::{ResolvedTimelineClip, SubtitleCue, TimelineIRv2, TimelineSource};
use crate::export::xmeml::{path_url, pseudo_uuid, safe_name, tf, xml_escape};

/// 多素材 XMEML 的唯一输入。字幕是文本与时间；外观由 ASS/MP4 完整承载。
pub struct XmemlV2Input<'a> {
    pub ir: &'a TimelineIRv2,
    pub cues: &'a [SubtitleCue],
}

fn rate_xml(rate: crate::FrameRate, indent: usize, output: &mut String) {
    output.push_str(&"  ".repeat(indent));
    output.push_str("<rate>\n");
    output.push_str(&"  ".repeat(indent + 1));
    output.push_str(&format!("<ntsc>{}</ntsc>\n", tf(rate.is_ntsc())));
    output.push_str(&"  ".repeat(indent + 1));
    output.push_str(&format!("<timebase>{}</timebase>\n", rate.timebase()));
    output.push_str(&"  ".repeat(indent));
    output.push_str("</rate>\n");
}

fn file_id(source: &TimelineSource) -> String {
    format!(
        "video_file_{}_{}",
        safe_name(&source.asset_id),
        pseudo_uuid(&source.original_path)
            .replace('-', "")
            .to_uppercase()
    )
}

fn clip_label(source: &TimelineSource, clip: &ResolvedTimelineClip) -> String {
    let stem = source
        .display_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(&source.display_name);
    format!("{stem} — {}", &clip.id[..clip.id.len().min(8)])
}

fn write_file_definition(
    source: &TimelineSource,
    file_id: &str,
    indent: usize,
    output: &mut String,
) {
    macro_rules! line {
        ($text:expr) => {{
            output.push_str(&"  ".repeat(indent));
            output.push_str(&$text);
            output.push('\n');
        }};
        ($offset:expr, $text:expr) => {{
            output.push_str(&"  ".repeat(indent + $offset));
            output.push_str(&$text);
            output.push('\n');
        }};
    }
    line!(format!("<file id=\"{file_id}\">"));
    line!(
        1,
        format!("<name>{}</name>", xml_escape(&source.display_name))
    );
    line!(
        1,
        format!("<pathurl>{}</pathurl>", path_url(&source.original_path))
    );
    rate_xml(source.rate, indent + 1, output);
    line!(
        1,
        format!("<duration>{}</duration>", source.source_duration_frames)
    );
    line!(1, "<timecode>");
    rate_xml(source.rate, indent + 2, output);
    line!(
        2,
        format!(
            "<frame>{}</frame>",
            source.source_tc_start_frame.unwrap_or(0)
        )
    );
    line!(
        2,
        format!(
            "<displayformat>{}</displayformat>",
            if source.source_tc_is_drop_frame {
                "DF"
            } else {
                "NDF"
            }
        )
    );
    line!(2, "<source>source</source>");
    line!(1, "</timecode>");
    line!(1, "<media>");
    line!(2, "<video>");
    line!(3, "<samplecharacteristics>");
    if let Some(width) = source.width {
        line!(4, format!("<width>{width}</width>"));
    }
    if let Some(height) = source.height {
        line!(4, format!("<height>{height}</height>"));
    }
    line!(4, "<pixelaspectratio>Square</pixelaspectratio>");
    line!(3, "</samplecharacteristics>");
    line!(2, "</video>");
    line!(2, "<audio>");
    line!(3, "<samplecharacteristics>");
    line!(4, "<depth>16</depth>");
    line!(
        4,
        format!("<samplerate>{}</samplerate>", source.audio_sample_rate)
    );
    line!(3, "</samplecharacteristics>");
    if let Some(channels) = source.audio_channels {
        line!(3, format!("<channelcount>{channels}</channelcount>"));
    }
    line!(2, "</audio>");
    line!(1, "</media>");
    line!("</file>");
}

fn write_video_clip(
    source: &TimelineSource,
    clip: &ResolvedTimelineClip,
    sequence_safe_name: &str,
    write_file: bool,
    output: &mut String,
) {
    let source_file_id = file_id(source);
    let item_id = format!("{sequence_safe_name}_v_{}", safe_name(&clip.id));
    let audio_id = format!("{sequence_safe_name}_a_{}", safe_name(&clip.id));
    macro_rules! line {
        ($indent:expr, $text:expr) => {{
            output.push_str(&"  ".repeat($indent));
            output.push_str(&$text);
            output.push('\n');
        }};
    }
    line!(5, format!("<clipitem id=\"{item_id}\">"));
    line!(
        6,
        format!("<name>{}</name>", xml_escape(&clip_label(source, clip)))
    );
    line!(
        6,
        format!("<duration>{}</duration>", source.source_duration_frames)
    );
    line!(6, format!("<in>{}</in>", clip.source_in_frame));
    line!(6, format!("<out>{}</out>", clip.source_out_frame));
    line!(6, format!("<start>{}</start>", clip.timeline_start_frame));
    line!(6, format!("<end>{}</end>", clip.timeline_end_frame));
    line!(6, "<pixelaspectratio>Square</pixelaspectratio>");
    if write_file {
        write_file_definition(source, &source_file_id, 6, output);
    } else {
        line!(6, format!("<file id=\"{source_file_id}\"/>"));
    }
    line!(6, "<sourcetrack>");
    line!(7, "<mediatype>video</mediatype>");
    line!(6, "</sourcetrack>");
    line!(6, "<link>");
    line!(7, format!("<linkclipref>{item_id}</linkclipref>"));
    line!(7, "<mediatype>video</mediatype>");
    line!(7, "<trackindex>1</trackindex>");
    line!(6, "</link>");
    line!(6, "<link>");
    line!(7, format!("<linkclipref>{audio_id}</linkclipref>"));
    line!(7, "<mediatype>audio</mediatype>");
    line!(7, "<trackindex>1</trackindex>");
    line!(6, "</link>");
    line!(5, "</clipitem>");
}

fn write_audio_clip(
    source: &TimelineSource,
    clip: &ResolvedTimelineClip,
    sequence_safe_name: &str,
    output: &mut String,
) {
    let source_file_id = file_id(source);
    let video_id = format!("{sequence_safe_name}_v_{}", safe_name(&clip.id));
    let item_id = format!("{sequence_safe_name}_a_{}", safe_name(&clip.id));
    macro_rules! line {
        ($indent:expr, $text:expr) => {{
            output.push_str(&"  ".repeat($indent));
            output.push_str(&$text);
            output.push('\n');
        }};
    }
    line!(5, format!("<clipitem id=\"{item_id}\">"));
    line!(
        6,
        format!("<name>{}</name>", xml_escape(&clip_label(source, clip)))
    );
    line!(
        6,
        format!("<duration>{}</duration>", source.source_duration_frames)
    );
    line!(6, format!("<in>{}</in>", clip.source_in_frame));
    line!(6, format!("<out>{}</out>", clip.source_out_frame));
    line!(6, format!("<start>{}</start>", clip.timeline_start_frame));
    line!(6, format!("<end>{}</end>", clip.timeline_end_frame));
    line!(6, format!("<file id=\"{source_file_id}\"/>"));
    line!(6, "<sourcetrack>");
    line!(7, "<mediatype>audio</mediatype>");
    line!(7, "<trackindex>1</trackindex>");
    line!(6, "</sourcetrack>");
    line!(6, "<link>");
    line!(7, format!("<linkclipref>{video_id}</linkclipref>"));
    line!(7, "<mediatype>video</mediatype>");
    line!(7, "<trackindex>1</trackindex>");
    line!(6, "</link>");
    line!(6, "<link>");
    line!(7, format!("<linkclipref>{item_id}</linkclipref>"));
    line!(7, "<mediatype>audio</mediatype>");
    line!(7, "<trackindex>1</trackindex>");
    line!(6, "</link>");
    line!(5, "</clipitem>");
}

fn write_subtitle_track(cues: &[SubtitleCue], output: &mut String) {
    macro_rules! line {
        ($indent:expr, $text:expr) => {{
            output.push_str(&"  ".repeat($indent));
            output.push_str(&$text);
            output.push('\n');
        }};
    }
    line!(4, "<track>");
    line!(5, "<name>Double Love Subtitles</name>");
    for (index, cue) in cues.iter().enumerate() {
        let id = format!("double_love_subtitle_{index:04}");
        line!(5, format!("<generatoritem id=\"{id}\">"));
        line!(6, format!("<name>Subtitle {:04}</name>", index + 1));
        line!(
            6,
            format!("<duration>{}</duration>", cue.end_frame - cue.start_frame)
        );
        line!(6, "<in>0</in>");
        line!(6, format!("<out>{}</out>", cue.end_frame - cue.start_frame));
        line!(6, format!("<start>{}</start>", cue.start_frame));
        line!(6, format!("<end>{}</end>", cue.end_frame));
        line!(6, "<effect>");
        line!(7, "<name>Text</name>");
        line!(7, "<effectid>Text</effectid>");
        line!(7, "<effectcategory>Text</effectcategory>");
        line!(7, "<effecttype>generator</effecttype>");
        line!(7, "<parameter>");
        line!(8, "<parameterid>str</parameterid>");
        line!(8, format!("<value>{}</value>", xml_escape(&cue.text)));
        line!(7, "</parameter>");
        line!(6, "</effect>");
        line!(5, "</generatoritem>");
    }
    line!(4, "</track>");
}

/// 生成同一份 XMEML，供 Premiere/Resolve 的导入验收使用。
pub fn export_xmeml_v2(input: &XmemlV2Input<'_>) -> String {
    let ir = input.ir;
    let safe = safe_name(&ir.name);
    let sources: HashMap<&str, &TimelineSource> = ir
        .sources
        .iter()
        .map(|source| (source.asset_id.as_str(), source))
        .collect();
    let mut output = String::new();
    macro_rules! line {
        ($indent:expr, $text:expr) => {{
            output.push_str(&"  ".repeat($indent));
            output.push_str(&$text);
            output.push('\n');
        }};
    }
    output.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"4\">\n",
    );
    line!(1, format!("<sequence id=\"sequence_id_{safe}\">"));
    line!(2, "<updatebehaviour>add</updatebehaviour>");
    line!(2, format!("<name>{}</name>", xml_escape(&ir.name)));
    line!(
        2,
        format!("<duration>{}</duration>", ir.output_duration_frames)
    );
    rate_xml(ir.rate, 2, &mut output);
    line!(2, "<timecode>");
    rate_xml(ir.rate, 3, &mut output);
    line!(3, "<frame>0</frame>");
    line!(3, "<source>source</source>");
    line!(2, "</timecode>");
    line!(2, "<media>");
    line!(3, "<video>");
    line!(4, "<format>");
    line!(5, "<samplecharacteristics>");
    line!(6, format!("<width>{}</width>", ir.canvas.width));
    line!(6, format!("<height>{}</height>", ir.canvas.height));
    line!(6, "<pixelaspectratio>Square</pixelaspectratio>");
    line!(5, "</samplecharacteristics>");
    line!(4, "</format>");
    line!(4, "<track>");
    let mut file_definitions = HashSet::new();
    for clip in &ir.clips {
        let Some(source) = sources.get(clip.source_asset_id.as_str()) else {
            continue;
        };
        write_video_clip(
            source,
            clip,
            &safe,
            file_definitions.insert(source.asset_id.as_str()),
            &mut output,
        );
    }
    line!(4, "</track>");
    write_subtitle_track(input.cues, &mut output);
    line!(3, "</video>");
    line!(3, "<audio>");
    line!(4, "<in>-1</in>");
    line!(4, "<out>-1</out>");
    line!(4, "<track>");
    for clip in &ir.clips {
        let Some(source) = sources.get(clip.source_asset_id.as_str()) else {
            continue;
        };
        write_audio_clip(source, clip, &safe, &mut output);
    }
    line!(4, "</track>");
    line!(3, "</audio>");
    line!(2, "</media>");
    line!(1, "</sequence>");
    output.push_str("</xmeml>\n");
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{CanvasSpec, OutputMapSegment};
    use crate::rational::FrameRate;

    #[test]
    fn writes_exact_120_timebases() {
        let mut ntsc = String::new();
        super::rate_xml(FrameRate::Fps120Ntsc, 0, &mut ntsc);
        assert!(ntsc.contains("<timebase>120</timebase>"));
        assert!(ntsc.contains("<ntsc>TRUE</ntsc>"));
        let mut exact = String::new();
        super::rate_xml(FrameRate::Fps120, 0, &mut exact);
        assert!(exact.contains("<timebase>120</timebase>"));
        assert!(exact.contains("<ntsc>FALSE</ntsc>"));
    }

    fn source(id: &str, rate: FrameRate, path: &str) -> TimelineSource {
        TimelineSource {
            asset_id: id.to_string(),
            display_name: format!("{id}.mov"),
            original_path: path.to_string(),
            rate,
            source_duration_frames: 300,
            audio_sample_rate: 44_100,
            audio_channels: Some(2),
            width: Some(1920),
            height: Some(1080),
            source_tc_start_frame: Some(100),
            source_tc_is_drop_frame: rate == FrameRate::Fps30Ntsc,
        }
    }

    #[test]
    fn keeps_each_source_rate_and_writes_editable_subtitle_text() {
        let ir = TimelineIRv2 {
            schema_version: 2,
            name: "mixed sources".to_string(),
            rate: FrameRate::Fps25,
            canvas: CanvasSpec::default(),
            sources: vec![
                source("a", FrameRate::Fps25, "/tmp/a.mov"),
                source("b", FrameRate::Fps30Ntsc, "/tmp/b & two.mov"),
            ],
            source_cuts: Vec::new(),
            clips: vec![
                ResolvedTimelineClip {
                    id: "clip-a".to_string(),
                    source_asset_id: "a".to_string(),
                    source_in_frame: 25,
                    source_out_frame: 50,
                    timeline_start_frame: 0,
                    timeline_end_frame: 25,
                },
                ResolvedTimelineClip {
                    id: "clip-b".to_string(),
                    source_asset_id: "b".to_string(),
                    source_in_frame: 30,
                    source_out_frame: 60,
                    timeline_start_frame: 25,
                    timeline_end_frame: 50,
                },
            ],
            output_duration_frames: 50,
            output_map: vec![OutputMapSegment {
                source_asset_id: "a".to_string(),
                clip_id: "clip-a".to_string(),
                src_start_sample: 0,
                src_end_sample: 48_000,
                out_start_frame: 0,
                out_end_frame: 25,
            }],
        };
        let xml = export_xmeml_v2(&XmemlV2Input {
            ir: &ir,
            cues: &[SubtitleCue {
                id: "cue-1".to_string(),
                source_word_ids: vec!["w1".to_string()],
                speaker_id: None,
                speaker_name: None,
                start_frame: 4,
                end_frame: 12,
                text: "字幕 & text".to_string(),
            }],
        });
        assert_eq!(
            xml.matches("<pathurl>").count(),
            2,
            "one definition per source"
        );
        assert!(xml.contains("<timebase>25</timebase>"));
        assert!(xml.contains("<timebase>30</timebase>"));
        assert!(xml.contains("<ntsc>TRUE</ntsc>"));
        assert!(xml.contains("<displayformat>DF</displayformat>"));
        assert!(xml.contains("<in>30</in>"));
        assert!(xml.contains("<start>25</start>"));
        assert!(xml.contains("Double Love Subtitles"));
        assert!(xml.contains("字幕 &amp; text"));
        assert!(xml.contains("b%20%26%20two.mov"));
    }
}
