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
use double_love_engine::{
    FfmpegTools, NewMediaAsset, NewTranscriptWord, ProjectStore, SpeakerAssignment, create_project,
};
use serde_json::{Value, json};

struct HostProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    events: Vec<Value>,
    responses: Vec<Value>,
}

impl HostProcess {
    fn spawn(app_data_dir: &Path) -> Self {
        Self::spawn_with(
            app_data_dir,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
            &["--test-transcribe-mock", "--test-speaker-mock"],
            false,
        )
    }

    fn spawn_with(
        app_data_dir: &Path,
        resource_dir: &Path,
        test_flags: &[&str],
        preset_mock_environment: bool,
    ) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_double-love-desktop-host"));
        command
            .args(["--app-data-dir"])
            .arg(app_data_dir)
            .args(["--resource-dir"])
            .arg(resource_dir)
            .args(test_flags)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if preset_mock_environment {
            command
                .env("DOUBLELOVE_ASR_MOCK", "1")
                .env("DOUBLELOVE_SPEAKER_MOCK", "1");
        }
        let mut child = command.spawn().expect("spawn desktop host");
        Self {
            stdin: child.stdin.take().expect("host stdin"),
            stdout: child.stdout.take().expect("host stdout"),
            child,
            events: Vec::new(),
            responses: Vec::new(),
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
                    let data = value["data"].clone();
                    self.responses.push(data.clone());
                    data
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
        "double-love-host-slice6-{label}-{}-{}",
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

fn generate_media(tools: &FfmpegTools, path: &Path, frequency: u32) {
    let status = Command::new(&tools.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=320x180:r=25:d=4",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={frequency}:sample_rate=48000:duration=4"),
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

fn seed_installed_models(app_data: &Path) {
    let model_root = app_data.join("models");
    fs::create_dir_all(&model_root).expect("model root");
    let installed = |model_id: &str, revision: &str| {
        fs::create_dir_all(model_root.join(model_id).join(revision)).expect("model directory");
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
                ),
                "silero-vad": installed(
                    "silero-vad",
                    "806dcba3f0b5d95282d0889a074954a2f8c6397b"
                ),
                "wespeaker-zh": installed(
                    "wespeaker-zh",
                    "f5a201849aa7cae741ec75cd02a0bc9dd5712ca2"
                )
            }
        }))
        .expect("installation JSON"),
    )
    .expect("installation state");
}

fn write_boundary_speaker_sidecar(resource_dir: &Path) -> PathBuf {
    let package_root = resource_dir.join("model-runtime/speaker");
    let package = package_root.join("double_love_speaker");
    fs::create_dir_all(&package).expect("boundary speaker package");
    fs::write(package.join("__init__.py"), "").expect("boundary speaker package init");
    fs::write(
        package.join("__main__.py"),
        r#"import json
import os
import sys
from pathlib import Path


def emit(event):
    sys.stdout.write(json.dumps(event, separators=(",", ":")) + "\n")
    sys.stdout.flush()


speaker_mock = os.environ.get("DOUBLELOVE_SPEAKER_MOCK") == "1"
asr_mock = os.environ.get("DOUBLELOVE_ASR_MOCK") == "1"
Path("observed-mock-env.txt").write_text(
    f"speaker={str(speaker_mock).lower()}\nasr={str(asr_mock).lower()}\n",
    encoding="utf-8",
)
for raw in sys.stdin:
    command = json.loads(raw)
    if command.get("cmd") == "hello":
        emit({"event": "ready", "version": 1, "pid": os.getpid(), "mock": speaker_mock})
    elif command.get("cmd") == "diarize" and speaker_mock:
        emit({
            "event": "speaker_segments",
            "task_id": command.get("task_id", ""),
            "segments": "malformed-on-purpose",
            "embeddings": [{
                "cluster_id": "private-cluster",
                "values": [
                    0.101001, -0.202002, 0.303003, -0.404004,
                    0.505005, -0.606006, 0.707007, -0.808008,
                    0.909009, -1.01001, 1.111011, -1.212012,
                ],
            }],
        })
    elif command.get("cmd") == "diarize":
        emit({
            "event": "error",
            "task_id": command.get("task_id", ""),
            "code": "SPEAKER_REAL_PATH",
            "message": "mock=false",
            "fatal": True,
        })
"#,
    )
    .expect("boundary speaker main");
    package_root.join("observed-mock-env.txt")
}

