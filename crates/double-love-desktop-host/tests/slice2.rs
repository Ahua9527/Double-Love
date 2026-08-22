use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use double_love_desktop_host::{
    framing::{read_frame, write_frame},
    protocol::{HostRequest, HostRequestMethod, HostResponse, HostResponseStatus},
};
use double_love_engine::{CanvasFit, CanvasSpec, ProjectStore, create_project};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

struct HostProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl HostProcess {
    fn spawn(app_data_dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_double-love-desktop-host"))
            .arg("--app-data-dir")
            .arg(app_data_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn desktop host");
        Self {
            stdin: child.stdin.take().expect("host stdin"),
            stdout: child.stdout.take().expect("host stdout"),
            child,
        }
    }

    fn invoke(&mut self, id: &str, name: &str, payload: Value) -> Value {
        let request = HostRequest::new(
            id,
            HostRequestMethod::Invoke {
                name: name.to_string(),
                payload,
            },
        );
        write_frame(
            &mut self.stdin,
            &serde_json::to_vec(&request).expect("serialize request"),
        )
        .expect("write request frame");
        loop {
            let frame = read_frame(&mut self.stdout)
                .expect("read host frame")
                .expect("host frame before EOF");
            let value: Value = serde_json::from_slice(&frame).expect("host JSON frame");
            if value.get("event").is_some() {
                continue;
            }
            let response: HostResponse = serde_json::from_value(value).expect("host response");
            assert_eq!(response.id, id);
            let result = operation(response);
            assert_operation_result_shape(&result);
            return result;
        }
    }

    fn stop(mut self) {
        let request = HostRequest::new("shutdown", HostRequestMethod::Shutdown);
        write_frame(
            &mut self.stdin,
            &serde_json::to_vec(&request).expect("serialize shutdown"),
        )
        .expect("write shutdown");
        read_frame(&mut self.stdout)
            .expect("read shutdown")
            .expect("shutdown response");
        drop(self.stdin);
        assert!(self.child.wait().expect("wait host").success());
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("host stderr")
            .read_to_string(&mut stderr)
            .expect("read host stderr");
        assert!(stderr.is_empty(), "unexpected host stderr: {stderr}");
    }
}

fn operation(response: HostResponse) -> Value {
    match response.response {
        HostResponseStatus::Ok { result } => {
            let value = serde_json::to_value(result).expect("host result JSON");
            assert_eq!(value["type"], "invoke");
            value["data"].clone()
        }
        other => panic!("expected outer host success, got {other:?}"),
    }
}

fn temp_directory(label: &str) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "double-love-host-slice2-{label}-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

fn assert_operation_result_shape(result: &Value) {
    let object = result.as_object().expect("operation result object");
    for field in [
        "status",
        "revision",
        "data",
        "counts",
        "diagnostics",
        "outputs",
    ] {
        assert!(object.contains_key(field), "operation result has {field}");
    }
    for field in ["total", "processed", "skipped", "failed", "unmatched"] {
        assert!(result["counts"][field].is_number(), "counts.{field} exists");
    }
    assert!(result["diagnostics"].is_array());
    assert!(result["outputs"].is_array());
}

fn diagnostic_code(result: &Value) -> &str {
    result["diagnostics"][0]["code"]
        .as_str()
        .expect("diagnostic code")
}

fn changed_canvas() -> CanvasSpec {
    CanvasSpec {
        width: 1280,
        height: 720,
        background: "#112233".to_string(),
        fit: CanvasFit::Cover,
        position_x: 10.0,
        position_y: -5.0,
        scale: 1.25,
        rotation_degrees: 2.0,
        opacity: 0.8,
    }
}

#[test]
fn existing_project_is_backed_up_before_service_open_and_create() {
    let root = temp_directory("pre-electron-backup");
    let project = root.join("open-project");
    let app_data = root.join("app-data");
    fs::create_dir_all(&root).expect("temporary root");
    let seeded = create_project(&project).expect("engine creates pre-existing project");
    let database = PathBuf::from(&seeded.database);
    let backup = project
        .join(".doublelove")
        .join("project.pre-electron-backup.sqlite");
    let mut host = HostProcess::spawn(&app_data);

    let opened = host.invoke("open-seeded", "project_open", json!({"path": project}));
    assert_eq!(opened["status"], "success");
    assert_eq!(opened["data"]["project_id"], seeded.project_id);
    assert_eq!(opened["data"]["revision"], 0);
    assert!(backup.is_file());

    let source_read_only = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("source opens read-only after service open");
    let source_schema: u32 = source_read_only
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("source schema version");
    assert_eq!(source_schema, 10);
    drop(source_read_only);

    let backup_read_only = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("backup opens read-only");
    let migration_table_count: u32 = backup_read_only
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |row| row.get(0),
        )
        .expect("schema migration table lookup");
    assert_eq!(migration_table_count, 1);
    let backup_schema: u32 = backup_read_only
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("backup schema version");
    assert_eq!(backup_schema, 10);
    let backup_revision: u64 = backup_read_only
        .query_row(
            "SELECT COALESCE(MAX(revision), 0) FROM revisions",
            [],
            |row| row.get(0),
        )
        .expect("backup revision");
    assert_eq!(backup_revision, 0);
    drop(backup_read_only);
    let first_backup_bytes = fs::read(&backup).expect("first backup bytes");

    let changed = host.invoke(
        "change-source",
        "canvas_set",
        json!({"canvas": changed_canvas()}),
    );
    assert_eq!(changed["revision"], 1);
    let reopened = host.invoke("reopen-seeded", "project_open", json!({"path": project}));
    assert_eq!(reopened["data"]["revision"], 1);
    assert_eq!(
        fs::read(&backup).expect("backup after reopen"),
        first_backup_bytes
    );

