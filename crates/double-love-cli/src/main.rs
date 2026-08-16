use std::path::PathBuf;

use clap::Parser;
use double_love_engine::{OperationResult, create_project, open_project};

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

    let result = match cli.command {
        Command::ProjectCreate => create_project(project)
            .map(OperationResult::success)
            .unwrap_or_else(|error| {
                OperationResult::failed("PROJECT_CREATE_FAILED", error.to_string())
            }),
        Command::ProjectOpen => open_project(project)
            .map(OperationResult::success)
            .unwrap_or_else(|error| {
                OperationResult::failed("PROJECT_OPEN_FAILED", error.to_string())
            }),
    };

    let failed = matches!(result.status, double_love_engine::OperationStatus::Failed);
    emit(&cli, &result);
    if failed {
        std::process::exit(1);
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
