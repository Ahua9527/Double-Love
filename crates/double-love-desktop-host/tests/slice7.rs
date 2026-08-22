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
    FfmpegTools, FrameRate, NewMediaAsset, ProjectStore, create_project, ffmpeg_supports_ass_filter,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct HostProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    events: Vec<Value>,
}

impl HostProcess {
    fn spawn(app_data_dir: &Path) -> Self {
        Self::spawn_with_tools(app_data_dir, None)
    }

    fn spawn_with_tools(app_data_dir: &Path, tools: Option<(&Path, &Path)>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_double-love-desktop-host"));
        command
            .args(["--app-data-dir"])
            .arg(app_data_dir)
            .args(["--resource-dir"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .arg("--test-transcribe-mock")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some((ffmpeg, ffprobe)) = tools {
            command
                .env("DOUBLELOVE_FFMPEG", ffmpeg)
                .env("DOUBLELOVE_FFPROBE", ffprobe);
        }
        let mut child = command.spawn().expect("spawn desktop host");
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
        "double-love-host-slice7-{label}-{}-{}",
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

fn generate_media(tools: &FfmpegTools, path: &Path, fps: &str, color: &str, frequency: u32) {
    let output = Command::new(&tools.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c={color}:s=320x180:r={fps}:d=2"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={frequency}:sample_rate=48000:duration=2"),
            "-c:v",
            "mpeg4",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(path)
        .output()
        .expect("generate media");
    assert!(
        output.status.success(),
        "ffmpeg fixture generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn seed_installed_transcription_models(app_data: &Path) {
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

fn start_and_wait_transcription(host: &mut HostProcess, id: &str, asset_id: &str) {
    let started = host.invoke(
        id,
        "transcribe_start",
        json!({
            "asset_id": asset_id,
            "model": "qwen3-asr-0.6b",
            "language": "auto"
        }),
    );
    assert_success(&started);
    let task_id = started["data"]["task_id"]
        .as_str()
        .expect("task id")
        .to_string();
    host.wait_task_state(&task_id, "succeeded");
}

fn file_sha256(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(fs::read(path).expect("artifact bytes"))
    )
}

fn assert_output(result: &Value, kind: &str, path: &Path) {
    assert_success(result);
    assert_eq!(result["outputs"].as_array().expect("outputs").len(), 1);
    assert_eq!(result["outputs"][0]["kind"], kind);
    assert_eq!(
        result["outputs"][0]["path"],
        path.to_string_lossy().as_ref()
    );
    let sha256 = result["outputs"][0]["sha256"]
        .as_str()
        .expect("output sha256");
    assert_eq!(sha256.len(), 64);
    assert_eq!(sha256, file_sha256(path));
}

fn seed_corrupt_unknown_asset_project(root: &Path, media_path: &Path) -> PathBuf {
    let summary = create_project(root).expect("create corrupt fixture project");
    let database = PathBuf::from(&summary.database);
    let store = ProjectStore::open(&database).expect("open corrupt fixture store");
    store
        .insert_media_asset(&NewMediaAsset {
            id: "known-asset".to_string(),
            kind: "video".to_string(),
            original_path: media_path.to_string_lossy().into_owned(),
            display_name: "known.mp4".to_string(),
            duration_samples: 96_000,
            audio_sample_rate: 48_000,
            fps_num: 25,
            fps_den: 1,
            video_timebase: 25,
            is_ntsc: false,
            width: Some(320),
            height: Some(180),
            audio_channels: Some(1),
            source_tc_start_frame: Some(0),
            source_tc_is_drop_frame: false,
            ffprobe_json: "{}".to_string(),
        })
        .expect("insert corrupt fixture asset");
    store
        .append_main_track_clip("corrupt-clip", "known-asset", 0, 25)
        .expect("append corrupt fixture clip");
    store
        .set_output_rate(FrameRate::Fps25)
        .expect("set explicit output rate");
    drop(store);

    let connection = Connection::open(&database).expect("open raw fixture connection");
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .expect("disable fixture foreign keys");
    let unknown = format!(
        "{}::{}",
        media_path.display(),
        root.canonicalize()
            .expect("canonical corrupt fixture")
            .display()
    );
    connection
        .execute(
            "UPDATE main_track_clip SET asset_id = ?1 WHERE id = 'corrupt-clip'",
            [&unknown],
        )
        .expect("corrupt fixture asset reference");
    database
}

#[test]
fn project_exports_match_tauri_semantics_and_sanitize_failure_diagnostics() {
    let tools = match FfmpegTools::discover() {
        Ok(tools) => tools,
        Err(_) => {
            missing_test_tools("ffmpeg/ffprobe unavailable");
            return;
        }
    };
    if !ffmpeg_supports_ass_filter(&tools) {
        missing_test_tools("ffmpeg lacks ass/libass filter");
        return;
    }
    if Command::new("python3").arg("--version").output().is_err() {
        missing_test_tools("python3 unavailable");
        return;
    }

    let root = temp_directory("commands");
    let app_data = root.join("app-data");
    let project = root.join("project");
    let first_media = root.join("first-25.mp4");
    let second_media = root.join("second-30000-1001.mp4");
    let xmeml_target = root.join("rough-cut.xml");
    let ass_target = root.join("rough-cut.ass");
    let mp4_target = root.join("rough-cut.mp4");
    fs::create_dir_all(&root).expect("temporary root");
    generate_media(&tools, &first_media, "25", "steelblue", 440);
    generate_media(&tools, &second_media, "30000/1001", "seagreen", 660);
    let canonical_first_media = first_media
        .canonicalize()
        .expect("canonical first media")
        .to_string_lossy()
        .into_owned();
    let canonical_second_media = second_media
        .canonicalize()
        .expect("canonical second media")
        .to_string_lossy()
        .into_owned();
    seed_installed_transcription_models(&app_data);

    let mut host = HostProcess::spawn(&app_data);
    for (id, command, payload) in [
        ("closed-preview", "project_export_preview", json!({})),
        (
            "closed-xmeml",
            "project_export_xmeml_apply",
            json!({"target_path": xmeml_target}),
        ),
        (
            "closed-ass",
            "project_export_ass_apply",
            json!({"target_path": ass_target}),
        ),
        (
            "closed-mp4",
            "project_render_mp4_apply",
            json!({"target_path": mp4_target}),
        ),
    ] {
        let result = host.invoke(id, command, payload);
        assert_eq!(result["status"], "failed");
        assert_eq!(diagnostic_code(&result), "PROJECT_NOT_OPEN");
    }

    let created = host.invoke("create", "project_create", json!({"path": project}));
    assert_success(&created);
    let database = PathBuf::from(
        created["data"]["database"]
            .as_str()
            .expect("project database"),
    );
    let canonical_project = project
        .canonicalize()
        .expect("canonical project")
        .to_string_lossy()
        .into_owned();

    let empty_preview = host.invoke("empty-preview", "project_export_preview", json!({}));
    assert_eq!(empty_preview["status"], "failed");
    assert_eq!(diagnostic_code(&empty_preview), "TIMELINE_EMPTY");
    assert!(
        empty_preview["diagnostics"][0]["blocks_export"]
            .as_bool()
            .expect("blocking diagnostic")
    );
    let empty_target = root.join("empty.xml");
    let empty_apply = host.invoke(
        "empty-apply",
        "project_export_xmeml_apply",
        json!({"target_path": empty_target}),
    );
    assert_eq!(empty_apply["status"], "failed");
    assert_eq!(diagnostic_code(&empty_apply), "TIMELINE_EMPTY");
    assert!(!root.join("empty.xml").exists());

    let imported_first = host.invoke("import-first", "import_media", json!({"path": first_media}));
    let imported_second = host.invoke(
        "import-second",
        "import_media",
        json!({"path": second_media}),
    );
    assert_success(&imported_first);
    assert_success(&imported_second);
    let first_asset = imported_first["data"]["id"]
        .as_str()
        .expect("first asset")
        .to_string();
    let second_asset = imported_second["data"]["id"]
        .as_str()
        .expect("second asset")
        .to_string();

    let unknown_asset = host.invoke(
        "unknown-asset",
        "main_track_append_full",
        json!({"asset_id": "unknown-asset"}),
    );
    assert_eq!(unknown_asset["status"], "failed");
    assert_eq!(diagnostic_code(&unknown_asset), "MEDIA_ASSET_MISSING");

    assert_success(&host.invoke(
        "append-first",
        "main_track_append_full",
        json!({"asset_id": first_asset}),
    ));
    assert_success(&host.invoke(
        "append-second",
        "main_track_append_full",
        json!({"asset_id": second_asset}),
    ));
    assert_success(&host.invoke(
        "canvas",
        "canvas_set",
        json!({"canvas": {
            "width": 320,
            "height": 180,
            "background": "#000000",
            "fit": "contain",
            "position_x": 0.0,
            "position_y": 0.0,
            "scale": 1.0,
            "rotation_degrees": 0.0,
            "opacity": 1.0
        }}),
    ));
    start_and_wait_transcription(&mut host, "transcribe-first", &first_asset);
    start_and_wait_transcription(&mut host, "transcribe-second", &second_asset);
    let first_transcript = host.invoke(
        "transcript-first",
        "transcript_get",
        json!({"asset_id": first_asset}),
    );
    assert_success(&first_transcript);
    assert!(
        first_transcript["data"]["words"]
            .as_array()
            .expect("transcript words")
            .len()
            >= 3
    );
    assert_success(&host.invoke(
        "omit",
        "edit_omit",
        json!({"asset_id": first_asset, "start_ordinal": 1, "end_ordinal": 1}),
    ));

    let revision_before_preview = host.invoke("revision-before", "project_revision", json!({}));
    let preview = host.invoke("preview", "project_export_preview", json!({}));
    assert_success(&preview);
    assert_eq!(preview["data"]["timeline"]["schema_version"], 2);
    assert_eq!(preview["data"]["timeline"]["name"], "project Rough Cut");
    assert_eq!(preview["data"]["timeline"]["rate"], "fps_25");
    assert_eq!(
        preview["data"]["timeline"]["sources"]
            .as_array()
            .expect("timeline sources")
            .len(),
        2
    );
    assert!(
        preview["data"]["timeline"]["clips"]
            .as_array()
            .expect("timeline clips")
            .iter()
            .any(|clip| clip["source_asset_id"] == first_asset)
    );
    assert!(
        preview["data"]["timeline"]["clips"]
            .as_array()
            .expect("timeline clips")
            .iter()
            .any(|clip| clip["source_asset_id"] == second_asset)
    );
    assert!(
        !preview["data"]["subtitle_cues"]
            .as_array()
            .expect("subtitle cues")
            .is_empty()
    );
    assert_eq!(
        preview["data"]["compatibility"]
            .as_array()
            .expect("compatibility reports")
            .len(),
        2
    );
    assert!(
        preview["outputs"]
            .as_array()
            .expect("preview outputs")
            .is_empty()
    );
    assert_eq!(
        host.invoke("revision-after", "project_revision", json!({}))["data"],
        revision_before_preview["data"]
    );
    assert!(!xmeml_target.exists());
    assert!(!ass_target.exists());
    assert!(!mp4_target.exists());

    let xmeml = host.invoke(
        "xmeml",
        "project_export_xmeml_apply",
        json!({"target_path": xmeml_target}),
    );
    assert_output(&xmeml, "premiere_resolve_xmeml", &xmeml_target);
    let xmeml_text = fs::read_to_string(&xmeml_target).expect("XMEML text");
    assert!(xmeml_text.contains("<!DOCTYPE xmeml>"));
    assert_eq!(xmeml_text.matches("<pathurl>").count(), 2);
    assert!(xmeml_text.contains("first-25.mp4"));
    assert!(xmeml_text.contains("second-30000-1001.mp4"));

    let ass = host.invoke(
        "ass",
        "project_export_ass_apply",
        json!({"target_path": ass_target}),
    );
    assert_output(&ass, "ass", &ass_target);
    let ass_text = fs::read_to_string(&ass_target).expect("ASS text");
    assert!(ass_text.contains("[V4+ Styles]"));
    assert!(ass_text.contains("Style: DoubleLove"));
    assert!(ass_text.contains("开拍"));

    let mp4 = host.invoke(
        "mp4",
        "project_render_mp4_apply",
        json!({"target_path": mp4_target}),
    );
    assert_output(&mp4, "mp4_burned_subtitles", &mp4_target);
    assert!(fs::metadata(&mp4_target).expect("MP4 metadata").len() > 10_000);
    let probe = Command::new(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name:format=duration",
            "-of",
            "json",
        ])
        .arg(&mp4_target)
        .output()
        .expect("ffprobe rendered MP4");
    assert!(probe.status.success(), "ffprobe failed");
    let probe: Value = serde_json::from_slice(&probe.stdout).expect("ffprobe JSON");
    assert_eq!(probe["streams"][0]["codec_name"], "h264");
    let duration = probe["format"]["duration"]
        .as_str()
        .expect("render duration")
        .parse::<f64>()
        .expect("numeric render duration");
    assert!(duration > 2.0, "render duration was {duration}");

    let history = host.invoke("history", "project_history", json!({"limit": 20}));
    assert_success(&history);
    let operations = history["data"]
        .as_array()
        .expect("history")
        .iter()
        .filter_map(|entry| entry["operation"].as_str())
        .collect::<Vec<_>>();
    for operation in ["export_xmeml", "export_ass", "export_mp4"] {
        assert!(
            operations.contains(&operation),
            "missing {operation} ledger entry: {operations:?}"
        );
    }
    assert!(
        ProjectStore::open(&database)
            .expect("open exported store")
            .revision()
            .expect("exported revision")
            >= mp4["revision"].as_u64().expect("MP4 revision")
    );

    let invalid_target = host.invoke(
        "invalid-target",
        "project_export_xmeml_apply",
        json!({"target_path": project}),
    );
    assert_eq!(invalid_target["status"], "failed");
    assert_eq!(diagnostic_code(&invalid_target), "EXPORT_WRITE_FAILED");

    let moved_media = root.join("first-25.missing");
    fs::rename(&first_media, &moved_media).expect("hide source media");
    let render_failure = host.invoke(
        "render-failure",
        "project_render_mp4_apply",
        json!({"target_path": root.join("failed-render.mp4")}),
    );
    assert_eq!(render_failure["status"], "failed");
    assert_eq!(diagnostic_code(&render_failure), "RENDER_FFMPEG_FAILED");
    let render_failure_json = serde_json::to_string(&render_failure).expect("failure JSON");
    for sensitive in [
        canonical_project.as_str(),
        first_media.to_string_lossy().as_ref(),
        second_media.to_string_lossy().as_ref(),
        canonical_first_media.as_str(),
        canonical_second_media.as_str(),
    ] {
        assert!(
            !render_failure_json.contains(sensitive),
            "failure leaked {sensitive}: {render_failure_json}"
        );
    }
    assert!(render_failure_json.contains("<MEDIA>"));

    let corrupt_project = root.join("corrupt-project");
    seed_corrupt_unknown_asset_project(&corrupt_project, &second_media);
    assert_success(&host.invoke(
        "open-corrupt",
        "project_open",
        json!({"path": corrupt_project}),
    ));
    let unknown_export = host.invoke("unknown-export", "project_export_preview", json!({}));
    assert_eq!(unknown_export["status"], "failed");
    assert_eq!(diagnostic_code(&unknown_export), "TIMELINE_SOURCE_MISSING");
    let unknown_json = serde_json::to_string(&unknown_export).expect("unknown export JSON");
    assert!(
        !unknown_json.contains(second_media.to_string_lossy().as_ref()),
        "{unknown_json}"
    );
    assert!(
        !unknown_json.contains(corrupt_project.to_string_lossy().as_ref()),
        "{unknown_json}"
    );
    assert!(
        !unknown_json.contains(
            corrupt_project
                .canonicalize()
                .expect("canonical corrupt project")
                .to_string_lossy()
                .as_ref()
        ),
        "{unknown_json}"
    );
    assert!(unknown_json.contains("<MEDIA>"));
    assert!(unknown_json.contains("<PROJECT>"));

    host.stop();
    fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(unix)]
#[test]
fn project_render_reports_the_tauri_libass_diagnostic() {
    use std::os::unix::fs::PermissionsExt;

    let tools = match FfmpegTools::discover() {
        Ok(tools) => tools,
        Err(_) => {
            missing_test_tools("ffmpeg/ffprobe unavailable");
            return;
        }
    };
    let root = temp_directory("libass-missing");
    let app_data = root.join("app-data");
    let project = root.join("project");
    let fake_ffmpeg = root.join("ffmpeg-without-libass");
    fs::create_dir_all(&root).expect("temporary root");
    fs::write(&fake_ffmpeg, "#!/bin/sh\nexit 0\n").expect("fake ffmpeg");
    let mut permissions = fs::metadata(&fake_ffmpeg)
        .expect("fake ffmpeg metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_ffmpeg, permissions).expect("fake ffmpeg executable");

    let summary = create_project(&project).expect("create libass fixture project");
    let store = ProjectStore::open(Path::new(&summary.database)).expect("open fixture store");
    store
        .insert_media_asset(&NewMediaAsset {
            id: "asset".to_string(),
            kind: "video".to_string(),
            original_path: root.join("unused.mp4").to_string_lossy().into_owned(),
            display_name: "unused.mp4".to_string(),
            duration_samples: 48_000,
            audio_sample_rate: 48_000,
            fps_num: 25,
            fps_den: 1,
            video_timebase: 25,
            is_ntsc: false,
            width: Some(320),
            height: Some(180),
            audio_channels: Some(1),
            source_tc_start_frame: Some(0),
            source_tc_is_drop_frame: false,
            ffprobe_json: "{}".to_string(),
        })
        .expect("insert fixture asset");
    store
        .append_main_track_clip("clip", "asset", 0, 25)
        .expect("append fixture clip");
    drop(store);

    let mut host =
        HostProcess::spawn_with_tools(&app_data, Some((&fake_ffmpeg, tools.ffprobe.as_path())));
    assert_success(&host.invoke("open", "project_open", json!({"path": project})));
    let result = host.invoke(
        "render",
        "project_render_mp4_apply",
        json!({"target_path": root.join("never-written.mp4")}),
    );
    assert_eq!(result["status"], "failed");
    assert_eq!(diagnostic_code(&result), "RENDER_ASS_FILTER_MISSING");
    assert_eq!(
        result["diagnostics"][0]["cause"],
        "当前 ffmpeg 未包含 ASS/libass 字幕滤镜，无法可靠烧录项目级字幕。"
    );
    assert_eq!(
        result["diagnostics"][0]["suggested_action"],
        "使用 App 随附的渲染运行时；开发环境请安装带 libass 的 ffmpeg。"
    );
    assert_eq!(result["diagnostics"][0]["blocks_export"], true);
    assert!(!root.join("never-written.mp4").exists());

    host.stop();
    fs::remove_dir_all(root).expect("cleanup");
}