    let fallback = ProjectStore::open(&backup).expect("Tauri-equivalent ProjectStore open");
    assert_eq!(
        fallback.project_id().expect("backup project id").as_deref(),
        Some(seeded.project_id.as_str())
    );
    assert_eq!(fallback.revision().expect("backup revision"), 0);
    drop(fallback);

    let create_root = root.join("create-project");
    let create_seeded = create_project(&create_root).expect("engine creates second project");
    let created = host.invoke(
        "create-over-existing",
        "project_create",
        json!({"path": create_root}),
    );
    assert_eq!(created["status"], "success");
    assert_eq!(created["data"]["project_id"], create_seeded.project_id);
    assert_eq!(
        ProjectStore::open(
            &create_root
                .join(".doublelove")
                .join("project.pre-electron-backup.sqlite")
        )
        .expect("create handler backup opens")
        .revision()
        .expect("create handler backup revision"),
        0
    );

    host.stop();
    fs::remove_dir_all(root).expect("remove temporary root");
}

#[test]
fn project_lifecycle_history_restore_and_recent_project_round_trip() {
    let root = temp_directory("lifecycle");
    let project = root.join("project");
    let app_data = root.join("app-data");
    fs::create_dir_all(&root).expect("temporary root");
    let mut host = HostProcess::spawn(&app_data);

    let not_open = host.invoke("closed", "project_revision", json!({}));
    assert_eq!(not_open["status"], "failed");
    assert_eq!(diagnostic_code(&not_open), "PROJECT_NOT_OPEN");

    let created = host.invoke("create", "project_create", json!({"path": project}));
    assert_eq!(created["status"], "success");
    assert_eq!(created["data"]["revision"], 1);
    let project_id = created["data"]["project_id"]
        .as_str()
        .expect("project id")
        .to_string();

    let initial_history = host.invoke("initial-history", "project_history", json!({}));
    assert_eq!(initial_history["data"][0]["revision"], 1);
    assert_eq!(
        initial_history["data"][0]["operation"],
        "subtitle_style_set"
    );
    assert_eq!(initial_history["data"][0]["restorable"], true);

    let canvas = changed_canvas();
    let changed = host.invoke("canvas-set", "canvas_set", json!({"canvas": canvas}));
    assert_eq!(changed["status"], "success");
    assert_eq!(changed["revision"], 2);
    assert_eq!(changed["data"]["width"], 1280);
    assert_eq!(
        host.invoke("revision", "project_revision", json!({}))["data"],
        2
    );

    let history = host.invoke("history", "project_history", json!({"limit": 1}));
    assert_eq!(history["data"].as_array().expect("history").len(), 1);
    assert_eq!(history["data"][0]["operation"], "canvas_set");
    assert_eq!(history["data"][0]["restorable"], true);

    let undo = host.invoke("undo", "edit_undo", json!({}));
    assert_eq!(undo["status"], "success");
    assert_eq!(undo["revision"], 3);
    assert_eq!(
        host.invoke("canvas-after-undo", "canvas_get", json!({}))["data"]["width"],
        1920
    );
    let redo = host.invoke("redo", "edit_redo", json!({}));
    assert_eq!(redo["status"], "success");
    assert_eq!(redo["revision"], 4);
    assert_eq!(
        host.invoke("canvas-after-redo", "canvas_get", json!({}))["data"]["width"],
        1280
    );

    let restored = host.invoke(
        "restore",
        "project_restore_revision",
        json!({"revision": 1}),
    );
    assert_eq!(restored["status"], "success");
    assert_eq!(restored["revision"], 5);
    assert_eq!(
        restored["data"],
        json!({"restored_revision": 1, "revision": 5})
    );

    let reopened = host.invoke("reopen", "project_open", json!({"path": project}));
    assert_eq!(reopened["status"], "success");
    assert_eq!(reopened["data"]["project_id"], project_id);
    assert_eq!(reopened["data"]["revision"], 5);

    let invalid_root = root.join("invalid-project");
    fs::create_dir_all(&invalid_root).expect("invalid project directory");
    let invalid = host.invoke(
        "invalid-open",
        "project_open",
        json!({"path": invalid_root}),
    );
    assert_eq!(invalid["status"], "failed");
    assert_eq!(diagnostic_code(&invalid), "PROJECT_OPEN_FAILED");
    assert_eq!(
        host.invoke("still-open", "project_revision", json!({}))["data"],
        5
    );

    let file_path = root.join("not-a-directory");
    fs::write(&file_path, b"fixture").expect("blocking file");
    let invalid_create = host.invoke(
        "invalid-create",
        "project_create",
        json!({"path": file_path}),
    );
    assert_eq!(diagnostic_code(&invalid_create), "PROJECT_CREATE_FAILED");

    let recent = host.invoke("recent", "recent_projects_list", json!({}));
    assert_eq!(recent["status"], "success");
    assert_eq!(recent["data"].as_array().expect("recent projects").len(), 1);
    assert_eq!(recent["data"][0]["project_id"], project_id);

    host.stop();
    let mut restarted = HostProcess::spawn(&app_data);
    let opened_after_restart = restarted.invoke(
        "open-after-restart",
        "project_open",
        json!({"path": project}),
    );
    assert_eq!(opened_after_restart["data"]["project_id"], project_id);
    assert_eq!(opened_after_restart["data"]["revision"], 5);
    restarted.stop();
    fs::remove_dir_all(root).expect("remove temporary root");
}
