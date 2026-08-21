//! TimelineIR v2 → 一次 ffmpeg 渲染。视频、音频、画布和 ASS 都消费同一份已编译时间线。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::contracts::{CanvasFit, ProjectExportPreview, TimelineSource};
use crate::storage::{ProjectStore, StorageError};
use crate::{FfmpegTools, OperationResult, OutputArtifact, export_ass, frame_to_samples};

use super::project::preview_project_export;

fn storage_failure<T>(error: StorageError) -> OperationResult<T> {
    OperationResult::failed("STORAGE_ERROR", error.to_string())
}

/// ASS 烧录依赖 libass。不同系统的 ffmpeg 编译选项不同，必须在渲染前明确检查，
/// 而不是把“找不到 filter”留给用户从长 stderr 中猜。
pub fn ffmpeg_supports_ass_filter(tools: &FfmpegTools) -> bool {
    Command::new(&tools.ffmpeg)
        .args(["-hide_banner", "-filters"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            text.lines()
                .any(|line| line.split_whitespace().any(|token| token == "ass"))
        })
        .unwrap_or(false)
}

fn rate_expression(rate: crate::FrameRate) -> String {
    let rational = rate.rational();
    format!("{}/{}", rational.num, rational.den)
}

fn ffmpeg_color(value: &str) -> String {
    let hex = value.trim().trim_start_matches('#');
    let rgb = match hex.len() {
        6 | 8 if hex.chars().all(|character| character.is_ascii_hexdigit()) => &hex[..6],
        _ => "000000",
    };
    format!("0x{rgb}")
}

fn escape_filter_filename(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn validate_canvas(canvas: &crate::CanvasSpec) -> Result<(), String> {
    if canvas.width < 2 || canvas.height < 2 {
        return Err("画布宽高至少为 2 像素。".to_string());
    }
    if !canvas.scale.is_finite() || canvas.scale <= 0.0 {
        return Err("画布缩放必须是大于 0 的有限数值。".to_string());
    }
    if !canvas.opacity.is_finite() || !(0.0..=1.0).contains(&canvas.opacity) {
        return Err("画布不透明度必须在 0 到 1 之间。".to_string());
    }
    if !canvas.rotation_degrees.is_finite()
        || !canvas.position_x.is_finite()
        || !canvas.position_y.is_finite()
    {
        return Err("画布位置和旋转必须是有限数值。".to_string());
    }
    Ok(())
}

fn source_for<'a>(
    sources: &'a HashMap<&str, &'a TimelineSource>,
    asset_id: &str,
) -> Result<&'a TimelineSource, String> {
    sources
        .get(asset_id)
        .copied()
        .ok_or_else(|| format!("时间线引用了不存在的源素材：{asset_id}"))
}

