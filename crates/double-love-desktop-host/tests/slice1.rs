use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use double_love_desktop_host::{
    framing::{read_frame, write_frame},
    protocol::{HostEvent, HostRequest, HostRequestMethod, HostResponse, HostResponseStatus},
};
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

    fn invoke(&mut self, id: &str, name: &str, payload: Value) -> (HostResponse, Vec<HostEvent>) {
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

        let mut events = Vec::new();
        loop {
            let frame = read_frame(&mut self.stdout)
                .expect("read host frame")
                .expect("host frame before EOF");
            let value: Value = serde_json::from_slice(&frame).expect("host JSON frame");
            if value.get("event").is_some() {
                events.push(serde_json::from_value(value).expect("host event"));
                continue;
            }
            let response: HostResponse = serde_json::from_value(value).expect("host response");
            assert_eq!(response.id, id);
            return (response, events);
        }
    }

    fn stop(mut self) {
        let request = HostRequest::new("shutdown", HostRequestMethod::Shutdown);
        write_frame(
            &mut self.stdin,
            &serde_json::to_vec(&request).expect("serialize shutdown"),
        )
        .expect("write shutdown");
        let frame = read_frame(&mut self.stdout)
            .expect("read shutdown")
            .expect("shutdown response");
        let response: HostResponse = serde_json::from_slice(&frame).expect("shutdown JSON");
        assert_eq!(response.id, "shutdown");
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

fn temp_directory(label: &str) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "double-love-host-slice1-{label}-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
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

fn diagnostic_code(result: &Value) -> &str {
    result["diagnostics"][0]["code"]
        .as_str()
        .expect("diagnostic code")
}

#[test]
fn preferences_onboarding_profile_catalog_and_events_round_trip() {
    let app_data = temp_directory("round-trip");
    let mut host = HostProcess::spawn(&app_data);

    let (response, events) = host.invoke("get-defaults", "preferences_get", json!({}));
    assert!(events.is_empty());
    let defaults = operation(response);
    assert_eq!(defaults["status"], "success");
    assert_eq!(defaults["data"]["theme"], "light");
    assert_eq!(defaults["data"]["restore_last_project"], true);
    assert_eq!(defaults["data"]["model_endpoint"], "https://huggingface.co");
    assert_eq!(
        defaults["data"]["model_root"],
        app_data.join("models").to_string_lossy().as_ref()
    );
    let persisted: Value = serde_json::from_slice(
        &fs::read(app_data.join("preferences.json")).expect("persisted preferences"),
    )
    .expect("preferences JSON");
    assert!(
        !app_data
            .join("preferences.json.pre-electron-backup")
            .exists(),
        "an absent preferences store must not produce a backup"
    );
    assert_eq!(persisted.as_object().expect("store object").len(), 1);
    assert_eq!(persisted["app_preferences"], defaults["data"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(app_data.join("preferences.json"))
                .expect("preferences metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    let (response, events) = host.invoke(
        "update-theme",
        "preferences_update",
        json!({"patch": {"theme": "dark"}}),
    );
    let updated = operation(response);
    assert_eq!(updated["status"], "success");
    assert_eq!(updated["data"]["theme"], "dark");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "dl://preferences-changed");
    assert_eq!(events[0].payload, json!({"changed_keys": ["theme"]}));

    let invalid = operation(
        host.invoke(
            "invalid-endpoint",
            "preferences_update",
            json!({"patch": {"model_endpoint": "http://example.test"}}),
        )
        .0,
    );
    assert_eq!(invalid["status"], "failed");
    assert_eq!(diagnostic_code(&invalid), "MODEL_ENDPOINT_INVALID");

    let blocked_parent = app_data.join("not-a-directory");
    fs::write(&blocked_parent, b"fixture").expect("blocking file");
    let failed_migration = operation(
        host.invoke(
            "failed-migration",
            "preferences_update",
            json!({"patch": {"model_root": blocked_parent.join("models")}}),
        )
        .0,
    );
    assert_eq!(failed_migration["status"], "failed");
    assert_eq!(
        diagnostic_code(&failed_migration),
        "MODEL_ROOT_MIGRATION_FAILED"
    );
    let after_failed_migration = operation(
        host.invoke("get-after-failed-migration", "preferences_get", json!({}))
            .0,
    );
    assert_eq!(
        after_failed_migration["data"]["model_root"],
        defaults["data"]["model_root"]
    );

    let completed = operation(
        host.invoke(
            "onboarding-complete",
            "onboarding_complete",
            json!({"defaultAsrModel": "qwen3-asr-0.6b", "step": 3}),
        )
        .0,
    );
    assert_eq!(
        completed["data"],
        json!({"version": 1, "completed": true, "step": 3})
    );
    let reset = operation(
        host.invoke("onboarding-reset", "onboarding_reset", json!({}))
            .0,
    );
    assert_eq!(
        reset["data"],
        json!({"version": 1, "completed": false, "step": 1})
    );
    let invalid_step = operation(
        host.invoke(
            "onboarding-invalid",
            "onboarding_complete",
            json!({"step": 4}),
        )
        .0,
    );
    assert_eq!(diagnostic_code(&invalid_step), "ONBOARDING_STEP_INVALID");

    let profile = operation(host.invoke("profile", "system_profile", json!({})).0);
    assert_eq!(profile["status"], "success");
    assert_eq!(profile["data"]["architecture"], "arm64");
    assert!(matches!(
        profile["data"]["recommended_asr_model"].as_str(),
        Some("qwen3-asr-0.6b" | "qwen3-asr-1.7b")
    ));

    let catalog = operation(host.invoke("catalog", "model_catalog", json!({})).0);
    let silero = catalog["data"]
        .as_array()
        .expect("model catalog")
        .iter()
        .find(|model| model["descriptor"]["id"] == "silero-vad")
        .expect("bundled silero-vad");
    assert_eq!(silero["installation"]["state"], "installed");

    let new_model_root = app_data.join("relocated-models");
    let (response, events) = host.invoke(
        "relocate-models",
        "preferences_update",
        json!({"patch": {"model_root": new_model_root}}),
    );
    let relocated = operation(response);
    assert_eq!(relocated["status"], "success");
    assert_eq!(
        relocated["data"]["model_root"],
        new_model_root.to_string_lossy().as_ref()
    );
    assert_eq!(events[0].payload, json!({"changed_keys": ["model_root"]}));
    let relocated_catalog = operation(
        host.invoke("relocated-catalog", "model_catalog", json!({}))
            .0,
    );
    assert!(
        relocated_catalog["data"]
            .as_array()
            .expect("relocated catalog")
            .iter()
            .any(|model| {
                model["descriptor"]["id"] == "silero-vad"
                    && model["installation"]["state"] == "installed"
            })
    );

    host.stop();
    fs::remove_dir_all(app_data).expect("remove app data");
}

#[test]
fn preexisting_preferences_get_one_unchanging_pre_electron_backup() {
    let app_data = temp_directory("pre-electron-backup");
    fs::create_dir_all(&app_data).expect("app data");
    let fixture =
        include_bytes!("../../double-love-desktop-service/tests/fixtures/preferences/v1.json");
    fs::write(app_data.join("preferences.json"), fixture).expect("preferences fixture");
    let backup = app_data.join("preferences.json.pre-electron-backup");
    let mut host = HostProcess::spawn(&app_data);

    let first = operation(host.invoke("first-get", "preferences_get", json!({})).0);
    assert_eq!(first["status"], "success");
    assert_eq!(first["data"]["theme"], "dark");
    assert_eq!(fs::read(&backup).expect("first backup"), fixture);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&backup)
                .expect("backup metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    let updated = operation(
        host.invoke(
            "change-after-backup",
            "preferences_update",
            json!({"patch": {"theme": "light"}}),
        )
        .0,
    );
    assert_eq!(updated["data"]["theme"], "light");
    let second = operation(host.invoke("second-get", "preferences_get", json!({})).0);
    assert_eq!(second["data"]["theme"], "light");
    assert_eq!(fs::read(&backup).expect("unchanged backup"), fixture);

    host.stop();
    fs::remove_dir_all(app_data).expect("remove app data");
}

#[test]
fn corrupt_preferences_recover_with_warning_and_backup() {
    let app_data = temp_directory("corrupt");
    fs::create_dir_all(&app_data).expect("app data");
    let corrupt =
        include_bytes!("../../double-love-desktop-service/tests/fixtures/preferences/corrupt.json");
    fs::write(app_data.join("preferences.json"), corrupt).expect("corrupt fixture");
    let mut host = HostProcess::spawn(&app_data);

    let recovered = operation(host.invoke("recover", "preferences_get", json!({})).0);
    assert_eq!(recovered["status"], "success");
    assert_eq!(diagnostic_code(&recovered), "PREFERENCES_RECOVERED");
    assert!(
        fs::read_dir(&app_data)
            .expect("app data entries")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("preferences.corrupt."))
    );
    assert_eq!(
        fs::read(app_data.join("preferences.json.pre-electron-backup"))
            .expect("pre-Electron backup survives recovery"),
        corrupt
    );

    host.stop();
    fs::remove_dir_all(app_data).expect("remove app data");
}