fn seed_boundary_speaker_project(project: &Path) {
    let summary = create_project(project).expect("create boundary project");
    let prepared_wav = project.join(".doublelove/prepared/boundary.wav");
    fs::create_dir_all(prepared_wav.parent().expect("prepared parent"))
        .expect("prepared directory");
    fs::write(&prepared_wav, b"sidecar fixture does not read this file")
        .expect("prepared placeholder");
    let store = ProjectStore::open(Path::new(&summary.database)).expect("open boundary store");
    store
        .insert_media_asset(&NewMediaAsset {
            id: "boundary-asset".to_string(),
            kind: "video".to_string(),
            original_path: project.join("boundary.mov").to_string_lossy().into_owned(),
            display_name: "boundary.mov".to_string(),
            duration_samples: 48_000,
            audio_sample_rate: 48_000,
            fps_num: 25,
            fps_den: 1,
            video_timebase: 25,
            is_ntsc: false,
            width: Some(320),
            height: Some(180),
            audio_channels: Some(1),
            source_tc_start_frame: None,
            source_tc_is_drop_frame: false,
            ffprobe_json: "{}".to_string(),
        })
        .expect("insert boundary asset");
    store
        .set_asset_prepared("boundary-asset", &prepared_wav.to_string_lossy())
        .expect("prepare boundary asset");
    store
        .insert_transcript_words(&[NewTranscriptWord {
            word_id: "boundary-word".to_string(),
            asset_id: "boundary-asset".to_string(),
            ordinal: 0,
            raw_text: "边界测试".to_string(),
            display_text: "边界测试".to_string(),
            language: Some("zh".to_string()),
            start_sample: 0,
            end_sample: 24_000,
            confidence: Some(0.99),
        }])
        .expect("insert boundary transcript");
}

fn start_boundary_diarization(host: &mut HostProcess, project: &Path, id: &str) -> String {
    assert_success(&host.invoke(
        &format!("{id}-open"),
        "project_open",
        json!({"path":project}),
    ));
    let started = host.invoke(
        &format!("{id}-start"),
        "speaker_diarize_start",
        json!({"asset_id":"boundary-asset"}),
    );
    assert_success(&started);
    started["data"]["task_id"]
        .as_str()
        .expect("boundary task id")
        .to_string()
}

fn diagnostic_code(result: &Value) -> &str {
    result["diagnostics"][0]["code"]
        .as_str()
        .expect("diagnostic code")
}

fn assert_success(result: &Value) {
    assert_eq!(result["status"], "success", "{result:#}");
}

fn start_and_wait(host: &mut HostProcess, command_id: &str, name: &str, payload: Value) -> String {
    let started = host.invoke(command_id, name, payload);
    assert_success(&started);
    let task_id = started["data"]["task_id"]
        .as_str()
        .expect("task id")
        .to_string();
    host.wait_task_state(&task_id, "succeeded");
    task_id
}

