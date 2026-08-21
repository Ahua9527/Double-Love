//! 媒体导入：ffprobe 探测 + 白名单校验 + 抽取 16kHz 准备音频。
//! 校验规则来自坑清单：无视频流 / 无音频流 / VFR / 帧率白名单外一律拒绝导入，
//! 全部给出可执行的 suggested_action；时长一律走有理数，不经过 f64。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use uuid::Uuid;

use crate::contracts::{AssetStatus, MediaAssetSummary};
use crate::rational::{FrameRate, Rational};
use crate::storage::{MediaAssetRow, NewMediaAsset, ProjectStore, StorageError};
use crate::{Diagnostic, DiagnosticLevel, OperationResult};

/// ASR 准备音频规格：16kHz / 单声道 / pcm_s16le（PRD 空白的切片自定义②）。
pub const PREPARED_SAMPLE_RATE: i64 = 16_000;

/// 本机 ffmpeg/ffprobe 位置。
#[derive(Debug, Clone)]
pub struct FfmpegTools {
    pub ffprobe: PathBuf,
    pub ffmpeg: PathBuf,
}

impl FfmpegTools {
    /// 发现顺序：环境变量 → PATH → Homebrew 常见位置。
    pub fn discover() -> Result<Self, Box<Diagnostic>> {
        let ffprobe = discover_binary("DOUBLELOVE_FFPROBE", "ffprobe");
        let ffmpeg = discover_binary("DOUBLELOVE_FFMPEG", "ffmpeg");
        match (ffprobe, ffmpeg) {
            (Some(ffprobe), Some(ffmpeg)) => Ok(Self { ffprobe, ffmpeg }),
            _ => Err(Box::new(Diagnostic {
                level: DiagnosticLevel::Error,
                code: "MEDIA_TOOLS_MISSING".to_string(),
                cause: "找不到 ffprobe/ffmpeg，无法探测媒体文件。".to_string(),
                object_id: None,
                impact: "无法导入媒体".to_string(),
                blocks_export: true,
                suggested_action: Some(
                    "安装 ffmpeg（例如 brew install ffmpeg）后重试。".to_string(),
                ),
            })),
        }
    }

    /// 使用 App 打包的显式运行时路径。发布包优先走这里，开发期仍可回退 `discover()`。
    pub fn from_paths(ffprobe: PathBuf, ffmpeg: PathBuf) -> Result<Self, Box<Diagnostic>> {
        if is_executable(&ffprobe) && is_executable(&ffmpeg) {
            Ok(Self { ffprobe, ffmpeg })
        } else {
            Err(Box::new(Diagnostic {
                level: DiagnosticLevel::Error,
                code: "MEDIA_RUNTIME_MISSING".to_string(),
                cause: "App 随附的 ffmpeg/ffprobe 运行时不完整。".to_string(),
                object_id: None,
                impact: "无法导入或渲染媒体".to_string(),
                blocks_export: true,
                suggested_action: Some("重新安装完整的 Double Love Studio 测试版。".to_string()),
            }))
        }
    }
}

fn discover_binary(env_key: &str, name: &str) -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(env_key) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    for prefix in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let candidate = Path::new(prefix).join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// ffprobe JSON 输出中本切片关心的字段。
#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    channels: Option<i64>,
    sample_rate: Option<String>,
    time_base: Option<String>,
    duration_ts: Option<i64>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

/// 探测+校验通过的媒体事实（有理数、整数，无 f64）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbedMedia {
    rate: FrameRate,
    width: Option<i64>,
    height: Option<i64>,
    audio_channels: Option<i64>,
    audio_sample_rate: i64,
    duration_samples: i64,
    source_tc_start_frame: Option<i64>,
    source_tc_is_drop_frame: bool,
}

