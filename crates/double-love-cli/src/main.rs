use std::path::PathBuf;

use clap::Parser;
use double_love_engine::{
    FfmpegTools, OperationResult, ProjectStore, create_project, import_media, open_project,
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