fn replace_first_transcript(
    database: &Path,
    asset_id: &str,
    project_root: &str,
    media_source: &str,
) -> String {
    let store = ProjectStore::open(database).expect("open project store");
    store
        .delete_transcript_words(asset_id)
        .expect("delete mock transcript words");
    let texts = [
        "我是李明。",
        project_root,
        media_source,
        "仅甲方发言可见。",
        "乙方秘密文本",
    ];
    let words = texts
        .iter()
        .enumerate()
        .map(|(ordinal, text)| NewTranscriptWord {
            word_id: format!("slice6-word-{ordinal}"),
            asset_id: asset_id.to_string(),
            ordinal: i64::try_from(ordinal).expect("ordinal"),
            raw_text: (*text).to_string(),
            display_text: (*text).to_string(),
            language: Some("zh".to_string()),
            start_sample: i64::try_from(ordinal).expect("ordinal") * 24_000,
            end_sample: i64::try_from(ordinal).expect("ordinal") * 24_000 + 14_400,
            confidence: Some(0.99),
        })
        .collect::<Vec<_>>();
    let other_word_id = words[4].word_id.clone();
    store
        .insert_transcript_words(&words)
        .expect("insert privacy-safe transcript words");
    other_word_id
}

fn contains_embedding_shaped_array(value: &Value) -> bool {
    match value {
        Value::Array(values) => {
            (values.len() >= 2 && values.iter().all(Value::is_number))
                || values.iter().any(contains_embedding_shaped_array)
        }
        Value::Object(values) => values.values().any(contains_embedding_shaped_array),
        _ => false,
    }
}

fn contains_embedding_field(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_embedding_field),
        Value::Object(values) => values.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "embedding" | "embeddings" | "embedding_values" | "values"
            ) || contains_embedding_field(value)
        }),
        _ => false,
    }
}

