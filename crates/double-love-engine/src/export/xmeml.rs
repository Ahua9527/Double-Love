//! XMEML（FCP7 XML）导出器：只做 TimelineIR → XML 的映射，不重算任何剪辑时间
//! （PRD 不变量：单一 TimelineIR，Exporter 禁止重算）。
//! 结构沿用 csv2xml 验证过的方言：DOCTYPE xmeml、rate(ntsc+timebase)、
//! displayformat=NDF、pathurl = file:// + percent-encoding + xml 双层转义。

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::contracts::TimelineIR;

/// xmlEscape：与 csv2xml 完全相同的 5 个实体。
pub(crate) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// pseudoUuid：md5 hex 按 8-4-4-4-12 连字符化（与 csv2xml 同算法）。
pub(crate) fn pseudo_uuid(source: &str) -> String {
    use md5::{Digest, Md5};
    let hex = format!("{:x}", Md5::digest(source.as_bytes()));
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// pathurl 字符集：非字母数字一律 percent-encode，仅保留路径分隔符与 unreserved。
const PATHURL_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// file:// + percent-encoding + xml 双层转义。
pub(crate) fn path_url(absolute_path: &str) -> String {
    xml_escape(&format!(
        "file://{}",
        utf8_percent_encode(absolute_path, PATHURL_SET)
    ))
}

/// XML id 安全名：剔除非 [A-Za-z0-9_-]；全剔光（如纯中文）回退 uuid 短码。
pub(crate) fn safe_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        let uuid = pseudo_uuid(name).replace('-', "");
        format!("media_{}", &uuid[..8])
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn tf(value: bool) -> &'static str {
    if value { "TRUE" } else { "FALSE" }
}

/// 导出所需的最小输入：IR + 源文件事实（不读库、不算时间）。
pub struct XmemlInput<'a> {
    pub ir: &'a TimelineIR,
    /// 原媒体绝对路径（只读引用）
    pub source_path: &'a str,
    /// 原媒体文件名（带扩展名）
    pub file_name: &'a str,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub audio_sample_rate: i64,
    pub audio_channels: Option<i64>,
    pub source_tc_start_frame: Option<i64>,
    pub source_tc_is_drop_frame: bool,
}

