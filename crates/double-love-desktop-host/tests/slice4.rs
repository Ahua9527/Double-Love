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
        "double-love-host-slice4-{label}-{}-{}",
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

fn generate_media(tools: &FfmpegTools, path: &Path, fps: &str, color: &str) {
    let status = Command::new(&tools.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c={color}:s=320x180:r={fps}:d=1"),
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

fn assert_success(result: &Value) {
    assert_eq!(result["status"], "success", "{result:#}");
}

#[test]
fn main_track_timeline_and_project_visual_settings_match_tauri_semantics() {
    let tools = match FfmpegTools::discover() {
        Ok(tools) => tools,
        Err(_) => {
            missing_test_tools("ffmpeg/ffprobe unavailable");
            return;
        }
    };
    let root = temp_directory("commands");
    let project = root.join("project");
    let app_data = root.join("app-data");
    let media_a = root.join("a-25.mp4");
    let media_b = root.join("b-30.mp4");
    fs::create_dir_all(&root).expect("temporary root");
    generate_media(&tools, &media_a, "25", "black");
    generate_media(&tools, &media_b, "30", "blue");

    let mut host = HostProcess::spawn(&app_data);
    for (id, command, payload) in [
        ("closed-timeline", "timeline_get", json!({})),
        ("closed-list", "main_track_list", json!({})),
        (
            "closed-append",
            "main_track_append_full",
            json!({"asset_id": "missing"}),
        ),
        ("closed-canvas", "canvas_get", json!({})),
        ("closed-rate", "output_rate_get", json!({})),
        ("closed-style", "subtitle_style_get", json!({})),
        (
            "closed-apply-style",
            "apply_default_subtitle_style",
            json!({}),
        ),
    ] {
        let result = host.invoke(id, command, payload);
        assert_eq!(result["status"], "failed");
        assert_eq!(diagnostic_code(&result), "PROJECT_NOT_OPEN");
    }

    assert_success(&host.invoke("create", "project_create", json!({"path": project})));
    let canonical_project = project
        .canonicalize()
        .expect("canonical project")
        .to_string_lossy()
        .into_owned();
    let imported_a = host.invoke("import-a", "import_media", json!({"path": media_a}));
    let imported_b = host.invoke("import-b", "import_media", json!({"path": media_b}));
    assert_success(&imported_a);
    assert_success(&imported_b);
    let asset_a = imported_a["data"]["id"].as_str().expect("asset a");
    let asset_b = imported_b["data"]["id"].as_str().expect("asset b");

    let unknown_asset = host.invoke(
        "unknown-asset",
        "main_track_append_full",
        json!({"asset_id": canonical_project}),
    );
    assert_eq!(unknown_asset["status"], "failed");
    assert_eq!(diagnostic_code(&unknown_asset), "MEDIA_ASSET_MISSING");
    let unknown_asset_json = serde_json::to_string(&unknown_asset).expect("unknown asset JSON");
    assert!(!unknown_asset_json.contains(&canonical_project));
    assert!(unknown_asset_json.contains("<PROJECT>"));

    let invalid_range = host.invoke(
        "invalid-range",
        "main_track_append",
        json!({"asset_id": asset_a, "source_in_frame": 0, "source_out_frame": 999}),
    );
    assert_eq!(invalid_range["status"], "failed");
    assert_eq!(diagnostic_code(&invalid_range), "MAIN_TRACK_RANGE_INVALID");

    let appended_a = host.invoke(
        "append-a",
        "main_track_append",
        json!({"assetId": asset_a, "sourceInFrame": 2, "sourceOutFrame": 20}),
    );
    let appended_b = host.invoke(
        "append-b",
        "main_track_append_full",
        json!({"assetId": asset_b}),
    );
    assert_success(&appended_a);
    assert_success(&appended_b);
    assert!(appended_a["revision"].is_number());
    assert!(appended_b["revision"].is_number());
    let clip_a = appended_a["data"]["id"].as_str().expect("clip a");
    let clip_b = appended_b["data"]["id"].as_str().expect("clip b");

    let listed = host.invoke("list-initial", "main_track_list", json!({}));
    assert_success(&listed);
    assert_eq!(listed["data"].as_array().expect("clip list").len(), 2);
    assert_eq!(listed["data"][0]["id"], clip_a);
    assert_eq!(listed["data"][1]["id"], clip_b);

    let moved = host.invoke(
        "move",
        "main_track_move",
        json!({"clipId": clip_a, "beforeClipId": null}),
    );
    assert_success(&moved);
    assert!(moved["revision"].is_number());
    let trimmed = host.invoke(
        "trim",
        "main_track_trim",
        json!({"clipId": clip_b, "sourceInFrame": 2, "sourceOutFrame": 28}),
    );
    assert_success(&trimmed);
    assert_eq!(trimmed["data"]["source_in_frame"], 2);
    assert_eq!(trimmed["data"]["source_out_frame"], 28);
    let split = host.invoke(
        "split",
        "main_track_split",
        json!({"clipId": clip_b, "sourceAtFrame": 15}),
    );
    assert_success(&split);
    assert_eq!(split["data"].as_array().expect("split clips").len(), 2);
    let right_clip = split["data"][1]["id"].as_str().expect("right clip");
    let removed = host.invoke("remove", "main_track_remove", json!({"clipId": right_clip}));
    assert_success(&removed);

    let unknown_clip = host.invoke(
        "unknown-clip",
        "main_track_trim",
        json!({
            "clip_id": canonical_project,
            "source_in_frame": 0,
            "source_out_frame": 1
        }),
    );
    assert_eq!(unknown_clip["status"], "failed");
    assert_eq!(diagnostic_code(&unknown_clip), "MAIN_TRACK_CLIP_MISSING");
    let unknown_clip_json = serde_json::to_string(&unknown_clip).expect("unknown clip JSON");
    assert!(!unknown_clip_json.contains(&canonical_project));
    assert!(unknown_clip_json.contains("<PROJECT>"));

    let final_list = host.invoke("list-final", "main_track_list", json!({}));
    assert_success(&final_list);
    assert_eq!(final_list["data"].as_array().expect("final clips").len(), 2);
    assert_eq!(final_list["data"][0]["source_asset_id"], asset_b);
    assert_eq!(final_list["data"][0]["source_in_frame"], 2);
    assert_eq!(final_list["data"][0]["source_out_frame"], 15);
    assert_eq!(final_list["data"][1]["source_asset_id"], asset_a);

    let timeline = host.invoke("timeline", "timeline_get", json!({}));
    assert_success(&timeline);
    assert_eq!(timeline["data"]["schema_version"], 2);
    assert_eq!(timeline["data"]["name"], "project Rough Cut");
    assert_eq!(timeline["data"]["rate"], "fps_30");
    assert_eq!(
        timeline["data"]["sources"]
            .as_array()
            .expect("sources")
            .len(),
        2
    );
    assert_eq!(
        timeline["data"]["clips"].as_array().expect("clips").len(),
        2
    );
    assert_eq!(timeline["data"]["clips"][0]["source_asset_id"], asset_b);
    assert_eq!(timeline["data"]["clips"][1]["source_asset_id"], asset_a);

    let canvas = json!({
        "width": 1280,
        "height": 720,
        "background": "#112233",
        "fit": "cover",
        "position_x": 5.0,
        "position_y": -2.0,
        "scale": 1.2,
        "rotation_degrees": 3.0,
        "opacity": 0.9
    });
    let canvas_set = host.invoke("canvas-set", "canvas_set", json!({"canvas": canvas}));
    assert_success(&canvas_set);
    assert!(canvas_set["revision"].is_number());
    assert_eq!(
        host.invoke("canvas-get", "canvas_get", json!({}))["data"],
        canvas
    );

    assert_eq!(
        host.invoke("rate-get", "output_rate_get", json!({}))["data"],
        Value::Null
    );
    let rate_set = host.invoke("rate-set", "output_rate_set", json!({"rate": "fps_24"}));
    assert_success(&rate_set);
    assert!(rate_set["revision"].is_number());
    assert_eq!(rate_set["data"], "fps_24");
    assert_eq!(
        host.invoke("rate-get-set", "output_rate_get", json!({}))["data"],
        "fps_24"
    );
    assert_eq!(
        host.invoke("timeline-explicit", "timeline_get", json!({}))["data"]["rate"],
        "fps_24"
    );
    let rate_clear = host.invoke("rate-clear", "output_rate_set", json!({"rate": null}));
    assert_success(&rate_clear);
    assert!(rate_clear["revision"].is_number());
    assert_eq!(rate_clear["data"], Value::Null);
    assert_eq!(
        host.invoke("rate-get-clear", "output_rate_get", json!({}))["data"],
        Value::Null
    );
    assert_eq!(
        host.invoke("timeline-follow", "timeline_get", json!({}))["data"]["rate"],
        "fps_30"
    );

    let initial_style = host.invoke("style-get", "subtitle_style_get", json!({}))["data"].clone();
    let mut project_style = initial_style.clone();
    project_style["font_size"] = json!(61.0);
    let style_set = host.invoke(
        "style-set",
        "subtitle_style_set",
        json!({"style": project_style}),
    );
    assert_success(&style_set);
    assert!(style_set["revision"].is_number());
    assert_eq!(
        host.invoke("style-get-set", "subtitle_style_get", json!({}))["data"]["font_size"],
        61.0
    );

    let mut preference_style = initial_style;
    preference_style["font_family"] = json!("Helvetica");
    preference_style["font_size"] = json!(44.0);
    let preferences = host.invoke(
        "preferences-style",
        "preferences_update",
        json!({"patch": {"default_subtitle_style": preference_style}}),
    );
    assert_success(&preferences);
    let applied = host.invoke(
        "apply-default-style",
        "apply_default_subtitle_style",
        json!({}),
    );
    assert_success(&applied);
    assert!(applied["revision"].is_number());
    assert_eq!(applied["data"]["font_family"], "Helvetica");
    assert_eq!(applied["data"]["font_size"], 44.0);
    assert_eq!(
        host.invoke("style-get-applied", "subtitle_style_get", json!({}))["data"],
        applied["data"]
    );

    host.stop();
    fs::remove_dir_all(root).expect("remove temporary root");
}