#[test]
fn production_host_scrubs_inherited_sidecar_mock_environment() {
    if Command::new("python3").arg("--version").output().is_err() {
        missing_test_tools("python3 unavailable");
        return;
    }

    let root = temp_directory("mock-environment");
    let app_data = root.join("app-data");
    let project = root.join("project");
    let resources = root.join("resources");
    seed_installed_models(&app_data);
    seed_boundary_speaker_project(&project);
    let observed_environment = write_boundary_speaker_sidecar(&resources);

    let mut host = HostProcess::spawn_with(&app_data, &resources, &[], true);
    let task_id = start_boundary_diarization(&mut host, &project, "production");
    host.wait_task_state(&task_id, "failed");

    assert_eq!(
        fs::read_to_string(&observed_environment).expect("observed child environment"),
        "speaker=false\nasr=false\n"
    );
    assert!(host.events.iter().any(|event| {
        event["event"] == "dl://progress"
            && event["payload"]["task"] == task_id
            && event["payload"]["message"] == "SPEAKER_REAL_PATH: mock=false"
    }));

    host.stop();
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn malformed_speaker_sidecar_embeddings_are_redacted_from_progress_events() {
    if Command::new("python3").arg("--version").output().is_err() {
        missing_test_tools("python3 unavailable");
        return;
    }

    let root = temp_directory("malformed-speaker");
    let app_data = root.join("app-data");
    let project = root.join("project");
    let resources = root.join("resources");
    seed_installed_models(&app_data);
    seed_boundary_speaker_project(&project);
    let observed_environment = write_boundary_speaker_sidecar(&resources);

    let mut host = HostProcess::spawn_with(&app_data, &resources, &["--test-speaker-mock"], false);
    let task_id = start_boundary_diarization(&mut host, &project, "malformed");
    host.wait_task_state(&task_id, "failed");

    assert_eq!(
        fs::read_to_string(&observed_environment).expect("observed child environment"),
        "speaker=true\nasr=true\n",
        "the explicit test flag must keep the deterministic mock path working"
    );
    let progress = host
        .events
        .iter()
        .find(|event| {
            event["event"] == "dl://progress"
                && event["payload"]["task"] == task_id
                && event["payload"]["phase"] == "error"
                && event["payload"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("sidecar 协议错误"))
        })
        .expect("malformed sidecar progress event");
    let message = progress["payload"]["message"]
        .as_str()
        .expect("malformed progress message");
    assert!(message.contains("<REDACTED>"), "{message}");
    assert!(message.len() <= 4096, "progress text was not capped");
    for leaked in [
        "0.101001",
        "-0.202002",
        "0.303003",
        "-0.404004",
        "0.909009",
        "-1.212012",
    ] {
        assert!(
            !message.contains(leaked),
            "embedding value leaked: {message}"
        );
    }
    assert!(!contains_embedding_shaped_array(progress));

    host.stop();
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn speaker_commands_match_tauri_and_keep_embeddings_inside_the_project_database() {
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
    let first_media = root.join("first.mp4");
    let second_media = root.join("second.mp4");
    fs::create_dir_all(&root).expect("temporary root");
    generate_media(&tools, &first_media, 440);
    generate_media(&tools, &second_media, 660);

    let mut unready_host = HostProcess::spawn(&app_data);
    let model_not_ready = unready_host.invoke(
        "model-not-ready",
        "speaker_diarize_start",
        json!({"asset_id":"missing"}),
    );
    assert_eq!(model_not_ready["status"], "failed");
    assert_eq!(diagnostic_code(&model_not_ready), "MODEL_NOT_READY");
    unready_host.stop();

    seed_installed_models(&app_data);
    let mut host = HostProcess::spawn(&app_data);
    for (command, payload) in [
        ("speaker_list", json!({})),
        ("speaker_name_proposals", json!({"asset_id":"missing"})),
        (
            "speaker_agent_payload_preview",
            json!({"asset_id":"missing","speaker_id":"missing"}),
        ),
        ("speaker_diarization_get", json!({"asset_id":"missing"})),
        ("speaker_diarize_start", json!({"asset_id":"missing"})),
    ] {
        let closed = host.invoke(&format!("closed-{command}"), command, payload);
        assert_eq!(closed["status"], "failed", "{closed:#}");
        assert_eq!(diagnostic_code(&closed), "PROJECT_NOT_OPEN");
    }
    let closed_confirmation = host.invoke(
        "closed-confirm",
        "speaker_name_confirm",
        json!({"speaker_id":"missing","display_name":"姓名","confirmed":false}),
    );
    assert_eq!(
        diagnostic_code(&closed_confirmation),
        "SPEAKER_CONFIRM_REQUIRED"
    );

    let created = host.invoke("create", "project_create", json!({"path":project}));
    assert_success(&created);
    let project_root = created["data"]["root"]
        .as_str()
        .expect("project root")
        .to_string();
    let database = PathBuf::from(
        created["data"]["database"]
            .as_str()
            .expect("project database"),
    );

    let first_import = host.invoke("import-first", "import_media", json!({"path":first_media}));
    let second_import = host.invoke(
        "import-second",
        "import_media",
        json!({"path":second_media}),
    );
    assert_success(&first_import);
    assert_success(&second_import);
    let first_asset = first_import["data"]["id"]
        .as_str()
        .expect("first asset id")
        .to_string();
    let second_asset = second_import["data"]["id"]
        .as_str()
        .expect("second asset id")
        .to_string();

    for (index, asset_id) in [&first_asset, &second_asset].into_iter().enumerate() {
        start_and_wait(
            &mut host,
            &format!("transcribe-{index}"),
            "transcribe_start",
            json!({"asset_id":asset_id,"model":"qwen3-asr-0.6b","language":"auto"}),
        );
    }

    let first_source = ProjectStore::open(&database)
        .expect("open project store")
        .media_asset(&first_asset)
        .expect("read first media")
        .expect("first media row")
        .original_path;
    let other_word_id =
        replace_first_transcript(&database, &first_asset, &project_root, &first_source);

    start_and_wait(
        &mut host,
        "diarize-first",
        "speaker_diarize_start",
        json!({"asset_id":first_asset}),
    );
    let first_diarization = host.invoke(
        "diarization-first",
        "speaker_diarization_get",
        json!({"asset_id":first_asset}),
    );
    assert_success(&first_diarization);
    assert_eq!(first_diarization["data"]["segment_count"], 1);
    assert_eq!(
        first_diarization["data"]["speakers"]
            .as_array()
            .expect("first speakers")
            .len(),
        1
    );
    let first_speaker = first_diarization["data"]["speakers"][0]["id"]
        .as_str()
        .expect("first speaker id")
        .to_string();

    start_and_wait(
        &mut host,
        "diarize-second",
        "speaker_diarize_start",
        json!({"asset_id":second_asset}),
    );
    let second_diarization = host.invoke(
        "diarization-second",
        "speaker_diarization_get",
        json!({"asset_id":second_asset}),
    );
    assert_success(&second_diarization);
    assert_eq!(second_diarization["data"]["segment_count"], 1);
    let second_speaker = second_diarization["data"]["speakers"][0]["id"]
        .as_str()
        .expect("second speaker id")
        .to_string();
    assert_ne!(first_speaker, second_speaker);
    assert!(
        second_diarization["data"]["merge_proposals"]
            .as_array()
            .expect("merge proposals")
            .iter()
            .any(|proposal| {
                let left = proposal["left_speaker_id"].as_str();
                let right = proposal["right_speaker_id"].as_str();
                (left == Some(first_speaker.as_str()) && right == Some(second_speaker.as_str()))
                    || (left == Some(second_speaker.as_str())
                        && right == Some(first_speaker.as_str()))
            })
    );

    let store = ProjectStore::open(&database).expect("open project store");
    assert_eq!(
        store
            .speaker_embeddings()
            .expect("project-local embeddings")
            .len(),
        2
    );
    store
        .set_word_speaker_assignments(
            &first_asset,
            &[(
                other_word_id.clone(),
                vec![SpeakerAssignment {
                    speaker_id: second_speaker.clone(),
                    confidence: Some(1.0),
                    evidence: "slice6_other_speaker".to_string(),
                }],
            )],
        )
        .expect("assign other-speaker control text");
    drop(store);

    let proposals = host.invoke(
        "name-proposals",
        "speaker_name_proposals",
        json!({"asset_id":first_asset}),
    );
    assert_success(&proposals);
    assert!(
        proposals["data"]
            .as_array()
            .expect("name proposals")
            .iter()
            .any(|proposal| {
                proposal["speaker_id"] == first_speaker && proposal["candidate_name"] == "李明"
            })
    );

    let agent = host.invoke(
        "agent-preview",
        "speaker_agent_payload_preview",
        json!({"asset_id":first_asset,"speaker_id":first_speaker}),
    );
    assert_success(&agent);
    assert_eq!(agent["data"]["speaker_id"], first_speaker);
    let agent_text = serde_json::to_string(&agent["data"]).expect("agent payload JSON");
    assert!(agent_text.contains("我是李明"), "{agent_text}");
    assert!(agent_text.contains("<PROJECT>"), "{agent_text}");
    assert!(agent_text.contains("<MEDIA>"), "{agent_text}");
    assert!(!agent_text.contains(&project_root), "{agent_text}");
    assert!(!agent_text.contains(&first_source), "{agent_text}");
    assert!(!agent_text.contains("乙方秘密文本"), "{agent_text}");
    assert!(!agent_text.contains("embedding"), "{agent_text}");
    assert!(!contains_embedding_shaped_array(&agent["data"]));

    let revision_before_confirm =
        host.invoke("revision-before", "project_revision", json!({}))["data"]
            .as_u64()
            .expect("revision before confirmation");
    let rejected_name = host.invoke(
        "reject-name",
        "speaker_name_confirm",
        json!({"speaker_id":first_speaker,"display_name":"李明","confirmed":false}),
    );
    assert_eq!(diagnostic_code(&rejected_name), "SPEAKER_CONFIRM_REQUIRED");
    assert_eq!(
        host.invoke("revision-after-reject", "project_revision", json!({}))["data"],
        revision_before_confirm
    );
    let confirmed_name = host.invoke(
        "confirm-name",
        "speaker_name_confirm",
        json!({"speaker_id":first_speaker,"display_name":"李明","confirmed":true}),
    );
    assert_success(&confirmed_name);
    assert_eq!(confirmed_name["data"]["display_name"], "李明");
    assert_eq!(confirmed_name["data"]["confirmed"], true);
    let name_revision = confirmed_name["revision"]
        .as_u64()
        .expect("name confirmation revision");
    assert!(name_revision > revision_before_confirm);

    let rejected_merge = host.invoke(
        "reject-merge",
        "speaker_merge_confirm",
        json!({
            "keep_speaker_id":first_speaker,
            "merge_speaker_id":second_speaker,
            "confirmed":false
        }),
    );
    assert_eq!(diagnostic_code(&rejected_merge), "SPEAKER_CONFIRM_REQUIRED");
    let merged = host.invoke(
        "confirm-merge",
        "speaker_merge_confirm",
        json!({
            "keep_speaker_id":first_speaker,
            "merge_speaker_id":second_speaker,
            "confirmed":true
        }),
    );
    assert_success(&merged);
    assert_eq!(merged["data"]["id"], first_speaker);
    assert!(merged["revision"].as_u64().expect("merge revision") > name_revision);

    let speakers = host.invoke("speaker-list", "speaker_list", json!({}));
    assert_success(&speakers);
    assert_eq!(
        speakers["data"].as_array().expect("visible speakers"),
        &[merged["data"].clone()]
    );
    let transcript = host.invoke(
        "transcript-after-merge",
        "transcript_get",
        json!({"asset_id":first_asset}),
    );
    assert_success(&transcript);
    assert!(
        transcript["data"]["words"]
            .as_array()
            .expect("transcript words")
            .iter()
            .filter_map(|word| word["speaker_assignments"].as_array())
            .flatten()
            .all(|assignment| assignment["speaker_id"] == first_speaker)
    );
    assert_eq!(
        ProjectStore::open(&database)
            .expect("open merged store")
            .speaker_embeddings()
            .expect("remaining embeddings")
            .len(),
        1
    );

    let database_text = database.to_string_lossy().into_owned();
    let archived = Command::new("sqlite3")
        .arg(&database)
        .arg(format!(
            "SELECT merged_into FROM speaker_identity WHERE id = '{}';",
            second_speaker.replace('\'', "''")
        ))
        .output();
    match archived {
        Ok(output) => {
            assert!(output.status.success(), "sqlite archive query failed");
            assert_eq!(
                String::from_utf8(output.stdout)
                    .expect("sqlite output")
                    .trim(),
                first_speaker
            );
        }
        Err(_) => missing_test_tools("sqlite3 unavailable for archived-identity assertion"),
    }

    let event_json = serde_json::to_string(&host.events).expect("events JSON");
    assert!(!event_json.contains(&project_root), "{event_json}");
    assert!(!event_json.contains(&first_source), "{event_json}");
    assert!(!event_json.contains(&database_text), "{event_json}");
    assert!(!host.events.iter().any(contains_embedding_field));
    assert!(!host.events.iter().any(contains_embedding_shaped_array));
    assert!(
        host.events
            .iter()
            .any(|event| event["event"] == "dl://progress")
    );
    assert!(host.events.iter().any(|event| {
        event["event"] == "dl://task-state" && event["payload"]["state"] == "succeeded"
    }));

    assert!(!host.responses.iter().any(contains_embedding_field));
    assert!(!host.responses.iter().any(contains_embedding_shaped_array));
    let logs = project.join(".doublelove/logs");
    if logs.is_dir() {
        for entry in fs::read_dir(&logs).expect("speaker logs") {
            let bytes = fs::read(entry.expect("log entry").path()).expect("read log");
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("[1.0, 0.0]"), "{text}");
            assert!(!text.contains("[1.0,0.0]"), "{text}");
        }
    }

    host.stop();
    fs::remove_dir_all(root).expect("cleanup");
}