fn parse_timecode_start(text: &str, rate: FrameRate) -> Option<(i64, bool)> {
    let text = text.trim();
    let drop_frame = text.contains(';');
    let values = text
        .replace(';', ":")
        .split(':')
        .map(|part| part.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let [hours, minutes, seconds, frames] = values.as_slice() else {
        return None;
    };
    if !(*minutes < 60 && *seconds < 60 && *frames >= 0 && *frames < rate.timebase()) {
        return None;
    }
    let nominal = rate.timebase();
    let total_minutes = hours * 60 + minutes;
    let mut total = ((hours * 3600 + minutes * 60 + seconds) * nominal) + frames;
    if drop_frame {
        let dropped = match rate {
            FrameRate::Fps30Ntsc => 2,
            FrameRate::Fps60Ntsc => 4,
            _ => return None,
        };
        total -= dropped * (total_minutes - total_minutes / 10);
    }
    Some((total, drop_frame))
}

/// 十进制秒字符串（如 "10.000000"）→ 有理数秒；逐位解析，不经过 f64。
fn parse_decimal_seconds(text: &str) -> Option<Rational> {
    let text = text.trim();
    if text.is_empty() || text == "N/A" {
        return None;
    }
    let (whole, frac) = match text.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (text, ""),
    };
    if frac.len() > 9 || !whole.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let scale = 10_i64.pow(frac.len() as u32);
    let num = whole.parse::<i64>().ok()?.checked_mul(scale)?
        + if frac.is_empty() {
            0
        } else {
            frac.parse::<i64>().ok()?
        };
    Some(Rational::new(num, scale))
}

/// 有理秒 × 采样率 → 采样数（四舍五入到最近整数，i128 防溢出）。
fn seconds_to_samples(seconds: Rational, sample_rate: i64) -> i64 {
    let num = seconds.num as i128 * sample_rate as i128;
    let den = seconds.den as i128;
    ((num * 2 + den) / (den * 2)) as i64
}

/// 从 ffprobe JSON 构建并校验媒体事实；任何一条不满足即给出对应诊断。
fn validate_probe(probe: &FfprobeOutput) -> Result<ProbedMedia, Box<Diagnostic>> {
    let reject = |code: &str, cause: String, action: &str| {
        Box::new(Diagnostic {
            level: DiagnosticLevel::Error,
            code: code.to_string(),
            cause,
            object_id: None,
            impact: "该文件未导入".to_string(),
            blocks_export: true,
            suggested_action: Some(action.to_string()),
        })
    };

    let video = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| {
            reject(
                "MEDIA_NO_VIDEO_STREAM",
                "文件中没有视频流。".to_string(),
                "确认导入的是包含画面的视频文件。",
            )
        })?;
    let audio = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .ok_or_else(|| {
            reject(
                "MEDIA_NO_AUDIO_STREAM",
                "文件中没有音频流，无法转录。".to_string(),
                "换用带同期声的素材，或先为素材合入音频。",
            )
        })?;

    let avg = video
        .avg_frame_rate
        .as_deref()
        .and_then(Rational::parse)
        .filter(|r| r.num > 0)
        .ok_or_else(|| {
            reject(
                "MEDIA_PROBE_INCOMPLETE",
                "ffprobe 未能给出有效帧率。".to_string(),
                "用 ffprobe 手动检查该文件；必要时重新封装。",
            )
        })?;
    let r = video.r_frame_rate.as_deref().and_then(Rational::parse);
    if let Some(r) = r
        && r != avg
    {
        return Err(reject(
            "MEDIA_VFR_UNSUPPORTED",
            format!(
                "检测到可变帧率（avg={}/{}, r={}/{}）。",
                avg.num, avg.den, r.num, r.den
            ),
            "先用 ffmpeg 转码为恒定帧率（CFR）再导入，例如：ffmpeg -i 输入 -vsync cfr -r 帧率 输出。",
        ));
    }

    let rate = FrameRate::from_rational(&avg).ok_or_else(|| {
        reject(
            "MEDIA_FPS_UNSUPPORTED",
            format!("帧率 {}/{} 不在支持列表内。", avg.num, avg.den),
            "支持的帧率：24、23.976、25、30、29.97、50、60、59.94；请先转码到其中之一。",
        )
    })?;
    let source_timecode = video
        .tags
        .get("timecode")
        .or_else(|| {
            probe
                .format
                .as_ref()
                .and_then(|format| format.tags.get("timecode"))
        })
        .and_then(|timecode| parse_timecode_start(timecode, rate));

    let audio_sample_rate: i64 = audio
        .sample_rate
        .as_deref()
        .and_then(|s| s.parse().ok())
        .filter(|rate| *rate > 0)
        .ok_or_else(|| {
            reject(
                "MEDIA_PROBE_INCOMPLETE",
                "ffprobe 未能给出有效音频采样率。".to_string(),
                "用 ffprobe 手动检查该文件；必要时重新封装。",
            )
        })?;

    // 时长：优先视频流 duration_ts × time_base（精确有理数），退回 format.duration 十进制。
    let duration_seconds = video
        .duration_ts
        .zip(video.time_base.as_deref().and_then(Rational::parse))
        .filter(|(ts, tb)| *ts >= 0 && tb.num > 0)
        .map(|(ts, tb)| Rational::new(ts * tb.num, tb.den))
        .or_else(|| {
            probe
                .format
                .as_ref()
                .and_then(|f| f.duration.as_deref())
                .and_then(parse_decimal_seconds)
        })
        .ok_or_else(|| {
            reject(
                "MEDIA_PROBE_INCOMPLETE",
                "无法确定媒体时长。".to_string(),
                "用 ffprobe 手动检查该文件；必要时重新封装。",
            )
        })?;
    let duration_samples = seconds_to_samples(duration_seconds, audio_sample_rate);
    if duration_samples <= 0 {
        return Err(reject(
            "MEDIA_PROBE_INCOMPLETE",
            "媒体时长为零。".to_string(),
            "确认文件未损坏；必要时重新封装。",
        ));
    }

    Ok(ProbedMedia {
        rate,
        width: video.width,
        height: video.height,
        audio_channels: audio.channels,
        audio_sample_rate,
        duration_samples,
        source_tc_start_frame: source_timecode.map(|(frame, _)| frame),
        source_tc_is_drop_frame: source_timecode.is_some_and(|(_, drop)| drop),
    })
}

