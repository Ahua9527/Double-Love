//! CLI E2E 无头全链路：合成 mp4 → import → transcribe --mock → edit-omit →
//! export-roughcut（preview 不落盘 / apply 落盘）→ edit-restore → 全删空拒绝导出。
//! 无 ffmpeg/python3 的环境跳过。

mod common;

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
fn rough_cut_headless_chain() {
    let (Some(ffmpeg), Some(_python)) = (tool_path("ffmpeg"), tool_path("python3")) else {
        common::missing_test_tools("ffmpeg or python3 not found");
        return;
    };
    let dir = temp_dir("roughcut");
    let project = dir.join("project");
    let media = dir.join("synthetic.mp4");

    // 2 秒合成素材（25fps CFR + 48kHz 正弦）：mock 每 0.5s 一词 → 4 词
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

    let (ok, transcribed) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "transcribe".into(),
        "--asset".into(),
        asset_id.clone(),
        "--mock".into(),
        "--sidecar-dir".into(),
        sidecar_dir(),
    ]);
    assert!(ok, "transcribe succeeds: {transcribed}");
    let words = transcribed["data"]["words"]
        .as_u64()
        .expect("words is a count");
    assert!(words >= 3, "mock should yield several words, got {words}");

    // 删掉中间词（保留头尾各至少一词）→ preview：两段、不落盘
    let (ok, omitted) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "edit-omit".into(),
        "--asset".into(),
        asset_id.clone(),
        "--start".into(),
        "1".into(),
        "--end".into(),
        (words - 2).to_string(),
    ]);
    assert!(ok, "omit succeeds: {omitted}");
    let omit_id = omitted["data"]["id"].as_str().expect("omit id").to_string();

    let (ok, preview) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "export-roughcut".into(),
        "--asset".into(),
        asset_id.clone(),
    ]);
    assert!(ok, "preview succeeds: {preview}");
    assert_eq!(preview["status"], "success");
    assert_eq!(
        preview["data"]["ir"]["clips"].as_array().map(Vec::len),
        Some(2),
        "删除中间词后应为两段: {preview}"
    );
    assert!(preview["data"]["artifact_path"].is_null(), "preview 不落盘");
    // project-create 会预建 exports 目录；preview 的不变量是「目录里零文件」
    let export_files = std::fs::read_dir(project.join(".doublelove/exports"))
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(export_files, 0, "preview 不得写出任何文件");

    // apply：XMEML 落盘 + sha256 + outputs 记录
    let (ok, applied) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "export-roughcut".into(),
        "--asset".into(),
        asset_id.clone(),
        "--apply".into(),
    ]);
    assert!(ok, "apply succeeds: {applied}");
    let artifact = applied["data"]["artifact_path"]
        .as_str()
        .expect("artifact path")
        .to_string();
    assert!(artifact.ends_with("_ROUGH_CUT.xml"));
    let xml = std::fs::read_to_string(&artifact).expect("artifact readable");
    assert!(xml.contains("<!DOCTYPE xmeml>"));
    assert_eq!(xml.matches("<clipitem id=").count(), 4, "两段 × 音视频各一");
    assert_eq!(
        applied["data"]["sha256"].as_str().map(str::len),
        Some(64),
        "sha256 hex"
    );
    assert_eq!(applied["outputs"][0]["kind"], "premiere_xmeml");

    // restore 整条 omit → preview 回到单段
    let (ok, restored) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "edit-restore".into(),
        "--operation".into(),
        omit_id,
        "--start".into(),
        "1".into(),
        "--end".into(),
        (words - 2).to_string(),
    ]);
    assert!(ok, "restore succeeds: {restored}");
    let (ok, preview) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "export-roughcut".into(),
        "--asset".into(),
        asset_id.clone(),
    ]);
    assert!(ok, "preview after restore: {preview}");
    assert_eq!(
        preview["data"]["ir"]["clips"].as_array().map(Vec::len),
        Some(1),
        "恢复后应为完整单段: {preview}"
    );

    // 全删空：preview 直接 failed（ROUGH_CUT_EMPTY），不产生任何文件
    let (ok, _) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "edit-omit".into(),
        "--asset".into(),
        asset_id.clone(),
        "--start".into(),
        "0".into(),
        "--end".into(),
        (words - 1).to_string(),
    ]);
    assert!(ok);
    let exports_before = std::fs::read_dir(project.join(".doublelove/exports"))
        .map(|entries| entries.count())
        .unwrap_or(0);
    let (ok, empty) = run_cli(&[
        "--json".into(),
        "--project".into(),
        p(),
        "export-roughcut".into(),
        "--asset".into(),
        asset_id.clone(),
        "--apply".into(),
    ]);
    assert!(!ok, "全删空必须失败: {empty}");
    assert_eq!(empty["status"], "failed");
    assert_eq!(empty["diagnostics"][0]["code"], "ROUGH_CUT_EMPTY");
    assert_eq!(empty["diagnostics"][0]["blocks_export"], true);
    let exports_after = std::fs::read_dir(project.join(".doublelove/exports"))
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(exports_before, exports_after, "失败不得新增导出文件");

    std::fs::remove_dir_all(&dir).expect("temp dir removed");
}