/// TimelineIR → XMEML 文档全文。
pub fn export_xmeml(input: &XmemlInput) -> String {
    let ir = input.ir;
    let rate = ir.rate;
    let timebase = rate.timebase();
    let ntsc = tf(rate.is_ntsc());
    let safe = safe_name(&ir.name);
    let file_id = format!(
        "video_file_{}_{}",
        safe,
        pseudo_uuid(input.source_path)
            .replace('-', "")
            .to_uppercase()
    );
    let stem = input
        .file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(input.file_name);

    let mut out = String::new();
    macro_rules! line {
        ($indent:expr, $text:expr $(,)?) => {{
            out.push_str(&"  ".repeat($indent));
            out.push_str(&$text);
            out.push('\n');
        }};
    }

    out.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"4\">\n",
    );
    line!(1, format!("<sequence id=\"sequence_id_{safe}\">"));
    line!(2, "<updatebehaviour>add</updatebehaviour>");
    line!(2, format!("<name>{}</name>", xml_escape(&ir.name)));
    line!(
        2,
        format!("<duration>{}</duration>", ir.output_duration_frames)
    );
    line!(2, "<rate>");
    line!(3, format!("<ntsc>{ntsc}</ntsc>"));
    line!(3, format!("<timebase>{timebase}</timebase>"));
    line!(2, "</rate>");
    line!(2, "<timecode>");
    line!(3, "<rate>");
    line!(4, format!("<ntsc>{ntsc}</ntsc>"));
    line!(4, format!("<timebase>{timebase}</timebase>"));
    line!(3, "</rate>");
    line!(3, "<frame>0</frame>");
    line!(3, "<source>source</source>");
    line!(2, "</timecode>");
    line!(2, "<in>-1</in>");
    line!(2, "<out>-1</out>");
    line!(2, "<media>");
    line!(3, "<video>");
    line!(4, "<format>");
    line!(5, "<samplecharacteristics>");
    if let Some(width) = input.width {
        line!(6, format!("<width>{width}</width>"));
    }
    if let Some(height) = input.height {
        line!(6, format!("<height>{height}</height>"));
    }
    line!(6, "<pixelaspectratio>Square</pixelaspectratio>");
    line!(5, "</samplecharacteristics>");
    line!(4, "</format>");
    line!(4, "<track>");

    for (index, clip) in ir.clips.iter().enumerate() {
        let clip_name = format!("{stem}_{:03}", index + 1);
        let video_ref = format!("{safe}_ci_{:03}", index + 1);
        let audio_ref = format!("{safe}_ci_{:03}_a", index + 1);
        line!(5, format!("<clipitem id=\"{video_ref}\">"));
        line!(6, format!("<name>{}</name>", xml_escape(&clip_name)));
        line!(
            6,
            format!("<duration>{}</duration>", ir.source_duration_frames)
        );
        line!(6, format!("<in>{}</in>", clip.source_in_frame));
        line!(6, format!("<out>{}</out>", clip.source_out_frame));
        line!(6, format!("<start>{}</start>", clip.timeline_start_frame));
        line!(6, format!("<end>{}</end>", clip.timeline_end_frame));
        line!(6, "<pixelaspectratio>Square</pixelaspectratio>");
        if index == 0 {
            // 首个 clipitem 内嵌完整 file 定义；其余按 id 引用
            line!(6, format!("<file id=\"{file_id}\">"));
            line!(7, format!("<name>{}</name>", xml_escape(input.file_name)));
            line!(
                7,
                format!("<pathurl>{}</pathurl>", path_url(input.source_path))
            );
            line!(7, "<rate>");
            line!(8, format!("<ntsc>{ntsc}</ntsc>"));
            line!(8, format!("<timebase>{timebase}</timebase>"));
            line!(7, "</rate>");
            line!(
                7,
                format!("<duration>{}</duration>", ir.source_duration_frames)
            );
            line!(7, "<timecode>");
            line!(
                8,
                format!(
                    "<frame>{}</frame>",
                    input.source_tc_start_frame.unwrap_or(0)
                ),
            );
            line!(
                8,
                format!(
                    "<displayformat>{}</displayformat>",
                    if input.source_tc_is_drop_frame {
                        "DF"
                    } else {
                        "NDF"
                    }
                )
            );
            line!(8, "<source>source</source>");
            line!(7, "</timecode>");
            line!(7, "<media>");
            line!(8, "<video>");
            line!(9, "<samplecharacteristics>");
            if let Some(width) = input.width {
                line!(10, format!("<width>{width}</width>"));
            }
            if let Some(height) = input.height {
                line!(10, format!("<height>{height}</height>"));
            }
            line!(10, "<pixelaspectratio>Square</pixelaspectratio>");
            line!(9, "</samplecharacteristics>");
            line!(8, "</video>");
            line!(8, "<audio>");
            line!(9, "<samplecharacteristics>");
            line!(10, "<depth>16</depth>");
            line!(
                10,
                format!("<samplerate>{}</samplerate>", input.audio_sample_rate)
            );
            line!(9, "</samplecharacteristics>");
            if let Some(channels) = input.audio_channels {
                line!(9, format!("<channelcount>{channels}</channelcount>"));
            }
            line!(8, "</audio>");
            line!(7, "</media>");
            line!(6, "</file>");
        } else {
            line!(6, format!("<file id=\"{file_id}\"/>"));
        }
        line!(6, "<sourcetrack>");
        line!(7, "<mediatype>video</mediatype>");
        line!(6, "</sourcetrack>");
        line!(6, "<link>");
        line!(7, format!("<linkclipref>{video_ref}</linkclipref>"));
        line!(7, "<mediatype>video</mediatype>");
        line!(7, "<trackindex>1</trackindex>");
        line!(7, "<clipindex>1</clipindex>");
        line!(6, "</link>");
        line!(6, "<link>");
        line!(7, format!("<linkclipref>{audio_ref}</linkclipref>"));
        line!(7, "<mediatype>audio</mediatype>");
        line!(7, "<trackindex>2</trackindex>");
        line!(7, "<clipindex>1</clipindex>");
        line!(6, "</link>");
        line!(5, "</clipitem>");
    }

    line!(4, "</track>");
    line!(3, "</video>");
    line!(3, "<audio>");
    line!(4, "<outputs>");
    line!(5, "<group>");
    line!(6, "<index>1</index>");
    line!(6, "<numchannels>1</numchannels>");
    line!(6, "<downmix>0</downmix>");
    line!(6, "<channel>");
    line!(7, "<index>1</index>");
    line!(6, "</channel>");
    line!(5, "</group>");
    line!(4, "</outputs>");
    line!(4, "<in>-1</in>");
    line!(4, "<out>-1</out>");
    line!(4, "<track>");

    for (index, clip) in ir.clips.iter().enumerate() {
        let clip_name = format!("{stem}_{:03}", index + 1);
        let video_ref = format!("{safe}_ci_{:03}", index + 1);
        let audio_ref = format!("{safe}_ci_{:03}_a", index + 1);
        line!(5, format!("<clipitem id=\"{audio_ref}\">"));
        line!(6, format!("<name>{}</name>", xml_escape(&clip_name)));
        line!(
            6,
            format!("<duration>{}</duration>", ir.source_duration_frames)
        );
        line!(6, format!("<in>{}</in>", clip.source_in_frame));
        line!(6, format!("<out>{}</out>", clip.source_out_frame));
        line!(6, format!("<start>{}</start>", clip.timeline_start_frame));
        line!(6, format!("<end>{}</end>", clip.timeline_end_frame));
        line!(6, format!("<file id=\"{file_id}\"/>"));
        line!(6, "<sourcetrack>");
        line!(7, "<mediatype>audio</mediatype>");
        line!(7, "<trackindex>1</trackindex>");
        line!(6, "</sourcetrack>");
        line!(6, "<link>");
        line!(7, format!("<linkclipref>{video_ref}</linkclipref>"));
        line!(7, "<mediatype>video</mediatype>");
        line!(7, "<trackindex>1</trackindex>");
        line!(7, "<clipindex>1</clipindex>");
        line!(6, "</link>");
        line!(6, "<link>");
        line!(7, format!("<linkclipref>{audio_ref}</linkclipref>"));
        line!(7, "<mediatype>audio</mediatype>");
        line!(7, "<trackindex>2</trackindex>");
        line!(7, "<clipindex>1</clipindex>");
        line!(6, "</link>");
        line!(5, "</clipitem>");
    }

    line!(4, "</track>");
    line!(3, "</audio>");
    line!(2, "</media>");
    line!(1, "</sequence>");
    out.push_str("</xmeml>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{IrClip, MapSegment, TIMELINE_IR_SCHEMA_VERSION};
    use crate::rational::FrameRate;

    #[test]
    fn xml_escape_covers_five_entities() {
        assert_eq!(
            xml_escape("a&b\"c'd<e>f"),
            "a&amp;b&quot;c&apos;d&lt;e&gt;f"
        );
    }

    #[test]
    fn pseudo_uuid_matches_md5_reference() {
        // md5("test") = 098f6bcd4621d373cade4e832627b4f6
        assert_eq!(pseudo_uuid("test"), "098f6bcd-4621-d373-cade-4e832627b4f6");
    }

    #[test]
    fn safe_name_strips_and_falls_back() {
        assert_eq!(safe_name("ROUGH CUT 01"), "ROUGH_CUT_01");
        assert_eq!(safe_name("粗剪版"), {
            let uuid = pseudo_uuid("粗剪版").replace('-', "");
            format!("media_{}", &uuid[..8])
        });
        assert_eq!(safe_name("a/b\\c:d"), "a_b_c_d");
    }

    #[test]
    fn pathurl_percent_encodes_and_xml_escapes() {
        assert_eq!(
            path_url("/Volumes/素材盘/项目 A/带 空格.mp4"),
            "file:///Volumes/%E7%B4%A0%E6%9D%90%E7%9B%98/%E9%A1%B9%E7%9B%AE%20A/%E5%B8%A6%20%E7%A9%BA%E6%A0%BC.mp4"
        );
        assert_eq!(path_url("/tmp/a&b.mp4"), "file:///tmp/a%26b.mp4");
    }

    fn golden_ir(rate: FrameRate) -> TimelineIR {
        TimelineIR {
            schema_version: TIMELINE_IR_SCHEMA_VERSION,
            name: "ROUGH CUT".to_string(),
            rate,
            source_asset_id: "a1".to_string(),
            source_duration_frames: 250,
            output_duration_frames: 219,
            clips: vec![
                IrClip {
                    clip_index: 0,
                    source_in_frame: 0,
                    source_out_frame: 22,
                    timeline_start_frame: 0,
                    timeline_end_frame: 22,
                    first_word_ordinal: 0,
                    last_word_ordinal: 0,
                },
                IrClip {
                    clip_index: 1,
                    source_in_frame: 53,
                    source_out_frame: 250,
                    timeline_start_frame: 22,
                    timeline_end_frame: 219,
                    first_word_ordinal: 2,
                    last_word_ordinal: 2,
                },
            ],
            output_map: vec![
                MapSegment {
                    src_start_sample: 0,
                    src_end_sample: 42_240,
                    out_start_frame: 0,
                    out_end_frame: 22,
                },
                MapSegment {
                    src_start_sample: 101_760,
                    src_end_sample: 480_000,
                    out_start_frame: 22,
                    out_end_frame: 219,
                },
            ],
        }
    }

    #[test]
    fn golden_25fps_multi_clipitem_full_text() {
        let ir = golden_ir(FrameRate::Fps25);
        let xml = export_xmeml(&XmemlInput {
            ir: &ir,
            source_path: "/Volumes/素材盘/项目 A/带 空格.mp4",
            file_name: "带 空格.mp4",
            width: Some(1920),
            height: Some(1080),
            audio_sample_rate: 48_000,
            audio_channels: Some(2),
            source_tc_start_frame: None,
            source_tc_is_drop_frame: false,
        });
        let expected = include_str!("xmeml_golden_25fps.xml");
        assert_eq!(xml, expected, "golden mismatch");
    }

    /// golden 文件再生成（维护用）：`cargo test -p double-love-engine regenerate_golden -- --ignored`
    /// 重写后必须肉眼 review git diff，防止把错误固化进 golden。
    #[test]
    #[ignore]
    fn regenerate_golden_file() {
        let ir = golden_ir(FrameRate::Fps25);
        let xml = export_xmeml(&XmemlInput {
            ir: &ir,
            source_path: "/Volumes/素材盘/项目 A/带 空格.mp4",
            file_name: "带 空格.mp4",
            width: Some(1920),
            height: Some(1080),
            audio_sample_rate: 48_000,
            audio_channels: Some(2),
            source_tc_start_frame: None,
            source_tc_is_drop_frame: false,
        });
        std::fs::write(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/export/xmeml_golden_25fps.xml"
            ),
            xml,
        )
        .expect("golden rewritten");
    }

    #[test]
    fn ntsc_rates_render_timebase_and_flag() {
        for (rate, timebase, ntsc) in [
            (FrameRate::Fps24Ntsc, 24, "TRUE"),
            (FrameRate::Fps30Ntsc, 30, "TRUE"),
            (FrameRate::Fps60Ntsc, 60, "TRUE"),
            (FrameRate::Fps120Ntsc, 120, "TRUE"),
            (FrameRate::Fps120, 120, "FALSE"),
            (FrameRate::Fps25, 25, "FALSE"),
        ] {
            let ir = golden_ir(rate);
            let xml = export_xmeml(&XmemlInput {
                ir: &ir,
                source_path: "/tmp/synthetic.mp4",
                file_name: "synthetic.mp4",
                width: None,
                height: None,
                audio_sample_rate: 48_000,
                audio_channels: None,
                source_tc_start_frame: None,
                source_tc_is_drop_frame: false,
            });
            assert!(
                xml.contains(&format!(
                    "<ntsc>{ntsc}</ntsc>\n            <timebase>{timebase}</timebase>"
                )) || xml.contains(&format!("<ntsc>{ntsc}</ntsc>")),
                "ntsc flag for {rate:?}"
            );
            assert!(xml.contains(&format!("<timebase>{timebase}</timebase>")));
        }
    }
}