#[test]
fn recent_projects_are_capped_and_missing_forget_is_explicit() {
    let app_data = temp_directory("recent");
    let mut host = HostProcess::spawn(&app_data);
    let defaults = operation(host.invoke("defaults", "preferences_get", json!({})).0);
    host.stop();

    let mut preferences = defaults["data"].clone();
    preferences["recent_projects"] = Value::Array(
        (0..25)
            .map(|index| {
                json!({
                    "project_id": format!("project-{index}"),
                    "root": format!("/tmp/double-love-project-{index}"),
                    "display_name": format!("Project {index}"),
                    "last_opened_at": format!("2026-01-01T00:00:{index:02}Z")
                })
            })
            .collect(),
    );
    fs::write(
        app_data.join("preferences.json"),
        serde_json::to_vec_pretty(&json!({"app_preferences": preferences}))
            .expect("serialize recent fixture"),
    )
    .expect("write recent fixture");

    let mut host = HostProcess::spawn(&app_data);
    let projects = operation(
        host.invoke("recent-list", "recent_projects_list", json!({}))
            .0,
    );
    assert_eq!(projects["status"], "success");
    assert_eq!(
        projects["data"].as_array().expect("recent projects").len(),
        20
    );

    let (response, events) = host.invoke(
        "recent-forget",
        "recent_project_forget",
        json!({"root": "/tmp/double-love-project-0"}),
    );
    let forgotten = operation(response);
    assert_eq!(forgotten["status"], "success");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "dl://preferences-changed");
    assert_eq!(
        events[0].payload,
        json!({"changed_keys": ["recent_projects"]})
    );

    let missing = operation(
        host.invoke(
            "recent-missing",
            "recent_project_forget",
            json!({"root": "/tmp/not-in-recents"}),
        )
        .0,
    );
    assert_eq!(missing["status"], "failed");
    assert_eq!(diagnostic_code(&missing), "RECENT_PROJECT_NOT_FOUND");

    host.stop();
    fs::remove_dir_all(app_data).expect("remove app data");
}
