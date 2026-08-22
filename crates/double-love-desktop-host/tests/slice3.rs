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
use double_love_engine::FfmpegTools;
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
            return match response.response {
                HostResponseStatus::Ok { result } => {
                    let value = serde_json::to_value(result).expect("host result JSON");
                    assert_eq!(value["type"], "invoke");
                    value["data"].clone()
                }
                other => panic!("expected outer host success, got {other:?}"),
            };
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

fn temp_directory(label: &str) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "double-love-host-slice3-{label}-{}-{}",
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

fn generate_media(tools: &FfmpegTools, path: &Path, fps: &str) {
    let status = Command::new(&tools.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=black:s=320x180:r={fps}:d=1"),
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
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
        .expect("run ffmpeg fixture generation");
    assert!(status.success(), "ffmpeg fixture generation failed");
}

fn diagnostic_code(result: &Value) -> &str {
    result["diagnostics"][0]["code"]
        .as_str()
        .expect("diagnostic code")
}

#[test]
fn imports_lists_and_resolves_project_media_without_renderer_path_exposure() {
    let tools = match FfmpegTools::discover() {
        Ok(tools) => tools,
        Err(_) => {
            missing_test_tools("ffmpeg/ffprobe unavailable");
            return;
        }
    };
    let root = temp_directory("media");
    let project = root.join("project");
    let app_data = root.join("app-data");
    let supported_media = root.join("supported.mp4");
    let unsupported_media = root.join("unsupported.mp4");
    let probe_failure_media = root.join("synthetic-ffprobe-failure.mp4");
    fs::create_dir_all(&root).expect("temporary root");
    generate_media(&tools, &supported_media, "25");
    generate_media(&tools, &unsupported_media, "27");
    fs::write(&probe_failure_media, b"synthetic invalid media").expect("invalid media fixture");

    let mut host = HostProcess::spawn(&app_data);
    for (id, command, payload) in [
        (
            "closed-import",
            "import_media",
            json!({"path": supported_media}),
        ),
        ("closed-list", "assets_list", json!({})),
        (
            "closed-resolve",
            "resolve_media_asset",
            json!({"asset_id": "missing"}),
        ),
    ] {
        let result = host.invoke(id, command, payload);
        assert_eq!(result["status"], "failed");
        assert_eq!(diagnostic_code(&result), "PROJECT_NOT_OPEN");
    }

    let created = host.invoke("create", "project_create", json!({"path": project}));
    assert_eq!(created["status"], "success");
    let canonical_project = project
        .canonicalize()
        .expect("canonical project")
        .to_string_lossy()
        .into_owned();

    let missing_path = root.join("missing.mp4");
    let missing = host.invoke("missing", "import_media", json!({"path": missing_path}));
    assert_eq!(missing["status"], "failed");
    assert_eq!(diagnostic_code(&missing), "MEDIA_FILE_MISSING");
    let missing_json = serde_json::to_string(&missing).expect("missing response JSON");
    assert!(!missing_json.contains(missing_path.to_string_lossy().as_ref()));
    assert!(!missing_json.contains(project.to_string_lossy().as_ref()));
    assert!(!missing_json.contains(&canonical_project));
    assert!(missing_json.contains("<SELECTED_MEDIA>"));

    let canonical_probe_failure = probe_failure_media
        .canonicalize()
        .expect("canonical probe failure media")
        .to_string_lossy()
        .into_owned();
    let probe_failure = host.invoke(
        "probe-failure",
        "import_media",
        json!({"path": probe_failure_media}),
    );
    assert_eq!(probe_failure["status"], "failed");
    assert_eq!(diagnostic_code(&probe_failure), "MEDIA_PROBE_FAILED");
    let probe_failure_json =
        serde_json::to_string(&probe_failure).expect("probe failure response JSON");
    assert!(!probe_failure_json.contains(probe_failure_media.to_string_lossy().as_ref()));
    assert!(!probe_failure_json.contains(&canonical_probe_failure));
    assert!(!probe_failure_json.contains(project.to_string_lossy().as_ref()));
    assert!(!probe_failure_json.contains(&canonical_project));
    assert!(probe_failure_json.contains("<SELECTED_MEDIA>"));

    let unsupported = host.invoke(
        "unsupported",
        "import_media",
        json!({"path": unsupported_media}),
    );
    assert_eq!(unsupported["status"], "failed");
    assert_eq!(diagnostic_code(&unsupported), "MEDIA_FPS_UNSUPPORTED");

    let imported = host.invoke("import", "import_media", json!({"path": supported_media}));
    assert_eq!(imported["status"], "success");
    assert_eq!(imported["data"]["status"], "prepared");
    let asset_id = imported["data"]["id"]
        .as_str()
        .expect("imported asset id")
        .to_string();
    let canonical_media = supported_media
        .canonicalize()
        .expect("canonical fixture media")
        .to_string_lossy()
        .into_owned();
    assert!(
        !serde_json::to_string(&imported)
            .expect("import JSON")
            .contains(&canonical_media),
        "renderer import response must not expose the source path"
    );

    let duplicate = host.invoke(
        "duplicate",
        "import_media",
        json!({"path": supported_media}),
    );
    assert_eq!(duplicate["status"], "success");
    assert_eq!(duplicate["data"]["id"], asset_id);
    assert_eq!(diagnostic_code(&duplicate), "MEDIA_ALREADY_IMPORTED");

    let listed = host.invoke("list", "assets_list", json!({}));
    assert_eq!(listed["status"], "success");
    assert_eq!(listed["data"].as_array().expect("asset list").len(), 1);
    assert_eq!(listed["data"][0]["id"], asset_id);
    assert_eq!(listed["data"][0]["status"], "prepared");
    let mut asset_fields = listed["data"][0]
        .as_object()
        .expect("asset summary object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    asset_fields.sort_unstable();
    assert_eq!(
        asset_fields,
        [
            "audio_channels",
            "audio_sample_rate",
            "display_name",
            "duration_samples",
            "height",
            "id",
            "rate",
            "status",
            "width",
        ]
    );
    assert!(
        !serde_json::to_string(&listed)
            .expect("list JSON")
            .contains(&canonical_media),
        "renderer asset list must not expose the source path"
    );
    assert!(
        !serde_json::to_string(&listed)
            .expect("list JSON")
            .contains(&canonical_project),
        "renderer asset list must not expose the project path"
    );

    let resolved = host.invoke(
        "resolve",
        "resolve_media_asset",
        json!({"asset_id": asset_id}),
    );
    assert_eq!(resolved["status"], "success");
    assert_eq!(resolved["data"], json!({"path": canonical_media}));

    let unknown = host.invoke(
        "resolve-unknown",
        "resolve_media_asset",
        json!({"asset_id": "unknown"}),
    );
    assert_eq!(unknown["status"], "failed");
    assert_eq!(diagnostic_code(&unknown), "MEDIA_ASSET_NOT_FOUND");

    fs::remove_file(&supported_media).expect("remove imported source");
    let removed = host.invoke(
        "resolve-removed",
        "resolve_media_asset",
        json!({"asset_id": duplicate["data"]["id"]}),
    );
    assert_eq!(removed["status"], "failed");
    assert_eq!(diagnostic_code(&removed), "MEDIA_ASSET_NOT_FOUND");

    let main_source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../studio/src/main/index.ts"),
    )
    .expect("read Electron main source");
    let renderer_allowlist = main_source
        .split("const RENDERER_COMMANDS")
        .nth(1)
        .and_then(|tail| tail.split("])").next())
        .expect("renderer command allowlist");
    assert!(!renderer_allowlist.contains("resolve_media_asset"));

    assert_eq!(
        host.invoke("usable", "assets_list", json!({}))["status"],
        "success"
    );
    host.stop();
    fs::remove_dir_all(root).expect("remove temporary root");
}
