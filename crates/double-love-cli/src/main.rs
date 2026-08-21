use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use clap::{Parser, ValueEnum};
use double_love_engine::{
    DEFAULT_HANDLES_MS, FfmpegTools, OperationResult, ProgressSink, ProjectStore, Sidecar,
    TaskRegistry, TranscribeConfig, append_full_main_track_asset, append_main_track_clip,
    compile_project_timeline, create_project, export_project_ass_to, export_project_xmeml_to,
    export_rough_cut, ffmpeg_supports_ass_filter, import_media, move_main_track_clip, omit_words,
    open_project, preview_project_export, remove_main_track_clip, render_project_mp4_to,
    resolve_python, restore_words, split_main_track_clip, start_transcription,
    trim_main_track_clip,
};

#[derive(Debug, Parser)]
#[command(
    name = "double-love",
    version,
    about = "Double Love Studio local project CLI"
)]
struct Cli {
    #[arg(long)]
    json: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    offline: bool,
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long)]
    no_apply: bool,
    #[arg(long, short)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// 打印 CLI、sidecar 与 TimelineIR 的版本信息（无需项目）。
    Version,
    /// 输出测试版的稳定能力边界（无需项目）。
    Spec,
    /// 检查 ffmpeg、Python 与本地 sidecar 目录（不下载任何模型）。
    Doctor {
        #[arg(long, default_value = "sidecars/asr")]
        asr_dir: PathBuf,
        #[arg(long, default_value = "sidecars/speaker")]
        speaker_dir: PathBuf,
    },
    /// 在强制离线模式下加载本地模型，验证模型文件和运行时是否完整。
    ModelVerify {
        #[arg(value_enum)]
        component: ModelComponent,
        #[arg(long, default_value = "sidecars/asr")]
        asr_dir: PathBuf,
        #[arg(long, default_value = "sidecars/speaker")]
        speaker_dir: PathBuf,
    },
    /// 运行无模型的 JSONL sidecar 协议自检，适合安装后快速确认。
    ModelTest {
        #[arg(value_enum)]
        component: ModelComponent,
        #[arg(long, default_value = "sidecars/asr")]
        asr_dir: PathBuf,
        #[arg(long, default_value = "sidecars/speaker")]
        speaker_dir: PathBuf,
    },
    ProjectCreate,
    ProjectOpen,
    /// 导入本地媒体：ffprobe 探测 + 校验 + 抽取 16kHz 准备音频
    ImportMedia {
        /// 媒体文件路径（原始媒体只读引用）
        #[arg(long)]
        file: PathBuf,
    },
    /// 转录：sidecar（Qwen3-ASR）逐词时间戳 → 项目库
    Transcribe {
        /// import-media 返回的资产 id
        #[arg(long)]
        asset: String,
        /// qwen3-asr-1.7b（默认）/ qwen3-asr-0.6b
        #[arg(long, default_value = "qwen3-asr-1.7b")]
        model: String,
        /// auto / zh / en
        #[arg(long, default_value = "auto")]
        language: String,
        /// 使用确定性 mock 引擎（测试/开发自举，不加载模型）
        #[arg(long)]
        mock: bool,
        /// 切块秒数
        #[arg(long, default_value = "30")]
        chunk_seconds: i64,
        /// double_love_asr 包目录
        #[arg(long, default_value = "sidecars/asr")]
        sidecar_dir: PathBuf,
    },
    /// 删除词区间（omit）：删除文字 ≠ 删除底层词，可用 edit-restore 恢复
    EditOmit {
        /// 资产 id
        #[arg(long)]
        asset: String,
        /// 起始词序（含）
        #[arg(long)]
        start: i64,
        /// 结束词序（含）
        #[arg(long)]
        end: i64,
        /// 切点前保留毫秒（默认 120）
        #[arg(long, default_value_t = DEFAULT_HANDLES_MS)]
        handles_before: i64,
        /// 切点后保留毫秒（默认 120）
        #[arg(long, default_value_t = DEFAULT_HANDLES_MS)]
        handles_after: i64,
    },
    /// 恢复某个 omit 的词区间（完全覆盖则整条恢复，部分覆盖自动拆段）
    EditRestore {
        /// 原 omit 操作 id
        #[arg(long)]
        operation: String,
        /// 起始词序（含）
        #[arg(long)]
        start: i64,
        /// 结束词序（含）
        #[arg(long)]
        end: i64,
    },
    /// 编译粗剪时间线：默认 preview 只算不写；--apply 落盘 XMEML + sha256 + export_artifact
    ExportRoughcut {
        /// 资产 id
        #[arg(long)]
        asset: String,
        /// 实际写入导出文件（不带此旗标为 preview，不落盘不写库）
        #[arg(long)]
        apply: bool,
        /// 导出目录（默认 <project>/.doublelove/exports）
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// 显示多素材 TimelineIR v2（不写文件）。
    TimelineShow,
    /// 将完整素材或指定源帧区间加入主轨。
    MainTrackAdd {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        source_in_frame: Option<i64>,
        #[arg(long)]
        source_out_frame: Option<i64>,
    },
    MainTrackMove {
        #[arg(long)]
        clip: String,
        #[arg(long)]
        before: Option<String>,
    },
    MainTrackTrim {
        #[arg(long)]
        clip: String,
        #[arg(long)]
        source_in_frame: i64,
        #[arg(long)]
        source_out_frame: i64,
    },
    MainTrackSplit {
        #[arg(long)]
        clip: String,
        #[arg(long)]
        source_at_frame: i64,
    },
    MainTrackRemove {
        #[arg(long)]
        clip: String,
    },
    /// 预览或写出项目级 ASS、XMEML 或烧录 MP4。
    ExportProject {
        #[arg(value_enum)]
        format: ProjectExportFormat,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModelComponent {
    Asr,
    Speaker,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProjectExportFormat {
    Xml,
    Ass,
    Mp4,
}

fn main() {
    let cli = Cli::parse();
    let requires_project = !matches!(
        &cli.command,
        Command::Version
            | Command::Spec
            | Command::Doctor { .. }
            | Command::ModelVerify { .. }
            | Command::ModelTest { .. }
    );
    if requires_project && cli.project.is_none() {
        emit(
            &cli,
            &OperationResult::<()>::failed(
                "PROJECT_REQUIRED",
                "请通过 --project 指定本地项目目录。",
            ),
        );
        std::process::exit(2);
    }

    if cli.verbose {
        eprintln!(
            "offline={}, dry_run={}, no_apply={}",
            cli.offline, cli.dry_run, cli.no_apply
        );
    }

    if requires_project && (cli.dry_run || cli.no_apply) {
        let result =
            OperationResult::success(format!("dry-run: {:?} 不会写入项目目录", cli.command));
        emit(&cli, &result);
        return;
    }

    let result = match &cli.command {
        Command::Version => into_json(OperationResult::success(serde_json::json!({
            "cli": env!("CARGO_PKG_VERSION"),
            "timeline_ir": [double_love_engine::TIMELINE_IR_SCHEMA_VERSION, double_love_engine::TIMELINE_IR_V2_SCHEMA_VERSION],
            "sidecar_protocol": double_love_engine::SIDECAR_PROTOCOL_VERSION,
        }))),
        Command::Spec => into_json(OperationResult::success(serde_json::json!({
            "product": "local transcript-driven rough cut",
            "main_track": ["add", "reorder", "trim", "split", "remove"],
            "outputs": ["ASS", "burned_subtitle_MP4", "Premiere_Resolve_XMEML"],
            "canvas": "project-wide only",
            "subtitle_style": "project-wide only",
            "not_in_beta": ["OCR", "screen text replacement", "B-roll", "picture in picture", "per-clip transforms", "keyframes"],
        }))),
        Command::Doctor {
            asr_dir,
            speaker_dir,
        } => into_json(doctor(asr_dir, speaker_dir)),
        Command::ModelVerify {
            component,
            asr_dir,
            speaker_dir,
        } => into_json(model_verify(*component, asr_dir, speaker_dir)),
        Command::ModelTest {
            component,
            asr_dir,
            speaker_dir,
        } => into_json(model_test(*component, asr_dir, speaker_dir)),
        Command::ProjectCreate => into_json(
            create_project(cli.project.as_ref().expect("project required"))
                .map(OperationResult::success)
                .unwrap_or_else(|error| {
                    OperationResult::failed("PROJECT_CREATE_FAILED", error.to_string())
                }),
        ),
        Command::ProjectOpen => into_json(
            open_project(cli.project.as_ref().expect("project required"))
                .map(OperationResult::success)
                .unwrap_or_else(|error| {
                    OperationResult::failed("PROJECT_OPEN_FAILED", error.to_string())
                }),
        ),
        Command::ImportMedia { file } => into_json(import_media_command(
            cli.project.as_ref().expect("project required"),
            file,
        )),
        Command::Transcribe {
            asset,
            model,
            language,
            mock,
            chunk_seconds,
            sidecar_dir,
        } => transcribe_command(
            cli.project.as_ref().expect("project required"),
            asset,
            model,
            language,
            *mock,
            *chunk_seconds,
            sidecar_dir,
        ),
        Command::EditOmit {
            asset,
            start,
            end,
            handles_before,
            handles_after,
        } => match open_store(cli.project.as_ref().expect("project required")) {
            Ok(store) => into_json(omit_words(
                &store,
                asset,
                *start,
                *end,
                *handles_before,
                *handles_after,
            )),
            Err(result) => *result,
        },
        Command::EditRestore {
            operation,
            start,
            end,
        } => match open_store(cli.project.as_ref().expect("project required")) {
            Ok(store) => into_json(restore_words(&store, operation, *start, *end)),
            Err(result) => *result,
        },
        Command::TimelineShow => {
            match open_store(cli.project.as_ref().expect("project required")) {
                Ok(store) => into_json(compile_project_timeline(
                    &store,
                    &project_timeline_name(cli.project.as_ref().expect("project required")),
                )),
                Err(result) => *result,
            }
        }
        Command::MainTrackAdd {
            asset,
            source_in_frame,
            source_out_frame,
        } => match open_store(cli.project.as_ref().expect("project required")) {
            Ok(store) => match (source_in_frame, source_out_frame) {
                (None, None) => into_json(append_full_main_track_asset(&store, asset)),
                (Some(source_in_frame), Some(source_out_frame)) => into_json(
                    append_main_track_clip(&store, asset, *source_in_frame, *source_out_frame),
                ),
                _ => into_json(OperationResult::<()>::failed(
                    "MAIN_TRACK_RANGE_REQUIRED",
                    "source-in-frame 与 source-out-frame 必须同时提供。",
                )),
            },
            Err(result) => *result,
        },
        Command::MainTrackMove { clip, before } => {
            match open_store(cli.project.as_ref().expect("project required")) {
                Ok(store) => into_json(move_main_track_clip(&store, clip, before.as_deref())),
                Err(result) => *result,
            }
        }
        Command::MainTrackTrim {
            clip,
            source_in_frame,
            source_out_frame,
        } => match open_store(cli.project.as_ref().expect("project required")) {
            Ok(store) => into_json(trim_main_track_clip(
                &store,
                clip,
                *source_in_frame,
                *source_out_frame,
            )),
            Err(result) => *result,
        },
        Command::MainTrackSplit {
            clip,
            source_at_frame,
        } => match open_store(cli.project.as_ref().expect("project required")) {
            Ok(store) => into_json(split_main_track_clip(&store, clip, *source_at_frame)),
            Err(result) => *result,
        },
        Command::MainTrackRemove { clip } => {
            match open_store(cli.project.as_ref().expect("project required")) {
                Ok(store) => into_json(remove_main_track_clip(&store, clip)),
                Err(result) => *result,
            }
        }
        Command::ExportProject { format, apply, out } => export_project_command(
            cli.project.as_ref().expect("project required"),
            *format,
            *apply,
            out.as_deref(),
        ),
        Command::ExportRoughcut { asset, apply, out } => {
            match open_store(cli.project.as_ref().expect("project required")) {
                Ok(store) => {
                    let exports_dir = out.clone().unwrap_or_else(|| {
                        cli.project
                            .as_ref()
                            .expect("project required")
                            .join(".doublelove/exports")
                    });
                    into_json(export_rough_cut(&store, asset, &exports_dir, *apply))
                }
                Err(result) => *result,
            }
        }
    };

    let failed = matches!(result.status, double_love_engine::OperationStatus::Failed);
    emit(&cli, &result);
    if failed {
        std::process::exit(1);
    }
}

fn into_json<T: serde::Serialize>(
    result: OperationResult<T>,
) -> OperationResult<serde_json::Value> {
    OperationResult {
        status: result.status,
        revision: result.revision,
        data: result
            .data
            .map(|data| serde_json::to_value(data).expect("data serializes")),
        counts: result.counts,
        diagnostics: result.diagnostics,
        outputs: result.outputs,
    }
}

fn project_timeline_name(project: &Path) -> String {
    project
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name} Rough Cut"))
        .unwrap_or_else(|| "Double Love Rough Cut".to_string())
}

fn export_project_command(
    project: &Path,
    format: ProjectExportFormat,
    apply: bool,
    out: Option<&Path>,
) -> OperationResult<serde_json::Value> {
    let store = match open_store(project) {
        Ok(store) => store,
        Err(result) => return *result,
    };
    let name = project_timeline_name(project);
    if !apply {
        return into_json(preview_project_export(&store, &name));
    }
    let exports = project.join(".doublelove/exports");
    let stem = project
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("double-love")
        .replace(' ', "_");
    match format {
        ProjectExportFormat::Xml => {
            let target = out
                .map(Path::to_path_buf)
                .unwrap_or_else(|| exports.join(format!("{stem}_ROUGH_CUT.xml")));
            into_json(export_project_xmeml_to(&store, &name, &target))
        }
        ProjectExportFormat::Ass => {
            let target = out
                .map(Path::to_path_buf)
                .unwrap_or_else(|| exports.join(format!("{stem}_ROUGH_CUT.ass")));
            into_json(export_project_ass_to(&store, &name, &target))
        }
        ProjectExportFormat::Mp4 => {
            let tools = match FfmpegTools::discover() {
                Ok(tools) => tools,
                Err(diagnostic) => {
                    let mut result = OperationResult::<serde_json::Value>::failed(
                        diagnostic.code,
                        diagnostic.cause,
                    );
                    result.diagnostics[0].suggested_action = diagnostic.suggested_action;
                    return result;
                }
            };
            let target = out
                .map(Path::to_path_buf)
                .unwrap_or_else(|| exports.join(format!("{stem}_ROUGH_CUT.mp4")));
            into_json(render_project_mp4_to(
                &store,
                &name,
                &tools,
                &project.join(".doublelove/cache"),
                &target,
            ))
        }
    }
}

fn sidecar_python(package_dir: &Path) -> Result<PathBuf, String> {
    resolve_python(None, &package_dir.join(".venv/bin/python")).ok_or_else(|| {
        format!(
            "找不到 Python 环境：{}（请运行对应的 prepare 脚本）",
            package_dir.display()
        )
    })
}

fn doctor(asr_dir: &Path, speaker_dir: &Path) -> OperationResult<serde_json::Value> {
    let mut checks = Vec::new();
    match FfmpegTools::discover() {
        Ok(tools) => checks.push(serde_json::json!({
            "name": "ffmpeg", "ok": true,
            "detail": format!("{}", tools.ffmpeg.display()),
        })),
        Err(diagnostic) => checks.push(serde_json::json!({
            "name": "ffmpeg", "ok": false, "code": diagnostic.code, "detail": diagnostic.cause,
        })),
    }
    if let Ok(tools) = FfmpegTools::discover() {
        checks.push(serde_json::json!({
            "name": "ffmpeg_ass_filter",
            "ok": ffmpeg_supports_ass_filter(&tools),
            "detail": if ffmpeg_supports_ass_filter(&tools) {
                "libass 可用于 MP4 字幕烧录"
            } else {
                "缺少 libass；开发环境可安装带 libass 的 ffmpeg，发布包必须随附该运行时"
            },
        }));
    }
    for (name, directory, script) in [
        ("asr", asr_dir, "scripts/prepare-asr.sh"),
        ("speaker", speaker_dir, "scripts/prepare-speaker.sh"),
    ] {
        let directory_ok = directory.is_dir();
        let venv_python = directory.join(".venv/bin/python");
        checks.push(serde_json::json!({
            "name": format!("{name}_sidecar"),
            "ok": directory_ok && venv_python.is_file(),
            "detail": if directory_ok {
                if venv_python.is_file() {
                    venv_python.display().to_string()
                } else {
                    format!("缺少专用运行环境；请运行 {script}")
                }
            } else {
                format!("目录不存在；请运行 {script}")
            },
        }));
    }
    let healthy = checks
        .iter()
        .all(|check| check["ok"].as_bool() == Some(true));
    let mut result = OperationResult::success(serde_json::json!({
        "offline": true,
        "checks": checks,
    }));
    if !healthy {
        result.status = double_love_engine::OperationStatus::Partial;
        result.diagnostics.push(double_love_engine::Diagnostic {
            level: double_love_engine::DiagnosticLevel::Warning,
            code: "DOCTOR_ACTION_REQUIRED".to_string(),
            cause: "部分本地运行环境尚未就绪。".to_string(),
            object_id: None,
            impact: "对应功能不可用，其余本地功能不受影响。".to_string(),
            blocks_export: false,
            suggested_action: Some("按每项 detail 中的 prepare 脚本完成安装后重试。".to_string()),
        });
    }
    result
}

fn model_verify(
    component: ModelComponent,
    asr_dir: &Path,
    speaker_dir: &Path,
) -> OperationResult<serde_json::Value> {
    let (name, package_dir, script) = match component {
        ModelComponent::Asr => (
            "asr",
            asr_dir,
            "from mlx_qwen3_asr import ForcedAligner, Session\nSession(model='Qwen/Qwen3-ASR-1.7B')\nForcedAligner(model_path='Qwen/Qwen3-ForcedAligner-0.6B')\nprint('asr model verified')",
        ),
        ModelComponent::Speaker => (
            "speaker",
            speaker_dir,
            "import wespeaker\nwespeaker.load_model('chinese')\nprint('speaker model verified')",
        ),
    };
    let python = match sidecar_python(package_dir) {
        Ok(python) => python,
        Err(error) => return OperationResult::failed("MODEL_RUNTIME_MISSING", error),
    };
    let output = ProcessCommand::new(python)
        .current_dir(package_dir)
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .args(["-c", script])
        .output();
    match output {
        Ok(output) if output.status.success() => OperationResult::success(serde_json::json!({
            "component": name,
            "offline": true,
            "verified": true,
        })),
        Ok(output) => OperationResult::failed(
            "MODEL_VERIFY_FAILED",
            format!(
                "{name} 本地模型不可用：{}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ),
        Err(error) => OperationResult::failed(
            "MODEL_VERIFY_FAILED",
            format!("无法启动本地模型校验：{error}"),
        ),
    }
}

fn model_test(
    component: ModelComponent,
    asr_dir: &Path,
    speaker_dir: &Path,
) -> OperationResult<serde_json::Value> {
    let (name, package_dir, module) = match component {
        ModelComponent::Asr => ("asr", asr_dir, "double_love_asr"),
        ModelComponent::Speaker => ("speaker", speaker_dir, "double_love_speaker"),
    };
    let python = match sidecar_python(package_dir) {
        Ok(python) => python,
        Err(error) => return OperationResult::failed("MODEL_RUNTIME_MISSING", error),
    };
    let directory =
        std::env::temp_dir().join(format!("double-love-model-test-{}", uuid::Uuid::new_v4()));
    let log_path = directory.join("sidecar.log");
    let spawned = Sidecar::spawn_module(&python, package_dir, module, true, &log_path);
    match spawned {
        Ok(sidecar) => {
            drop(sidecar);
            let _ = std::fs::remove_dir_all(&directory);
            OperationResult::success(serde_json::json!({
                "component": name,
                "mock_protocol": true,
                "tested": true,
            }))
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&directory);
            OperationResult::failed("MODEL_TEST_FAILED", error.to_string())
        }
    }
}

/// 打开项目库；失败时给出可直接 emit 的错误结果。
fn open_store(
    project: &std::path::Path,
) -> Result<ProjectStore, Box<OperationResult<serde_json::Value>>> {
    let summary = open_project(project).map_err(|error| {
        Box::new(OperationResult::failed(
            "PROJECT_OPEN_FAILED",
            format!("{error}（请先运行 project-create）"),
        ))
    })?;
    ProjectStore::open(std::path::Path::new(&summary.database))
        .map_err(|error| Box::new(OperationResult::failed("STORAGE_ERROR", error.to_string())))
}

fn import_media_command(
    project: &std::path::Path,
    file: &std::path::Path,
) -> OperationResult<double_love_engine::MediaAssetSummary> {
    let summary = match open_project(project) {
        Ok(summary) => summary,
        Err(error) => {
            return OperationResult::failed(
                "PROJECT_OPEN_FAILED",
                format!("{error}（请先运行 project-create）"),
            );
        }
    };
    let store = match ProjectStore::open(std::path::Path::new(&summary.database)) {
        Ok(store) => store,
        Err(error) => return OperationResult::failed("STORAGE_ERROR", error.to_string()),
    };
    let tools = match FfmpegTools::discover() {
        Ok(tools) => tools,
        Err(diagnostic) => {
            let mut result = OperationResult::failed(&diagnostic.code, &diagnostic.cause);
            result.diagnostics[0].suggested_action = diagnostic.suggested_action.clone();
            return result;
        }
    };
    let prepared_dir = std::path::Path::new(&summary.root).join(".doublelove/prepared");
    import_media(&store, &prepared_dir, &tools, file)
}

/// CLI 进度汇：进度与终态打 stderr（stdout 只留给结构化结果）。
struct CliSink;

impl ProgressSink for CliSink {
    fn progress(&self, event: double_love_engine::ProgressEvent) {
        match (event.completed, event.total) {
            (Some(done), Some(total)) => eprintln!(
                "[{}] {} {}/{} {}",
                event.task, event.phase, done, total, event.message
            ),
            _ => eprintln!("[{}] {} {}", event.task, event.phase, event.message),
        }
    }

    fn task_state(&self, task_id: &str, state: double_love_engine::TaskState) {
        eprintln!("[{task_id}] 终态：{state:?}");
    }
}

#[allow(clippy::too_many_arguments)]
fn transcribe_command(
    project: &std::path::Path,
    asset: &str,
    model: &str,
    language: &str,
    mock: bool,
    chunk_seconds: i64,
    sidecar_dir: &std::path::Path,
) -> OperationResult<serde_json::Value> {
    let summary = match open_project(project) {
        Ok(summary) => summary,
        Err(error) => {
            return OperationResult::failed(
                "PROJECT_OPEN_FAILED",
                format!("{error}（请先运行 project-create）"),
            );
        }
    };
    let store = match ProjectStore::open(std::path::Path::new(&summary.database)) {
        Ok(store) => Arc::new(Mutex::new(store)),
        Err(error) => return OperationResult::failed("STORAGE_ERROR", error.to_string()),
    };
    let config = TranscribeConfig {
        asset_id: asset.to_string(),
        model: model.to_string(),
        language: language.to_string(),
        mock,
        python: None,
        package_dir: sidecar_dir.to_path_buf(),
        log_dir: std::path::Path::new(&summary.root).join(".doublelove/logs"),
        chunk_seconds,
    };
    let registry = TaskRegistry::new();
    let task_id =
        match start_transcription(Arc::clone(&store), &registry, Arc::new(CliSink), config) {
            Ok(task_id) => task_id,
            Err(error) => return OperationResult::failed("TRANSCRIBE_START_FAILED", error),
        };

    let terminal = loop {
        match registry.state(&task_id) {
            Some(state)
                if !matches!(
                    state,
                    double_love_engine::TaskState::Pending | double_love_engine::TaskState::Running
                ) =>
            {
                break state;
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    };
    let words = store
        .lock()
        .map_err(|_| "存储锁不可用".to_string())
        .and_then(|guard| {
            guard
                .count_transcript_words(asset)
                .map_err(|error| error.to_string())
        })
        .unwrap_or(0);
    registry.shutdown();

    let data = serde_json::json!({
        "task_id": task_id,
        "state": terminal,
        "words": words,
    });
    match terminal {
        double_love_engine::TaskState::Succeeded => {
            let mut result = OperationResult::success(data);
            result.counts.processed = words;
            result.counts.total = words;
            result
        }
        double_love_engine::TaskState::Partial => {
            let mut result = OperationResult::success(data);
            result.status = double_love_engine::OperationStatus::Partial;
            result.counts.processed = words;
            result.counts.total = words;
            result.diagnostics.push(double_love_engine::Diagnostic {
                level: double_love_engine::DiagnosticLevel::Warning,
                code: "TRANSCRIBE_PARTIAL".to_string(),
                cause: "候选转录出现错误，旧版本保持不变（详见上方日志）。".to_string(),
                object_id: Some(asset.to_string()),
                impact: "新候选不会覆盖当前可编辑文本".to_string(),
                blocks_export: false,
                suggested_action: Some("检查 .doublelove/logs 下的 sidecar 日志。".to_string()),
            });
            result
        }
        double_love_engine::TaskState::Cancelled => {
            let mut result = OperationResult::failed(
                "TRANSCRIBE_CANCELLED",
                "转录已取消，当前可编辑版本保持不变。",
            );
            result.status = double_love_engine::OperationStatus::Cancelled;
            result.data = Some(data);
            result.counts.processed = words;
            result.diagnostics[0].blocks_export = false;
            result
        }
        _ => OperationResult::failed("TRANSCRIBE_FAILED", "转录失败（详见上方日志）。"),
    }
}

fn emit<T: serde::Serialize>(cli: &Cli, result: &OperationResult<T>) {
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(result).expect("result serializes")
        );
    } else {
        println!("status: {:?}", result.status);
        for diagnostic in &result.diagnostics {
            println!("{}: {}", diagnostic.code, diagnostic.cause);
        }
    }
}
