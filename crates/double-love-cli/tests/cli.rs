use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("double-love-cli-test-{label}-{unique}"))
}

fn double_love() -> Command {
    Command::new(env!("CARGO_BIN_EXE_double-love"))
}

#[test]
fn project_create_json_matches_operation_result_contract() {
    let root = unique_temp_dir("create");

    let output = double_love()
        .args(["--json", "--project"])
        .arg(&root)
        .arg("project-create")
        .output()
        .expect("cli runs");
    assert!(output.status.success());

    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is valid json");
    assert_eq!(result["status"], "success");
    assert!(result["revision"].is_null());
    assert!(result["diagnostics"].is_array());
    assert!(result["outputs"].is_array());
    for field in ["total", "processed", "skipped", "failed", "unmatched"] {
        assert!(result["counts"][field].is_number(), "counts.{field} exists");
    }
    assert!(result["data"]["project_id"].is_string());
    assert!(result["data"]["revision"].is_number());

    std::fs::remove_dir_all(&root).expect("temporary project is removed");
}

#[test]
fn missing_project_flag_exits_2_with_project_required() {
    let output = double_love()
        .args(["--json", "project-create"])
        .output()
        .expect("cli runs");
    assert_eq!(output.status.code(), Some(2));

    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is valid json");
    assert_eq!(result["status"], "failed");
    assert_eq!(result["diagnostics"][0]["code"], "PROJECT_REQUIRED");
}

#[test]
fn dry_run_writes_nothing_to_disk() {
    let root = unique_temp_dir("dry-run");

    let output = double_love()
        .args(["--json", "--dry-run", "--project"])
        .arg(&root)
        .arg("project-create")
        .output()
        .expect("cli runs");
    assert!(output.status.success());

    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is valid json");
    assert_eq!(result["status"], "success");
    assert!(
        !root.exists(),
        "dry-run must not create the project directory"
    );
}
