use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::Parser;
use double_love_engine::{
    DEFAULT_HANDLES_MS, FfmpegTools, OperationResult, ProgressSink, ProjectStore, TaskRegistry,
    TranscribeConfig, create_project, export_rough_cut, import_media, omit_words, open_project,
    restore_words, start_transcription,
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
}

fn main() {
    let cli = Cli::parse();
    let Some(project) = cli.project.as_ref() else {
        emit(
            &cli,
            &OperationResult::<()>::failed(
                "PROJECT_REQUIRED",
                "请通过 --project 指定本地项目目录。",
            ),
        );
        std::process::exit(2);
    };

    if cli.verbose {
        eprintln!(
            "offline={}, dry_run={}, no_apply={}",
            cli.offline, cli.dry_run, cli.no_apply
        );
    }

    if cli.dry_run || cli.no_apply {
        let result =
            OperationResult::success(format!("dry-run: {:?} 不会写入项目目录", cli.command));
        emit(&cli, &result);
        return;
    }

    let result = match &cli.command {
        Command::ProjectCreate => into_json(
            create_project(project)
                .map(OperationResult::success)
                .unwrap_or_else(|error| {
                    OperationResult::failed("PROJECT_CREATE_FAILED", error.to_string())
                }),
        ),
        Command::ProjectOpen => into_json(
            open_project(project)
                .map(OperationResult::success)
                .unwrap_or_else(|error| {
                    OperationResult::failed("PROJECT_OPEN_FAILED", error.to_string())
                }),
        ),
        Command::ImportMedia { file } => into_json(import_media_command(project, file)),
        Command::Transcribe {
            asset,
            model,
            language,
            mock,
            chunk_seconds,
            sidecar_dir,
        } => transcribe_command(
            project,
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
        } => match open_store(project) {
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
        } => match open_store(project) {
            Ok(store) => into_json(restore_words(&store, operation, *start, *end)),
            Err(result) => *result,
        },
        Command::ExportRoughcut { asset, apply, out } => match open_store(project) {
            Ok(store) => {
                let exports_dir = out
                    .clone()
                    .unwrap_or_else(|| project.join(".doublelove/exports"));
                into_json(export_rough_cut(&store, asset, &exports_dir, *apply))
            }
            Err(result) => *result,
        },
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
                cause: "转录完成但出现非致命错误（详见上方日志）。".to_string(),
                object_id: Some(asset.to_string()),
                impact: "部分转录词可能缺失".to_string(),
                blocks_export: false,
                suggested_action: Some("检查 .doublelove/logs 下的 sidecar 日志。".to_string()),
            });
            result
        }
        double_love_engine::TaskState::Cancelled => {
            let mut result =
                OperationResult::failed("TRANSCRIBE_CANCELLED", "转录已取消，已落库的词保留。");
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
