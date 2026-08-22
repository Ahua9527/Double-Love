use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use double_love_engine::{ProjectError, create_project, open_project};

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("double-love-test-{label}-{unique}"))
}

#[test]
fn create_project_materializes_doublelove_structure() {
    let root = unique_temp_dir("create");

    let summary = create_project(&root).expect("project is created");

    let doublelove = root.join(".doublelove");
    assert!(doublelove.join("cache").is_dir());
    assert!(doublelove.join("logs").is_dir());
    assert!(doublelove.join("exports").is_dir());
    assert!(doublelove.join("project.sqlite").is_file());
    assert_eq!(summary.revision, 0);
    assert!(!summary.project_id.is_empty());

    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(doublelove.join("manifest.json")).expect("manifest exists"),
    )
    .expect("manifest is valid json");
    assert_eq!(
        manifest
            .as_object()
            .expect("manifest object")
            .keys()
            .collect::<Vec<_>>(),
        ["projectId", "rawInputs", "schemaVersion"]
    );
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["projectId"], summary.project_id);
    assert_eq!(manifest["rawInputs"], "read-only references");
    assert!(manifest.get("schema_version").is_none());
    assert!(manifest.get("project_id").is_none());

    fs::remove_dir_all(&root).expect("temporary project is removed");
}

#[test]
fn open_project_is_idempotent_and_keeps_project_id() {
    let root = unique_temp_dir("open");
    let created = create_project(&root).expect("project is created");

    let opened = open_project(&root).expect("project opens");
    let reopened = open_project(&root).expect("project reopens");

    assert_eq!(opened.project_id, created.project_id);
    assert_eq!(reopened.project_id, created.project_id);
    assert_eq!(opened.revision, created.revision);
    assert_eq!(reopened.revision, created.revision);

    fs::remove_dir_all(&root).expect("temporary project is removed");
}

#[test]
fn open_project_rejects_directory_without_project() {
    let root = unique_temp_dir("missing");
    fs::create_dir_all(&root).expect("empty directory is created");

    let error = open_project(&root).expect_err("open must fail without a project");
    assert!(matches!(error, ProjectError::InvalidPath(_)));

    fs::remove_dir_all(&root).expect("temporary directory is removed");
}

#[test]
fn create_project_rejects_file_path() {
    let root = unique_temp_dir("file");
    fs::create_dir_all(&root).expect("parent directory is created");
    let file = root.join("not-a-dir.txt");
    fs::write(&file, b"not a project").expect("file is written");

    let error = create_project(&file).expect_err("create must fail on a file path");
    assert!(matches!(error, ProjectError::NotDirectory(_)));

    fs::remove_dir_all(&root).expect("temporary directory is removed");
}
