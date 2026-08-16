//! CLI E2E：合成 mp4 → import-media → transcribe --mock → 词落库。
//! 无 ffmpeg/python3 的环境跳过。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn tool_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("double-love-cli-{label}-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir created");
    dir
}

fn run_cli(args: &[String]) -> (bool, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_double-love"))
        .args(args)
        .output()
        .expect("cli runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let json = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("stdout is json: {error}\nstdout: {stdout}"));
    (output.status.success(), json)
}

fn sidecar_dir() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sidecars/asr")
        .canonicalize()
        .expect("sidecar dir")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn transcribe_mock_after_import() {
    let (Some(ffmpeg), Some(_python)) = (tool_path("ffmpeg"), tool_path("python3")) else {
        eprintln!("skip: ffmpeg or python3 not found");
        return;
    };
    let dir = temp_dir("transcribe");
    let project = dir.join("project");
    let media = dir.join("synthetic.mp4");

    // 2 秒合成素材（25fps CFR + 48kHz 正弦）
    let status = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "lavfi", "-i", "testsrc=size=320x240:rate=25"])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args([
            "-t", "2", "-pix_fmt", "yuv420p", "-c:v", "mpeg4", "-c:a", "aac",
        ])
        .arg(&media)
        .status()
        .expect("ffmpeg runs");
    assert!(status.success());

    let p = || project.to_string_lossy().into_owned();
    let (ok, _) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "project-create".into(),
    ]);
    assert!(ok);

    let (ok, imported) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "import-media".into(),
        "--file".into(),
        media.to_string_lossy().into_owned(),
    ]);
    assert!(ok, "import succeeds: {imported}");
    let asset_id = imported["data"]["id"]
        .as_str()
        .expect("asset id")
        .to_string();

    let (ok, result) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "transcribe".into(),
        "--asset".into(),
        asset_id,
        "--mock".into(),
        "--sidecar-dir".into(),
        sidecar_dir(),
    ]);
    assert!(ok, "transcribe succeeds: {result}");
    assert_eq!(result["status"], "success");
    assert_eq!(result["data"]["state"], "succeeded");
    let words = result["data"]["words"].as_u64().expect("words is a count");
    assert!(words >= 3, "mock should yield several words, got {words}");
    assert_eq!(result["counts"]["processed"], words);

    // 未知资产：同步前置校验失败
    let (ok, missing) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "transcribe".into(),
        "--asset".into(),
        "no-such-asset".into(),
        "--mock".into(),
        "--sidecar-dir".into(),
        sidecar_dir(),
    ]);
    assert!(!ok);
    assert_eq!(missing["diagnostics"][0]["code"], "TRANSCRIBE_START_FAILED");

    std::fs::remove_dir_all(&dir).expect("temp dir removed");
}
