//! CLI E2E：合成 mp4 → project-create → import-media → 断言结构化结果。
//! 无 ffmpeg 的环境直接跳过（本机开发环境有 ffmpeg，会真实执行）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn ffmpeg_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("DOUBLELOVE_FFMPEG") {
        return Some(PathBuf::from(explicit));
    }
    for prefix in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let candidate = Path::new(prefix).join("ffmpeg");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // PATH 查找
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("ffmpeg"))
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

fn synthesize_mp4(ffmpeg: &Path, output: &Path, rate: u32) {
    let status = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "lavfi", "-i"])
        .arg(format!("testsrc=size=320x240:rate={rate}"))
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args([
            "-t", "2", "-pix_fmt", "yuv420p", "-c:v", "mpeg4", "-c:a", "aac",
        ])
        .arg(output)
        .status()
        .expect("ffmpeg runs");
    assert!(status.success(), "synthetic mp4 encodes");
}

fn run_cli(args: &[&str]) -> (bool, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_double-love"))
        .args(args)
        .output()
        .expect("cli runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let json = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("stdout is json: {error}\nstdout: {stdout}"));
    (output.status.success(), json)
}

#[test]
fn import_media_end_to_end_with_synthetic_mp4() {
    let Some(ffmpeg) = ffmpeg_path() else {
        eprintln!("skip: ffmpeg not found on this machine");
        return;
    };
    let dir = temp_dir("e2e");
    let project = dir.join("project");
    let media = dir.join("synthetic.mp4");
    synthesize_mp4(&ffmpeg, &media, 25);

    let project_arg = project.to_string_lossy().into_owned();
    let (ok, _) = run_cli(&["--json", "--project", &project_arg, "project-create"]);
    assert!(ok, "project-create succeeds");

    let media_arg = media.to_string_lossy().into_owned();
    let (ok, result) = run_cli(&[
        "--json",
        "--project",
        &project_arg,
        "import-media",
        "--file",
        &media_arg,
    ]);
    assert!(ok, "import-media succeeds: {result}");
    assert_eq!(result["status"], "success");
    assert_eq!(result["data"]["rate"], "fps_25");
    assert_eq!(result["data"]["audio_sample_rate"], 48_000);
    // 2 秒 @ 48kHz：时长必须精确落在采样整数上
    assert_eq!(result["data"]["duration_samples"], 96_000);
    assert_eq!(result["data"]["status"], "prepared");

    // 准备音频已落盘且唯一
    let prepared: Vec<_> = std::fs::read_dir(project.join(".doublelove/prepared"))
        .expect("prepared dir exists")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wav"))
        .collect();
    assert_eq!(prepared.len(), 1, "exactly one prepared wav");

    // 重复导入：复用资产 + info 诊断，不报错
    let (ok, again) = run_cli(&[
        "--json",
        "--project",
        &project_arg,
        "import-media",
        "--file",
        &media_arg,
    ]);
    assert!(ok, "re-import still succeeds");
    assert_eq!(again["status"], "success");
    assert_eq!(again["data"]["id"], result["data"]["id"]);
    let codes: Vec<&str> = again["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert!(codes.contains(&"MEDIA_ALREADY_IMPORTED"));

    std::fs::remove_dir_all(&dir).expect("temp dir removed");
}

#[test]
fn import_media_rejects_unsupported_fps() {
    let Some(ffmpeg) = ffmpeg_path() else {
        eprintln!("skip: ffmpeg not found on this machine");
        return;
    };
    let dir = temp_dir("fps");
    let project = dir.join("project");
    let media = dir.join("15fps.mp4");
    synthesize_mp4(&ffmpeg, &media, 15);

    let project_arg = project.to_string_lossy().into_owned();
    let (ok, _) = run_cli(&["--json", "--project", &project_arg, "project-create"]);
    assert!(ok);

    let media_arg = media.to_string_lossy().into_owned();
    let (ok, result) = run_cli(&[
        "--json",
        "--project",
        &project_arg,
        "import-media",
        "--file",
        &media_arg,
    ]);
    assert!(!ok, "15fps must fail");
    assert_eq!(result["status"], "failed");
    assert_eq!(result["diagnostics"][0]["code"], "MEDIA_FPS_UNSUPPORTED");

    std::fs::remove_dir_all(&dir).expect("temp dir removed");
}

#[test]
fn import_media_reports_missing_file() {
    let dir = temp_dir("missing");
    let project = dir.join("project");
    let project_arg = project.to_string_lossy().into_owned();
    let (ok, _) = run_cli(&["--json", "--project", &project_arg, "project-create"]);
    assert!(ok);

    let missing = dir.join("nope.mp4").to_string_lossy().into_owned();
    let (ok, result) = run_cli(&[
        "--json",
        "--project",
        &project_arg,
        "import-media",
        "--file",
        &missing,
    ]);
    assert!(!ok, "missing file must fail");
    assert_eq!(result["diagnostics"][0]["code"], "MEDIA_FILE_MISSING");

    std::fs::remove_dir_all(&dir).expect("temp dir removed");
}