/// 构造可审计的 filter graph。所有边界都来自 TimelineIR 的源/输出帧，而非浮点秒。
pub fn ffmpeg_filter_graph(
    preview: &ProjectExportPreview,
    input_indices: &HashMap<String, usize>,
    ass_path: &Path,
) -> Result<String, String> {
    let timeline = &preview.timeline;
    validate_canvas(&timeline.canvas)?;
    if timeline.clips.is_empty() {
        return Err("主轨没有可渲染的片段。".to_string());
    }
    let sources: HashMap<&str, &TimelineSource> = timeline
        .sources
        .iter()
        .map(|source| (source.asset_id.as_str(), source))
        .collect();
    let width = timeline.canvas.width;
    let height = timeline.canvas.height;
    let scaled_width = (width as f64 * timeline.canvas.scale).round().max(2.0) as i64;
    let scaled_height = (height as f64 * timeline.canvas.scale).round().max(2.0) as i64;
    let aspect_mode = match timeline.canvas.fit {
        CanvasFit::Contain => "decrease",
        CanvasFit::Cover => "increase",
    };
    let mut filters = Vec::new();
    let mut concat_inputs = String::new();
    for (index, clip) in timeline.clips.iter().enumerate() {
        let source = source_for(&sources, &clip.source_asset_id)?;
        let input_index = input_indices
            .get(&source.asset_id)
            .copied()
            .ok_or_else(|| format!("渲染输入缺少素材：{}", source.display_name))?;
        let output_frames = clip.timeline_end_frame - clip.timeline_start_frame;
        if output_frames <= 0 {
            return Err(format!("片段 {} 的输出时长无效。", clip.id));
        }
        let source_start_sample =
            frame_to_samples(clip.source_in_frame, source.rate, source.audio_sample_rate);
        let source_end_sample =
            frame_to_samples(clip.source_out_frame, source.rate, source.audio_sample_rate);
        let output_samples = frame_to_samples(output_frames, timeline.rate, 48_000);
        let rotation = if timeline.canvas.rotation_degrees.abs() > f64::EPSILON {
            format!(
                ",rotate={:.8}*PI/180:ow=rotw(iw):oh=roth(ih)",
                timeline.canvas.rotation_degrees
            )
        } else {
            String::new()
        };
        let alpha = if (timeline.canvas.opacity - 1.0).abs() > f64::EPSILON {
            format!(
                ",format=rgba,colorchannelmixer=aa={:.6}",
                timeline.canvas.opacity
            )
        } else {
            ",format=rgba".to_string()
        };
        filters.push(format!(
            "[{input_index}:v]trim=start_frame={}:end_frame={},setpts=PTS-STARTPTS,fps=fps={},trim=end_frame={}{}{},scale={}:{}:force_original_aspect_ratio={}[fg{index}]",
            clip.source_in_frame,
            clip.source_out_frame,
            rate_expression(timeline.rate),
            output_frames,
            rotation,
            alpha,
            scaled_width,
            scaled_height,
            aspect_mode,
        ));
        filters.push(format!(
            "color=c={}:s={}x{}:r={},trim=end_frame={},setpts=PTS-STARTPTS[bg{index}]",
            ffmpeg_color(&timeline.canvas.background),
            width,
            height,
            rate_expression(timeline.rate),
            output_frames,
        ));
        filters.push(format!(
            "[bg{index}][fg{index}]overlay=x='(main_w-overlay_w)/2+{:.6}*main_w':y='(main_h-overlay_h)/2+{:.6}*main_h':shortest=1[v{index}]",
            timeline.canvas.position_x,
            timeline.canvas.position_y,
        ));
        filters.push(format!(
            "[{input_index}:a]atrim=start_sample={source_start_sample}:end_sample={source_end_sample},asetpts=N/SR/TB,aresample=48000,apad,atrim=end_sample={output_samples}[a{index}]"
        ));
        concat_inputs.push_str(&format!("[v{index}][a{index}]"));
    }
    filters.push(format!(
        "{concat_inputs}concat=n={}:v=1:a=1[vconcat][aconcat]",
        timeline.clips.len()
    ));
    filters.push(format!(
        "[vconcat]ass=filename='{}'[vout]",
        escape_filter_filename(ass_path)
    ));
    filters.push("[aconcat]aresample=48000[aout]".to_string());
    Ok(filters.join(";"))
}

fn cache_ass_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(format!("subtitles-{}.ass", Uuid::new_v4()))
}

fn temporary_mp4_path(target: &Path) -> Result<PathBuf, String> {
    let parent = target
        .parent()
        .ok_or_else(|| "导出路径没有父目录。".to_string())?;
    let name = target
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("render");
    Ok(parent.join(format!(".{name}-{}.tmp.mp4", Uuid::new_v4())))
}