/// 运行 ffprobe，返回解析结果与原始 JSON 文本（原文入库存档）。
fn run_ffprobe(
    tools: &FfmpegTools,
    path: &Path,
) -> Result<(FfprobeOutput, String), Box<Diagnostic>> {
    let failure = |code: &str, cause: String, action: &str| {
        Box::new(Diagnostic {
            level: DiagnosticLevel::Error,
            code: code.to_string(),
            cause,
            object_id: None,
            impact: "该文件未导入".to_string(),
            blocks_export: true,
            suggested_action: Some(action.to_string()),
        })
    };
    let output = Command::new(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let raw = String::from_utf8_lossy(&out.stdout).into_owned();
            let parsed = serde_json::from_str(&raw).map_err(|error| {
                failure(
                    "MEDIA_PROBE_INCOMPLETE",
                    format!("ffprobe 输出无法解析：{error}"),
                    "用 ffprobe 手动检查该文件。",
                )
            })?;
            Ok((parsed, raw))
        }
        Ok(out) => Err(failure(
            "MEDIA_PROBE_FAILED",
            format!("ffprobe 执行失败：{}", String::from_utf8_lossy(&out.stderr)),
            "确认文件存在且未损坏。",
        )),
        Err(error) => Err(failure(
            "MEDIA_PROBE_FAILED",
            format!("无法启动 ffprobe：{error}"),
            "确认 ffmpeg 安装完整。",
        )),
    }
}

