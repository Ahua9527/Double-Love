//! mock sidecar 全流程：hello → transcribe → words/progress → done；cancel 路径。
//! 无 python3 的环境跳过（本机有，会真实起子进程）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use double_love_engine::{Sidecar, SidecarCommand, SidecarEvent, SidecarPoll, resolve_python};

fn package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sidecars/asr")
        .canonicalize()
        .expect("sidecar package dir exists")
}

fn write_test_wav(path: &Path, seconds: u32) {
    let rate = 16_000_u32;
    let samples = seconds * rate;
    let data_len = samples * 2;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.resize(44 + data_len as usize, 0);
    std::fs::write(path, wav).expect("test wav written");
}

fn temp_file(label: &str, ext: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("double-love-sidecar-{label}-{unique}.{ext}"))
}

fn transcribe_cmd(task_id: &str, wav: &Path, chunk_seconds: i64) -> SidecarCommand {
    SidecarCommand::Transcribe {
        task_id: task_id.to_string(),
        wav_path: wav.to_string_lossy().into_owned(),
        model: "qwen3-asr-1.7b".to_string(),
        model_dir: "/tmp/double-love-test-asr".to_string(),
        aligner_dir: "/tmp/double-love-test-aligner".to_string(),
        language: "auto".to_string(),
        source_sample_rate: 48_000,
        chunk_seconds,
    }
}

#[test]
fn mock_sidecar_transcribes_full_flow() {
    let Some(python) = resolve_python(None, &package_dir().join(".venv/bin/python")) else {
        eprintln!("skip: python3 not found");
        return;
    };
    let wav = temp_file("flow", "wav");
    write_test_wav(&wav, 3);
    let log = temp_file("flow", "log");

    let mut sidecar =
        Sidecar::spawn(&python, &package_dir(), true, &log).expect("sidecar spawns and handshakes");
    sidecar
        .send(&transcribe_cmd("task-flow", &wav, 1))
        .expect("transcribe command sends");

    let mut words_seen = Vec::new();
    let mut progress_seen = 0;
    let mut done_count = None;
    for _ in 0..40 {
        match sidecar.next_event(Duration::from_secs(5)) {
            SidecarPoll::Event(Ok(SidecarEvent::Words { chunk, words, .. })) => {
                for word in words {
                    words_seen.push((chunk, word.raw_text, word.start_sample, word.end_sample));
                }
            }
            SidecarPoll::Event(Ok(SidecarEvent::Progress { .. })) => progress_seen += 1,
            SidecarPoll::Event(Ok(SidecarEvent::Done { word_count, .. })) => {
                done_count = Some(word_count);
                break;
            }
            SidecarPoll::Event(Ok(other)) => panic!("unexpected event: {other:?}"),
            SidecarPoll::Event(Err(reason)) => panic!("protocol error: {reason}"),
            SidecarPoll::TimedOut => panic!("timed out waiting for events"),
            SidecarPoll::Closed => panic!("sidecar closed before done"),
        }
    }

    assert_eq!(done_count, Some(6), "3 段 × 2 词");
    assert_eq!(progress_seen, 3);
    // 词序与采样域：chunk0 词0 = [0, 14400]，词1 起于 0.5s=24000；chunk1 词0 起于 1s=48000
    assert_eq!(words_seen[0], (0, "开拍".to_string(), 0, 14_400));
    assert_eq!(words_seen[1], (0, "镜一".to_string(), 24_000, 38_400));
    assert_eq!(words_seen[2], (1, "开拍".to_string(), 48_000, 62_400));
    // 采样严格递增、首尾不相交
    for pair in words_seen.windows(2) {
        assert!(pair[0].3 <= pair[1].2, "words must not overlap");
    }

    drop(sidecar); // Drop：stdin EOF → sidecar 自行退出
    std::fs::remove_file(&wav).ok();
    std::fs::remove_file(&log).ok();
}

#[test]
fn mock_sidecar_honours_cancel() {
    let Some(python) = resolve_python(None, &package_dir().join(".venv/bin/python")) else {
        eprintln!("skip: python3 not found");
        return;
    };
    let wav = temp_file("cancel", "wav");
    write_test_wav(&wav, 10);
    let log = temp_file("cancel", "log");

    let mut sidecar = Sidecar::spawn(&python, &package_dir(), true, &log).expect("sidecar spawns");
    sidecar
        .send(&transcribe_cmd("task-cancel", &wav, 1))
        .expect("transcribe command sends");

    // 等首个 words 事件后立刻取消
    loop {
        match sidecar.next_event(Duration::from_secs(5)) {
            SidecarPoll::Event(Ok(SidecarEvent::Words { .. })) => break,
            SidecarPoll::Event(Ok(SidecarEvent::Progress { .. })) => continue,
            SidecarPoll::Event(Ok(other)) => panic!("unexpected event before cancel: {other:?}"),
            SidecarPoll::Event(Err(reason)) => panic!("protocol error: {reason}"),
            SidecarPoll::TimedOut => panic!("timed out waiting for first words"),
            SidecarPoll::Closed => panic!("sidecar closed before first words"),
        }
    }
    sidecar
        .send(&SidecarCommand::Cancel {
            task_id: "task-cancel".to_string(),
        })
        .expect("cancel sends");

    let mut cancelled = false;
    for _ in 0..40 {
        match sidecar.next_event(Duration::from_secs(5)) {
            SidecarPoll::Event(Ok(SidecarEvent::Cancelled { task_id })) => {
                assert_eq!(task_id, "task-cancel");
                cancelled = true;
                break;
            }
            SidecarPoll::Event(Ok(SidecarEvent::Done { .. })) => {
                panic!("cancel 后不应出现 done（10 段不可能瞬间转完）")
            }
            SidecarPoll::Event(Ok(_)) => continue,
            SidecarPoll::Event(Err(reason)) => panic!("protocol error: {reason}"),
            SidecarPoll::TimedOut => panic!("timed out waiting for cancelled"),
            SidecarPoll::Closed => panic!("sidecar closed before cancelled"),
        }
    }
    assert!(cancelled);

    drop(sidecar);
    std::fs::remove_file(&wav).ok();
    std::fs::remove_file(&log).ok();
}
