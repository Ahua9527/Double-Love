use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use double_love_desktop_host::{
    framing::{read_frame, write_frame},
    protocol::{HostRequest, HostRequestMethod, HostResponse, HostResponseStatus},
};
use double_love_engine::{FfmpegTools, ProjectStore};
use serde_json::{Value, json};

struct HostProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    events: Vec<Value>,
}

impl HostProcess {
    fn spawn(app_data_dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_double-love-desktop-host"))
            .args(["--app-data-dir"])
            .arg(app_data_dir)
            .args(["--resource-dir"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .arg("--test-transcribe-mock")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn desktop host");
        Self {
            stdin: child.stdin.take().expect("host stdin"),
            stdout: child.stdout.take().expect("host stdout"),
            child,
            events: Vec::new(),
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
        .expect("write request");
        loop {
            let frame = read_frame(&mut self.stdout)
                .expect("read frame")
                .expect("frame before EOF");
            let value: Value = serde_json::from_slice(&frame).expect("frame JSON");
            if value.get("event").is_some() {
                self.events.push(value);
                continue;
            }
            let response: HostResponse = serde_json::from_value(value).expect("response");
            assert_eq!(response.id, id);
            return match response.response {
                HostResponseStatus::Ok { result } => {
                    let value = serde_json::to_value(result).expect("result JSON");
                    assert_eq!(value["type"], "invoke");
                    value["data"].clone()
                }
                other => panic!("expected outer host success, got {other:?}"),
            };
        }
    }

    fn wait_task_state(&mut self, task_id: &str, terminal: &str) {
        for _ in 0..1000 {
            if self.events.iter().any(|event| {
                event["event"] == "dl://task-state"
                    && event["payload"]["task_id"] == task_id
                    && event["payload"]["state"] == terminal
            }) {
                return;
            }
            let frame = read_frame(&mut self.stdout)
                .expect("read event")
                .expect("event before EOF");
            let value: Value = serde_json::from_slice(&frame).expect("event JSON");
            assert!(
                value.get("event").is_some(),
                "unexpected response: {value:#}"
            );
            self.events.push(value);
        }
        panic!("task {task_id} did not reach {terminal}");
    }

    fn stop(mut self) {
        let request = HostRequest::new("shutdown", HostRequestMethod::Shutdown);
        write_frame(
            &mut self.stdin,
            &serde_json::to_vec(&request).expect("serialize shutdown"),
        )
        .expect("write shutdown");
        loop {
            let frame = read_frame(&mut self.stdout)
                .expect("read shutdown")
                .expect("shutdown frame");
            let value: Value = serde_json::from_slice(&frame).expect("shutdown JSON");
            if value.get("event").is_none() {
                break;
            }
        }
        drop(self.stdin);
        assert!(self.child.wait().expect("wait host").success());
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("read stderr");
        assert!(stderr.is_empty(), "unexpected host stderr: {stderr}");
    }
}

fn temp_directory(label: &str) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "double-love-host-slice5-{label}-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

fn missing_test_tools(reason: &str) {
    assert!(
        std::env::var("DOUBLELOVE_REQUIRE_TEST_TOOLS").as_deref() != Ok("1"),
        "required test tools unavailable: {reason}"
    );
    eprintln!("skip: {reason}");
}

fn generate_media(tools: &FfmpegTools, path: &Path, duration: u32) {
    let status = Command::new(&tools.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=black:s=320x180:r=25:d={duration}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:sample_rate=48000:duration={duration}"),
            "-c:v",
            "mpeg4",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(path)
        .status()
        .expect("generate media");
    assert!(status.success(), "ffmpeg fixture generation failed");
}

fn seed_installed_transcription_models(app_data: &Path) {
    let model_root = app_data.join("models");
    fs::create_dir_all(&model_root).expect("model root");
    let installed = |model_id: &str, revision: &str| {
        json!({
            "model_id": model_id,
            "revision": revision,
            "state": "installed",
            "bytes_downloaded": 0,
            "bytes_total": 0,
            "staging_id": null,
            "last_error_code": null,
            "last_error_message": null,
            "updated_at": "2026-01-01T00:00:00Z"
        })
    };
    fs::write(
        model_root.join("installations.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "installations": {
                "qwen3-asr-0.6b": installed(
                    "qwen3-asr-0.6b",
                    "5eb144179a02acc5e5ba31e748d22b0cf3e303b0"
                ),
                "qwen3-forced-aligner-0.6b": installed(
                    "qwen3-forced-aligner-0.6b",
                    "c7cbfc2048c462b0d63a45797104fc9db3ad62b7"
                )
            }
        }))
        .expect("installation JSON"),
    )
    .expect("installation state");
}

fn diagnostic_code(result: &Value) -> &str {
    result["diagnostics"][0]["code"]
        .as_str()
        .expect("diagnostic code")
}

fn assert_success(result: &Value) {
    assert_eq!(result["status"], "success", "{result:#}");
}

#[test]
fn progress_events_sanitize_project_paths_from_mock_sidecar_errors() {
    let tools = match FfmpegTools::discover() {
        Ok(tools) => tools,
        Err(_) => {
            missing_test_tools("ffmpeg/ffprobe unavailable");
            return;
        }
    };
    if Command::new("python3").arg("--version").output().is_err() {
        missing_test_tools("python3 unavailable");
        return;
    }

    let root = temp_directory("progress-sanitization");
    let app_data = root.join("app-data");
    let project = root.join("project");
    let media = root.join("fixture.mp4");
    fs::create_dir_all(&root).expect("temporary root");
    generate_media(&tools, &media, 1);
    seed_installed_transcription_models(&app_data);

    let mut host = HostProcess::spawn(&app_data);
    assert_success(&host.invoke("create", "project_create", json!({"path":project})));
    let imported = host.invoke("import", "import_media", json!({"path":media}));
    assert_success(&imported);
    let asset_id = imported["data"]["id"].as_str().expect("asset id");
    let prepared_wav = ProjectStore::open(&project.join(".doublelove/project.sqlite"))
        .expect("open project store")
        .media_asset(asset_id)
        .expect("read media asset")
        .expect("media asset")
        .prepared_wav_path
        .expect("prepared wav path");
    fs::write(&prepared_wav, b"corrupt wav").expect("corrupt prepared wav");

    let started = host.invoke(
        "transcribe",
        "transcribe_start",
        json!({"asset_id":asset_id,"model":"qwen3-asr-0.6b","language":"auto"}),
    );
    assert_success(&started);
    let task_id = started["data"]["task_id"]
        .as_str()
        .expect("task id")
        .to_string();
    host.wait_task_state(&task_id, "failed");

    let progress = host
        .events
        .iter()
        .find(|event| {
            event["event"] == "dl://progress"
                && event["payload"]["task"] == task_id
                && event["payload"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("ASR_BAD_WAV"))
        })
        .expect("path-bearing ASR error progress event");
    let message = progress["payload"]["message"]
        .as_str()
        .expect("progress message");
    assert!(message.contains("<PROJECT>"), "{message}");
    assert!(!message.contains(&project.to_string_lossy().into_owned()));
    assert!(!message.contains(&prepared_wav));
    assert!(!message.contains(&root.to_string_lossy().into_owned()));
    assert!(host.events.iter().any(|event| {
        event["event"] == "dl://task-state"
            && event["payload"]["task_id"] == task_id
            && event["payload"]["state"] == "failed"
    }));

    host.stop();
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn transcription_edits_roughcut_and_model_diagnostics_match_tauri_semantics() {
    let tools = match FfmpegTools::discover() {
        Ok(tools) => tools,
        Err(_) => {
            missing_test_tools("ffmpeg/ffprobe unavailable");
            return;
        }
    };
    if Command::new("python3").arg("--version").output().is_err() {
        missing_test_tools("python3 unavailable");
        return;
    }

    let root = temp_directory("commands");
    let app_data = root.join("app-data");
    let project = root.join("project");
    let media = root.join("fixture.mp4");
    let export = root.join("rough-cut.xml");
    fs::create_dir_all(&root).expect("temporary root");
    generate_media(&tools, &media, 61);
    seed_installed_transcription_models(&app_data);

    let mut host = HostProcess::spawn(&app_data);
    for command in [
        "transcript_get",
        "roughcut_preview",
        "edit_omit",
        "edit_restore",
        "export_roughcut_apply",
    ] {
        let payload = match command {
            "edit_omit" => json!({"asset_id":"missing","start_ordinal":0,"end_ordinal":0}),
            "edit_restore" => json!({"operation_id":"missing","start_ordinal":0,"end_ordinal":0}),
            "export_roughcut_apply" => json!({"asset_id":"missing","target_path":export}),
            _ => json!({"asset_id":"missing"}),
        };
        let result = host.invoke(&format!("closed-{command}"), command, payload);
        assert_eq!(result["status"], "failed");
        assert_eq!(diagnostic_code(&result), "PROJECT_NOT_OPEN");
    }

    let reveal_root = host.invoke("reveal-root", "model_reveal", json!({}));
    assert_success(&reveal_root);
    assert_eq!(
        reveal_root["data"]["path"],
        app_data.join("models").to_string_lossy().as_ref()
    );
    assert!(app_data.join("models").is_dir());
    let reveal_model = host.invoke(
        "reveal-model",
        "model_reveal",
        json!({"model_id":"qwen3-asr-0.6b"}),
    );
    assert_success(&reveal_model);
    assert!(
        reveal_model["data"]["path"]
            .as_str()
            .expect("path")
            .ends_with("qwen3-asr-0.6b/5eb144179a02acc5e5ba31e748d22b0cf3e303b0")
    );
    let logs = host.invoke("logs", "diagnostics_reveal_logs", json!({}));
    assert_success(&logs);
    assert_eq!(
        logs["data"]["path"],
        app_data.join("logs").to_string_lossy().as_ref()
    );
    assert!(app_data.join("logs").is_dir());

    let doctor = host.invoke("doctor", "doctor_run", json!({}));
    assert_success(&doctor);
    assert_eq!(doctor["data"]["schema_version"], 1);
    assert!(
        host.events
            .iter()
            .any(|event| event["event"] == "dl://doctor-result")
    );

    assert_success(&host.invoke("create", "project_create", json!({"path":project})));
    let imported = host.invoke("import", "import_media", json!({"path":media}));
    assert_success(&imported);
    let asset_id = imported["data"]["id"]
        .as_str()
        .expect("asset id")
        .to_string();

    let started = host.invoke(
        "transcribe",
        "transcribe_start",
        json!({"asset_id":asset_id,"model":"qwen3-asr-0.6b","language":"auto"}),
    );
    assert_success(&started);
    let task_id = started["data"]["task_id"]
        .as_str()
        .expect("task id")
        .to_string();
    host.wait_task_state(&task_id, "succeeded");
    assert!(
        host.events.iter().any(|event| {
            event["event"] == "dl://progress" && event["payload"]["task"] == task_id
        })
    );

    let transcript = host.invoke("transcript", "transcript_get", json!({"asset_id":asset_id}));
    assert_success(&transcript);
    let word_count = transcript["data"]["words"].as_array().expect("words").len();
    assert!(word_count >= 4);

    let omitted = host.invoke(
        "omit",
        "edit_omit",
        json!({"asset_id":asset_id,"start_ordinal":1,"end_ordinal":2}),
    );
    assert_success(&omitted);
    assert_eq!(omitted["data"]["handles_before_ms"], 120);
    assert_eq!(omitted["data"]["handles_after_ms"], 120);
    let operation_id = omitted["data"]["id"].as_str().expect("operation id");
    let preview_revision = host.invoke("preview-revision", "project_revision", json!({}))["data"]
        .as_u64()
        .expect("revision");
    let preview = host.invoke("preview", "roughcut_preview", json!({"asset_id":asset_id}));
    assert_success(&preview);
    assert!(preview["data"]["artifact_path"].is_null());
    assert!(preview["data"]["sha256"].is_null());
    let preview_exports = project.join(".doublelove/exports");
    assert!(
        !preview_exports.exists()
            || fs::read_dir(&preview_exports)
                .expect("preview exports")
                .next()
                .is_none(),
        "preview must not write an artifact"
    );
    assert_eq!(
        host.invoke("preview-revision-after", "project_revision", json!({}))["data"],
        preview_revision
    );

    let applied = host.invoke(
        "apply",
        "export_roughcut_apply",
        json!({"asset_id":asset_id,"target_path":export}),
    );
    assert_success(&applied);
    assert_eq!(applied["outputs"][0]["kind"], "premiere_xmeml");
    assert_eq!(
        applied["outputs"][0]["path"],
        export.to_string_lossy().as_ref()
    );
    assert_eq!(applied["outputs"][0]["sha256"], applied["data"]["sha256"]);
    assert_eq!(
        applied["data"]["artifact_path"],
        export.to_string_lossy().as_ref()
    );
    assert_eq!(applied["data"]["sha256"].as_str().expect("sha").len(), 64);
    assert!(
        fs::read_to_string(&export)
            .expect("XMEML")
            .contains("<!DOCTYPE xmeml>")
    );
    let history = host.invoke("history", "project_history", json!({"limit":10}));
    assert!(
        history["data"]
            .as_array()
            .expect("history")
            .iter()
            .any(|entry| { entry["operation"] == "export_roughcut" })
    );

    let restored = host.invoke(
        "restore",
        "edit_restore",
        json!({"operation_id":operation_id,"start_ordinal":1,"end_ordinal":2}),
    );
    assert_success(&restored);
    assert!(
        host.invoke(
            "transcript-restored",
            "transcript_get",
            json!({"asset_id":asset_id})
        )["data"]["omits"]
            .as_array()
            .expect("omits")
            .is_empty()
    );

    let all_omitted = host.invoke(
        "omit-all",
        "edit_omit",
        json!({
            "asset_id":asset_id,
            "start_ordinal":0,
            "end_ordinal":i64::try_from(word_count).expect("word count") - 1,
            "handles_before_ms":120,
            "handles_after_ms":120
        }),
    );
    assert_success(&all_omitted);
    let empty_target = root.join("empty.xml");
    let empty = host.invoke(
        "empty-apply",
        "export_roughcut_apply",
        json!({"asset_id":asset_id,"target_path":empty_target}),
    );
    assert_eq!(empty["status"], "failed");
    assert_eq!(diagnostic_code(&empty), "ROUGH_CUT_EMPTY");
    assert!(
        empty["diagnostics"][0]["blocks_export"]
            .as_bool()
            .expect("blocks")
    );
    assert!(!root.join("empty.xml").exists());

    // Start a replacement candidate and cancel it before it can replace the active transcript.
    let before_cancel = ProjectStore::open(&project.join(".doublelove/project.sqlite"))
        .expect("open store")
        .active_transcript_run_id(&asset_id)
        .expect("active run")
        .expect("active run id");
    let cancel_started = host.invoke(
        "cancel-start",
        "transcribe_start",
        json!({"asset_id":asset_id,"model":"qwen3-asr-0.6b","language":"auto"}),
    );
    assert_success(&cancel_started);
    let cancel_task = cancel_started["data"]["task_id"]
        .as_str()
        .expect("cancel task")
        .to_string();
    let cancelled = host.invoke("cancel", "task_cancel", json!({"task_id":cancel_task}));
    assert_success(&cancelled);
    host.wait_task_state(&cancel_task, "cancelled");
    let after_cancel = ProjectStore::open(&project.join(".doublelove/project.sqlite"))
        .expect("open store")
        .active_transcript_run_id(&asset_id)
        .expect("active run")
        .expect("active run id");
    assert_eq!(after_cancel, before_cancel);
    let not_running = host.invoke(
        "cancel-again",
        "task_cancel",
        json!({"task_id":cancel_task}),
    );
    assert_eq!(diagnostic_code(&not_running), "TASK_NOT_RUNNING");

    let verify = host.invoke(
        "verify",
        "model_verify",
        json!({"model_id":"qwen3-asr-0.6b"}),
    );
    assert_eq!(verify["status"], "failed");
    assert_eq!(diagnostic_code(&verify), "MODEL_VERIFY_FAILED");
    assert_eq!(
        host.invoke("catalog", "model_catalog", json!({}))["data"]
            .as_array()
            .expect("catalog")
            .iter()
            .find(|item| item["descriptor"]["id"] == "qwen3-asr-0.6b")
            .expect("asr")["installation"]["state"],
        "corrupt"
    );

    std::thread::sleep(Duration::from_millis(50));
    host.stop();
    fs::remove_dir_all(root).expect("cleanup");
}