/// 抽取 16kHz 单声道 pcm_s16le 准备音频（ASR 输入）。
fn run_prepare_wav(
    tools: &FfmpegTools,
    source: &Path,
    wav_path: &Path,
) -> Result<(), Box<Diagnostic>> {
    let output = Command::new(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(PREPARED_SAMPLE_RATE.to_string())
        .args(["-c:a", "pcm_s16le"])
        .arg(wav_path)
        .output();
    match output {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(Box::new(Diagnostic {
            level: DiagnosticLevel::Error,
            code: "MEDIA_WAV_FAILED".to_string(),
            cause: format!("准备音频抽取失败：{}", String::from_utf8_lossy(&out.stderr)),
            object_id: None,
            impact: "该文件未导入".to_string(),
            blocks_export: true,
            suggested_action: Some("确认磁盘空间充足后重试。".to_string()),
        })),
        Err(error) => Err(Box::new(Diagnostic {
            level: DiagnosticLevel::Error,
            code: "MEDIA_WAV_FAILED".to_string(),
            cause: format!("无法启动 ffmpeg：{error}"),
            object_id: None,
            impact: "该文件未导入".to_string(),
            blocks_export: true,
            suggested_action: Some("确认 ffmpeg 安装完整。".to_string()),
        })),
    }
}

fn row_to_summary(row: MediaAssetRow) -> Option<MediaAssetSummary> {
    Some(MediaAssetSummary {
        id: row.id,
        display_name: row.display_name,
        duration_samples: row.duration_samples,
        audio_sample_rate: row.audio_sample_rate,
        rate: FrameRate::from_rational(&Rational::new(row.fps_num, row.fps_den))?,
        width: row.width,
        height: row.height,
        audio_channels: row.audio_channels,
        status: AssetStatus::parse(&row.status)?,
    })
}

fn storage_failure<T>(error: StorageError) -> OperationResult<T> {
    OperationResult::failed("STORAGE_ERROR", error.to_string())
}

/// 列出全部已导入资产（GUI 资产列表；FrameRate 无法识别的行跳过）。
pub fn list_media_assets(store: &ProjectStore) -> OperationResult<Vec<MediaAssetSummary>> {
    match store.media_assets() {
        Ok(rows) => OperationResult::success(rows.into_iter().filter_map(row_to_summary).collect()),
        Err(error) => storage_failure(error),
    }
}

/// 导入一个本地媒体文件：探测 → 校验 → 落库 → 抽取准备音频。
/// 原始媒体只读引用；重复导入同一路径复用既有资产并给出 info 诊断。
pub fn import_media(
    store: &ProjectStore,
    prepared_dir: &Path,
    tools: &FfmpegTools,
    source: &Path,
) -> OperationResult<MediaAssetSummary> {
    let source = match source.canonicalize() {
        Ok(path) if path.is_file() => path,
        _ => {
            return OperationResult::failed(
                "MEDIA_FILE_MISSING",
                format!("文件不存在或不是普通文件：{}", source.display()),
            );
        }
    };

    match store.media_asset_by_path(&source.to_string_lossy()) {
        Ok(Some(existing)) => {
            let Some(summary) = row_to_summary(existing) else {
                return OperationResult::failed("STORAGE_CORRUPT", "已存资产的帧率字段无法解析。");
            };
            let mut result = OperationResult::success(summary);
            result.diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Info,
                code: "MEDIA_ALREADY_IMPORTED".to_string(),
                cause: "该文件此前已导入，复用现有资产。".to_string(),
                object_id: result.data.as_ref().map(|s| s.id.clone()),
                impact: "无".to_string(),
                blocks_export: false,
                suggested_action: None,
            });
            return result;
        }
        Ok(None) => {}
        Err(error) => return storage_failure(error),
    }

    let (probe, ffprobe_json) = match run_ffprobe(tools, &source) {
        Ok(probe) => probe,
        Err(diagnostic) => {
            return OperationResult::failed(&diagnostic.code, &diagnostic.cause);
        }
    };
    let probed = match validate_probe(&probe) {
        Ok(probed) => probed,
        Err(diagnostic) => {
            let mut result = OperationResult::failed(&diagnostic.code, &diagnostic.cause);
            result.diagnostics[0].suggested_action = diagnostic.suggested_action;
            return result;
        }
    };

    let display_name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled".to_string());
    let asset = NewMediaAsset {
        id: Uuid::new_v4().to_string(),
        kind: "video".to_string(),
        original_path: source.to_string_lossy().into_owned(),
        display_name: display_name.clone(),
        duration_samples: probed.duration_samples,
        audio_sample_rate: probed.audio_sample_rate,
        fps_num: probed.rate.rational().num,
        fps_den: probed.rate.rational().den,
        video_timebase: probed.rate.timebase(),
        is_ntsc: probed.rate.is_ntsc(),
        width: probed.width,
        height: probed.height,
        audio_channels: probed.audio_channels,
        source_tc_start_frame: probed.source_tc_start_frame,
        source_tc_is_drop_frame: probed.source_tc_is_drop_frame,
        ffprobe_json,
    };
    if let Err(error) = store.insert_media_asset(&asset) {
        return storage_failure(error);
    }

    if let Err(error) = std::fs::create_dir_all(prepared_dir) {
        return OperationResult::failed(
            "MEDIA_WAV_FAILED",
            format!("无法创建准备音频目录：{error}"),
        );
    }
    let wav_path = prepared_dir.join(format!("{}.wav", asset.id));
    if let Err(diagnostic) = run_prepare_wav(tools, &source, &wav_path) {
        let mut result = OperationResult::failed(&diagnostic.code, &diagnostic.cause);
        result.diagnostics[0].suggested_action = diagnostic.suggested_action;
        return result;
    }
    if let Err(error) = store.set_asset_prepared(&asset.id, &wav_path.to_string_lossy()) {
        return storage_failure(error);
    }

    OperationResult::success(MediaAssetSummary {
        id: asset.id,
        display_name,
        duration_samples: probed.duration_samples,
        audio_sample_rate: probed.audio_sample_rate,
        rate: probed.rate,
        width: probed.width,
        height: probed.height,
        audio_channels: probed.audio_channels,
        status: AssetStatus::Prepared,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_with_status(status: &str) -> MediaAssetRow {
        MediaAssetRow {
            id: "asset-1".to_string(),
            kind: "video".to_string(),
            original_path: "/tmp/source.mp4".to_string(),
            display_name: "source.mp4".to_string(),
            duration_samples: 48_000,
            audio_sample_rate: 48_000,
            fps_num: 25,
            fps_den: 1,
            video_timebase: 25,
            is_ntsc: false,
            width: Some(1920),
            height: Some(1080),
            audio_channels: Some(2),
            source_tc_start_frame: None,
            source_tc_is_drop_frame: false,
            prepared_wav_path: Some("/tmp/prepared.wav".to_string()),
            status: status.to_string(),
        }
    }

    #[test]
    fn list_summary_keeps_persisted_transcribed_state() {
        let summary = row_to_summary(row_with_status("transcribed")).expect("known status");
        assert_eq!(summary.status, AssetStatus::Transcribed);
    }

    #[test]
    fn list_summary_rejects_unknown_persisted_state() {
        assert!(row_to_summary(row_with_status("unexpected")).is_none());
    }

    #[test]
    fn parses_ntsc_drop_frame_timecode_without_float_seconds() {
        assert_eq!(
            parse_timecode_start("01:00:00;00", FrameRate::Fps30Ntsc),
            Some((107_892, true))
        );
        assert_eq!(
            parse_timecode_start("01:00:00:00", FrameRate::Fps30Ntsc),
            Some((108_000, false))
        );
        assert_eq!(parse_timecode_start("00:01:00;00", FrameRate::Fps25), None);
    }

    fn probe_json(video: &str, audio: &str, format: &str) -> FfprobeOutput {
        let text = format!(r#"{{"streams":[{video},{audio}],"format":{format}}}"#);
        serde_json::from_str(&text).expect("synthetic probe json parses")
    }

    const VIDEO_25: &str = r#"{
        "codec_type":"video","avg_frame_rate":"25/1","r_frame_rate":"25/1",
        "width":1920,"height":1080,"time_base":"1/12800","duration_ts":128000
    }"#;
    const AUDIO_48K: &str = r#"{
        "codec_type":"audio","sample_rate":"48000","channels":2
    }"#;
    const FORMAT_10S: &str = r#"{"duration":"10.000000"}"#;

    #[test]
    fn accepts_25fps_cfr_and_computes_samples_without_floats() {
        let probe = probe_json(VIDEO_25, AUDIO_48K, FORMAT_10S);
        let media = validate_probe(&probe).expect("valid probe");
        assert_eq!(media.rate, FrameRate::Fps25);
        assert_eq!(media.audio_sample_rate, 48000);
        // 128000 / 12800 = 10s → 480000 采样
        assert_eq!(media.duration_samples, 480_000);
        assert_eq!(media.width, Some(1920));
    }

    #[test]
    fn accepts_ntsc_rate_and_maps_timebase() {
        let video = r#"{
            "codec_type":"video","avg_frame_rate":"30000/1001","r_frame_rate":"30000/1001",
            "width":1920,"height":1080,"time_base":"1/30000","duration_ts":300000
        }"#;
        let probe = probe_json(video, AUDIO_48K, FORMAT_10S);
        let media = validate_probe(&probe).expect("valid ntsc probe");
        assert_eq!(media.rate, FrameRate::Fps30Ntsc);
        assert!(media.rate.is_ntsc());
        assert_eq!(media.rate.timebase(), 30);
    }

    #[test]
    fn rejects_vfr_with_cfr_suggestion() {
        let video = r#"{
            "codec_type":"video","avg_frame_rate":"30000/1001","r_frame_rate":"30/1",
            "width":1920,"height":1080
        }"#;
        let probe = probe_json(video, AUDIO_48K, FORMAT_10S);
        let error = validate_probe(&probe).expect_err("vfr must be rejected");
        assert_eq!(error.code, "MEDIA_VFR_UNSUPPORTED");
        assert!(error.suggested_action.expect("action").contains("CFR"));
    }

    #[test]
    fn rejects_fps_outside_whitelist() {
        let video = r#"{
            "codec_type":"video","avg_frame_rate":"15/1","r_frame_rate":"15/1",
            "width":1920,"height":1080
        }"#;
        let probe = probe_json(video, AUDIO_48K, FORMAT_10S);
        let error = validate_probe(&probe).expect_err("15fps must be rejected");
        assert_eq!(error.code, "MEDIA_FPS_UNSUPPORTED");
    }

    #[test]
    fn rejects_missing_streams() {
        let no_video = probe_json(AUDIO_48K, AUDIO_48K, FORMAT_10S);
        let probe: FfprobeOutput = serde_json::from_str(
            r#"{"streams":[{"codec_type":"audio","sample_rate":"48000","channels":2}],"format":{"duration":"10.0"}}"#,
        )
        .expect("audio-only json");
        let _ = no_video;
        assert_eq!(
            validate_probe(&probe)
                .expect_err("audio-only must be rejected")
                .code,
            "MEDIA_NO_VIDEO_STREAM"
        );

        let no_audio: FfprobeOutput = serde_json::from_str(
            r#"{"streams":[{"codec_type":"video","avg_frame_rate":"25/1","r_frame_rate":"25/1","width":1920,"height":1080,"time_base":"1/25","duration_ts":250}],"format":{"duration":"10.0"}}"#,
        )
        .expect("video-only json");
        assert_eq!(
            validate_probe(&no_audio)
                .expect_err("video-only must be rejected")
                .code,
            "MEDIA_NO_AUDIO_STREAM"
        );
    }

    #[test]
    fn falls_back_to_format_duration_and_parses_decimals_exactly() {
        let video = r#"{
            "codec_type":"video","avg_frame_rate":"25/1","r_frame_rate":"25/1",
            "width":1920,"height":1080
        }"#;
        let probe = probe_json(video, AUDIO_48K, r#"{"duration":"2.500000"}"#);
        let media = validate_probe(&probe).expect("format duration fallback");
        assert_eq!(media.duration_samples, 120_000);

        assert_eq!(
            parse_decimal_seconds("10.000000"),
            Some(Rational::new(10, 1))
        );
        assert_eq!(parse_decimal_seconds("1.500"), Some(Rational::new(3, 2)));
        assert_eq!(parse_decimal_seconds("N/A"), None);
        assert_eq!(parse_decimal_seconds("1.0000000001"), None);
    }
}