/// 渲染烧录完整 ASS 样式的 MP4。临时文件位于输出目标旁，ASS 位于项目缓存。
pub fn render_project_mp4_to(
    store: &ProjectStore,
    name: &str,
    tools: &FfmpegTools,
    cache_dir: &Path,
    target: &Path,
) -> OperationResult<ProjectExportPreview> {
    if !ffmpeg_supports_ass_filter(tools) {
        let mut result = OperationResult::failed(
            "RENDER_ASS_FILTER_MISSING",
            "当前 ffmpeg 未包含 ASS/libass 字幕滤镜，无法可靠烧录项目级字幕。",
        );
        result.diagnostics[0].suggested_action =
            Some("使用 App 随附的渲染运行时；开发环境请安装带 libass 的 ffmpeg。".to_string());
        return result;
    }
    let mut result = preview_project_export(store, name);
    let Some(preview) = result.data.as_ref() else {
        return result;
    };
    if let Err(error) = std::fs::create_dir_all(cache_dir) {
        return OperationResult::failed(
            "RENDER_CACHE_FAILED",
            format!("无法创建项目缓存：{error}"),
        );
    }
    let subtitle_style = match store.subtitle_style() {
        Ok(style) => style,
        Err(error) => return storage_failure(error),
    };
    let ass_path = cache_ass_path(cache_dir);
    if let Err(error) = std::fs::write(
        &ass_path,
        export_ass(&preview.timeline, &subtitle_style, &preview.subtitle_cues),
    ) {
        return OperationResult::failed("RENDER_ASS_FAILED", format!("无法写入临时 ASS：{error}"));
    }
    let mut inputs = Vec::new();
    let mut input_indices = HashMap::new();
    for clip in &preview.timeline.clips {
        if input_indices.contains_key(&clip.source_asset_id) {
            continue;
        }
        let Some(source) = preview
            .timeline
            .sources
            .iter()
            .find(|source| source.asset_id == clip.source_asset_id)
        else {
            return OperationResult::failed("TIMELINE_SOURCE_MISSING", "时间线引用的素材不存在。");
        };
        input_indices.insert(source.asset_id.clone(), inputs.len());
        inputs.push(source.original_path.clone());
    }
    let filter = match ffmpeg_filter_graph(preview, &input_indices, &ass_path) {
        Ok(filter) => filter,
        Err(error) => return OperationResult::failed("RENDER_GRAPH_INVALID", error),
    };
    let temp = match temporary_mp4_path(target) {
        Ok(path) => path,
        Err(error) => return OperationResult::failed("EXPORT_WRITE_FAILED", error),
    };
    if let Some(parent) = target.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return OperationResult::failed(
            "EXPORT_WRITE_FAILED",
            format!("无法创建导出目录：{error}"),
        );
    }
    let mut command = Command::new(&tools.ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-y"]);
    for input in inputs {
        command.arg("-i").arg(input);
    }
    let output = command
        .args([
            "-filter_complex",
            &filter,
            "-map",
            "[vout]",
            "-map",
            "[aout]",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-ar",
            "48000",
            "-movflags",
            "+faststart",
        ])
        .arg(&temp)
        .output();
    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            let _ = std::fs::remove_file(&temp);
            return OperationResult::failed(
                "RENDER_FFMPEG_FAILED",
                format!(
                    "ffmpeg 渲染失败：{}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            );
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            return OperationResult::failed(
                "RENDER_FFMPEG_FAILED",
                format!("无法启动 ffmpeg：{error}"),
            );
        }
    };
    drop(output);
    if let Err(error) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return OperationResult::failed(
            "EXPORT_WRITE_FAILED",
            format!("无法原子替换 MP4：{error}"),
        );
    }
    let bytes = match std::fs::read(target) {
        Ok(bytes) => bytes,
        Err(error) => {
            return OperationResult::failed(
                "EXPORT_WRITE_FAILED",
                format!("无法读取成片：{error}"),
            );
        }
    };
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let Some(first_clip) = preview.timeline.clips.first() else {
        return OperationResult::failed("TIMELINE_EMPTY", "主轨没有可渲染的片段。");
    };
    let path = target.to_string_lossy().into_owned();
    match store.apply_export_artifact(
        &Uuid::new_v4().to_string(),
        &first_clip.source_asset_id,
        "mp4_burned_subtitles",
        &path,
        &sha256,
    ) {
        Ok(revision) => result.revision = Some(revision),
        Err(error) => return storage_failure(error),
    }
    result.outputs.push(OutputArtifact {
        kind: "mp4_burned_subtitles".to_string(),
        path,
        sha256: Some(sha256),
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{CanvasSpec, TimelineIRv2};
    use crate::rational::FrameRate;
    use crate::storage::NewMediaAsset;

    fn preview() -> ProjectExportPreview {
        let source = TimelineSource {
            asset_id: "a".to_string(),
            display_name: "a.mov".to_string(),
            original_path: "/tmp/a.mov".to_string(),
            rate: FrameRate::Fps30Ntsc,
            source_duration_frames: 300,
            audio_sample_rate: 44_100,
            audio_channels: Some(2),
            width: Some(1920),
            height: Some(1080),
            source_tc_start_frame: Some(0),
            source_tc_is_drop_frame: false,
        };
        ProjectExportPreview {
            timeline: TimelineIRv2 {
                schema_version: 2,
                name: "render".to_string(),
                rate: FrameRate::Fps25,
                canvas: CanvasSpec::default(),
                sources: vec![source],
                source_cuts: Vec::new(),
                clips: vec![crate::ResolvedTimelineClip {
                    id: "c".to_string(),
                    source_asset_id: "a".to_string(),
                    source_in_frame: 30,
                    source_out_frame: 90,
                    timeline_start_frame: 0,
                    timeline_end_frame: 50,
                }],
                output_duration_frames: 50,
                output_map: Vec::new(),
            },
            subtitle_cues: Vec::new(),
            compatibility: Vec::new(),
        }
    }

    #[test]
    fn graph_uses_source_frames_output_frames_and_48khz_audio() {
        let mut indices = HashMap::new();
        indices.insert("a".to_string(), 0);
        let graph = ffmpeg_filter_graph(&preview(), &indices, Path::new("/tmp/caption file.ass"))
            .expect("graph");
        assert!(graph.contains("trim=start_frame=30:end_frame=90"));
        assert!(graph.contains("fps=fps=25/1,trim=end_frame=50"));
        assert!(graph.contains("atrim=start_sample=44144:end_sample=132432"));
        assert!(graph.contains("aresample=48000"));
        assert!(graph.contains("ass=filename='/tmp/caption file.ass'"));
    }

    #[test]
    fn renders_a_mixed_rate_project_when_local_ffmpeg_is_available() {
        let Ok(tools) = FfmpegTools::discover() else {
            eprintln!("skip: ffmpeg unavailable");
            return;
        };
        if !ffmpeg_supports_ass_filter(&tools) {
            eprintln!("skip: ffmpeg lacks ass/libass filter");
            return;
        }
        let root = std::env::temp_dir().join(format!("double-love-render-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root");
        let first = root.join("first.mp4");
        let second = root.join("second.mp4");
        for (target, rate, color) in [
            (&first, "25", "steelblue"),
            (&second, "30000/1001", "seagreen"),
        ] {
            let output = Command::new(&tools.ffmpeg)
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                ])
                .arg(format!("color=c={color}:s=320x180:r={rate}"))
                .args([
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000",
                    "-t",
                    "2",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                    "-c:a",
                    "aac",
                ])
                .arg(target)
                .output()
                .expect("ffmpeg starts");
            assert!(
                output.status.success(),
                "fixture ffmpeg: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let store = ProjectStore::open(&root.join("project.sqlite")).expect("store");
        for (id, path, rate) in [
            ("a", &first, FrameRate::Fps25),
            ("b", &second, FrameRate::Fps30Ntsc),
        ] {
            let rational = rate.rational();
            store
                .insert_media_asset(&NewMediaAsset {
                    id: id.to_string(),
                    kind: "video".to_string(),
                    original_path: path.to_string_lossy().into_owned(),
                    display_name: format!("{id}.mp4"),
                    duration_samples: 96_000,
                    audio_sample_rate: 48_000,
                    fps_num: rational.num,
                    fps_den: rational.den,
                    video_timebase: rate.timebase(),
                    is_ntsc: rate.is_ntsc(),
                    width: Some(320),
                    height: Some(180),
                    audio_channels: Some(1),
                    source_tc_start_frame: Some(0),
                    source_tc_is_drop_frame: false,
                    ffprobe_json: "{}".to_string(),
                })
                .expect("asset");
        }
        store
            .append_main_track_clip("c-a", "a", 0, 25)
            .expect("clip a");
        store
            .append_main_track_clip("c-b", "b", 0, 30)
            .expect("clip b");
        let target = root.join("rough-cut.mp4");
        let result = render_project_mp4_to(&store, "mixed", &tools, &root.join("cache"), &target);
        assert_eq!(
            result.status,
            crate::OperationStatus::Success,
            "{:?}",
            result.diagnostics
        );
        assert!(target.is_file());
        let probe = Command::new(&tools.ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=sample_rate",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(&target)
            .output()
            .expect("ffprobe starts");
        assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "48000");
        drop(store);
        std::fs::remove_dir_all(root).ok();
    }
}
