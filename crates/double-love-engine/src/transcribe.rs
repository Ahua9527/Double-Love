//! 转录管线：sidecar 事件流 → transcript_word 批事务落库。
//! - 每个 words 事件一个事务（chunk 粒度），但写入不可见候选版本
//! - 完整成功后才原子切换活动版本；取消/失败保留旧文本与删改
//! - 取消/失败/协议异常都经 ProgressEvent 与终态上报，不静默

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::sidecar::{Sidecar, SidecarCommand, SidecarEvent, SidecarPoll, resolve_python};
use crate::storage::{NewTranscriptWord, ProjectStore};
use crate::task::{SharedSink, TaskRegistry};
use crate::{ProgressEvent, TaskState};

/// 转录任务配置。
pub struct TranscribeConfig {
    pub asset_id: String,
    /// qwen3-asr-1.7b（默认）/ qwen3-asr-0.6b
    pub model: String,
    /// 模型管理器解析出的本地绝对 ASR 权重目录。
    pub model_dir: PathBuf,
    /// 模型管理器解析出的本地绝对 ForcedAligner 权重目录。
    pub aligner_dir: PathBuf,
    /// auto / zh / en（auto 由引擎检测）
    pub language: String,
    /// 测试与开发自举用 mock 引擎
    pub mock: bool,
    /// 显式 python 路径；None 时按 venv → PATH 发现
    pub python: Option<PathBuf>,
    /// double_love_asr 包目录（sidecars/asr）
    pub package_dir: PathBuf,
    /// sidecar stderr 日志目录
    pub log_dir: PathBuf,
    /// 切块秒数，默认 30
    pub chunk_seconds: i64,
}

const SUPPORTED_MODELS: [&str; 2] = ["qwen3-asr-1.7b", "qwen3-asr-0.6b"];
/// sidecar 事件静默上限：模型加载+长 chunk 推理可能较慢，给足 2 分钟
const EVENT_SILENCE_LIMIT: Duration = Duration::from_secs(120);

/// 启动转录任务（异步）。同步段做前置校验，失败直接 Err；
/// Ok 返回 task_id，进度与终态经 sink 上报。
pub fn start_transcription(
    store: Arc<Mutex<ProjectStore>>,
    registry: &TaskRegistry,
    sink: SharedSink,
    config: TranscribeConfig,
) -> Result<String, String> {
    if !SUPPORTED_MODELS.contains(&config.model.as_str()) {
        return Err(format!(
            "不支持的模型：{}（支持：{}）",
            config.model,
            SUPPORTED_MODELS.join("、")
        ));
    }

    // 前置校验（同步）：资产存在、已抽取准备音频
    let (wav_path, source_sample_rate) = {
        let guard = store.lock().map_err(|_| "存储锁不可用".to_string())?;
        let asset = guard
            .media_asset(&config.asset_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("资产不存在：{}", config.asset_id))?;
        let wav = asset
            .prepared_wav_path
            .clone()
            .ok_or_else(|| "资产尚未抽取准备音频（请先 import-media）".to_string())?;
        (wav, asset.audio_sample_rate)
    };

    let python = resolve_python(
        config.python.as_deref(),
        &config.package_dir.join(".venv/bin/python"),
    )
    .ok_or_else(|| {
        "找不到 python 解释器（请运行 scripts/prepare-asr.sh 或确认 python3 可用）".to_string()
    })?;

    let task_id = Uuid::new_v4().to_string();
    let worker_id = task_id.clone();
    let worker_store = Arc::clone(&store);
    registry.spawn(&task_id, sink, move |token, sink| {
        run_transcription(
            &worker_id,
            &worker_store,
            &sink,
            &config,
            &python,
            &wav_path,
            source_sample_rate,
            &token,
        )
    })?;
    Ok(task_id)
}

#[allow(clippy::too_many_arguments)]
fn run_transcription(
    task_id: &str,
    store: &Arc<Mutex<ProjectStore>>,
    sink: &SharedSink,
    config: &TranscribeConfig,
    python: &std::path::Path,
    wav_path: &str,
    source_sample_rate: i64,
    token: &crate::task::CancellationToken,
) -> TaskState {
    let report = |phase: &str, message: String| {
        sink.progress(ProgressEvent {
            task: task_id.to_string(),
            phase: phase.to_string(),
            completed: None,
            total: None,
            message,
        });
    };

    let log_path = config.log_dir.join(format!("asr-{task_id}.log"));
    let mut sidecar = match Sidecar::spawn(python, &config.package_dir, config.mock, &log_path) {
        Ok(sidecar) => sidecar,
        Err(error) => {
            report("error", format!("sidecar 启动失败：{error}"));
            return TaskState::Failed;
        }
    };
    if let Err(error) = sidecar.send(&SidecarCommand::Transcribe {
        task_id: task_id.to_string(),
        wav_path: wav_path.to_string(),
        model: config.model.clone(),
        model_dir: config.model_dir.to_string_lossy().into_owned(),
        aligner_dir: config.aligner_dir.to_string_lossy().into_owned(),
        language: config.language.clone(),
        source_sample_rate,
        chunk_seconds: config.chunk_seconds,
    }) {
        report("error", format!("转录命令发送失败：{error}"));
        return TaskState::Failed;
    }

    // 新词先进入不可见的候选 run；当前版本（及其 omit）在整个任务期间保持可用。
    let run_id = match store
        .lock()
        .map_err(|_| "存储锁不可用".to_string())
        .and_then(|guard| {
            guard
                .begin_transcript_run(&config.asset_id, &config.model, &config.language)
                .map(|run| run.id)
                .map_err(|error| error.to_string())
        }) {
        Ok(run_id) => run_id,
        Err(error) => {
            report("error", format!("建立候选转录版本失败：{error}"));
            return TaskState::Failed;
        }
    };
    report(
        "progress",
        "正在生成新的候选转录；旧版本会保留到新版本完整成功。".to_string(),
    );

    let finish_candidate = |status: &str| {
        store
            .lock()
            .map_err(|_| "存储锁不可用".to_string())
            .and_then(|guard| {
                guard
                    .finish_transcript_run(&run_id, status)
                    .map_err(|error| error.to_string())
            })
    };

    let mut next_ordinal: i64 = 0;
    let mut had_error = false;
    let mut cancel_sent = false;
    let mut last_event = Instant::now();

    loop {
        if token.is_cancelled() && !cancel_sent {
            let _ = sidecar.send(&SidecarCommand::Cancel {
                task_id: task_id.to_string(),
            });
            cancel_sent = true;
        }
        if last_event.elapsed() > EVENT_SILENCE_LIMIT {
            report(
                "error",
                format!(
                    "sidecar 超过 {} 秒无事件，判定挂起",
                    EVENT_SILENCE_LIMIT.as_secs()
                ),
            );
            let _ = finish_candidate("failed");
            return TaskState::Failed;
        }
        let event = match sidecar.next_event(Duration::from_secs(1)) {
            SidecarPoll::Event(event) => event,
            SidecarPoll::TimedOut => continue, // 1 秒轮询，便于及时响应取消
            SidecarPoll::Closed => {
                report(
                    "error",
                    "sidecar 已关闭 stdout，转录未给出终态。".to_string(),
                );
                let _ = finish_candidate("failed");
                return TaskState::Failed;
            }
        };
        last_event = Instant::now();

        match event {
            Ok(SidecarEvent::Words { words, .. }) => {
                let batch: Vec<NewTranscriptWord> = words
                    .into_iter()
                    .map(|word| {
                        let ordinal = next_ordinal;
                        next_ordinal += 1;
                        NewTranscriptWord {
                            word_id: Uuid::new_v4().to_string(),
                            asset_id: config.asset_id.clone(),
                            ordinal,
                            raw_text: word.raw_text,
                            display_text: word.display_text,
                            language: word.language,
                            start_sample: word.start_sample,
                            end_sample: word.end_sample,
                            confidence: word.confidence,
                        }
                    })
                    .collect();
                let insert = store
                    .lock()
                    .map_err(|_| "存储锁不可用".to_string())
                    .and_then(|guard| {
                        guard
                            .insert_transcript_words_for_run(&run_id, &batch)
                            .map_err(|error| error.to_string())
                    });
                if let Err(error) = insert {
                    report("error", format!("转录词落库失败：{error}"));
                    let _ = finish_candidate("failed");
                    return TaskState::Failed;
                }
            }
            Ok(SidecarEvent::Progress {
                completed,
                total,
                message,
                ..
            }) => {
                sink.progress(ProgressEvent {
                    task: task_id.to_string(),
                    phase: "transcribe".to_string(),
                    completed,
                    total,
                    message,
                });
            }
            Ok(SidecarEvent::Done { word_count, .. }) => {
                if had_error || word_count != next_ordinal as u64 {
                    let cause = if had_error {
                        "sidecar 报告了非致命错误"
                    } else {
                        "sidecar 的词数与实际写入不一致"
                    };
                    report(
                        "error",
                        format!("候选转录未切换：{cause}；旧版本保持不变。"),
                    );
                    let _ = finish_candidate("partial");
                    return TaskState::Partial;
                }
                let activate = store
                    .lock()
                    .map_err(|_| "存储锁不可用".to_string())
                    .and_then(|guard| {
                        guard
                            .activate_transcript_run(&run_id)
                            .map_err(|error| error.to_string())
                    });
                if let Err(error) = activate {
                    report("error", format!("切换转录版本失败：{error}"));
                    let _ = finish_candidate("failed");
                    return TaskState::Failed;
                }
                report(
                    "progress",
                    format!("转录完成：{word_count} 词（ordinal 0..{next_ordinal}）"),
                );
                return TaskState::Succeeded;
            }
            Ok(SidecarEvent::Cancelled { .. }) => {
                report(
                    "progress",
                    format!(
                        "已取消：丢弃候选显示，旧版本保持不变（候选已写入 {next_ordinal} 词）。"
                    ),
                );
                let _ = finish_candidate("cancelled");
                return TaskState::Cancelled;
            }
            Ok(SidecarEvent::Error {
                code,
                message,
                fatal,
                ..
            }) => {
                report("error", format!("{code}: {message}"));
                if fatal {
                    let _ = finish_candidate("failed");
                    return TaskState::Failed;
                }
                had_error = true;
            }
            Ok(SidecarEvent::Ready { .. }) => {} // 握手期已消费；重复 ready 忽略
            Ok(SidecarEvent::SpeakerSegments { .. } | SidecarEvent::DiarizationDone { .. }) => {
                report(
                    "error",
                    "ASR sidecar 返回了不属于转录任务的事件。".to_string(),
                );
                let _ = finish_candidate("failed");
                return TaskState::Failed;
            }
            Err(reason) => {
                report("error", format!("sidecar 协议错误：{reason}"));
                let _ = finish_candidate("failed");
                return TaskState::Failed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::NewMediaAsset;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("double-love-transcribe-{label}-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_test_wav(path: &Path, seconds: u32) {
        let rate = 16_000_u32;
        let samples = seconds * rate;
        let data_len = samples * 2;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.resize(44 + data_len as usize, 0);
        std::fs::write(path, wav).expect("test wav");
    }

    fn fixture_store(dir: &Path, wav: &Path) -> Arc<Mutex<ProjectStore>> {
        let store = ProjectStore::open(&dir.join("project.sqlite")).expect("store");
        store
            .insert_media_asset(&NewMediaAsset {
                id: "asset-1".to_string(),
                kind: "video".to_string(),
                original_path: dir.join("synthetic.mp4").to_string_lossy().into_owned(),
                display_name: "synthetic.mp4".to_string(),
                duration_samples: 480_000,
                audio_sample_rate: 48_000,
                fps_num: 25,
                fps_den: 1,
                video_timebase: 25,
                is_ntsc: false,
                width: Some(1920),
                height: Some(1080),
                audio_channels: Some(2),
                source_tc_start_frame: None,
                source_tc_is_drop_frame: false,
                ffprobe_json: "{}".to_string(),
            })
            .expect("asset inserts");
        store
            .set_asset_prepared("asset-1", &wav.to_string_lossy())
            .expect("prepared set");
        Arc::new(Mutex::new(store))
    }

    fn test_config(dir: &Path, mock: bool) -> TranscribeConfig {
        TranscribeConfig {
            asset_id: "asset-1".to_string(),
            model: "qwen3-asr-1.7b".to_string(),
            model_dir: dir.join("models/qwen3-asr-1.7b"),
            aligner_dir: dir.join("models/qwen3-forced-aligner-0.6b"),
            language: "auto".to_string(),
            mock,
            python: None,
            package_dir: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../sidecars/asr")
                .canonicalize()
                .expect("package dir"),
            log_dir: dir.join("logs"),
            chunk_seconds: 1,
        }
    }

    struct NullSink;
    impl crate::task::ProgressSink for NullSink {
        fn progress(&self, _event: ProgressEvent) {}
        fn task_state(&self, _task_id: &str, _state: TaskState) {}
    }

    fn wait_terminal(registry: &TaskRegistry, task_id: &str) -> TaskState {
        for _ in 0..600 {
            if let Some(state) = registry.state(task_id)
                && !matches!(state, TaskState::Pending | TaskState::Running)
            {
                return state;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("task never reached terminal state");
    }

    #[test]
    fn transcribes_mock_words_into_storage() {
        let Some(_) = resolve_python(None, Path::new("/nonexistent-venv")) else {
            eprintln!("skip: python3 not found");
            return;
        };
        let dir = temp_dir("flow");
        let wav = dir.join("prepared.wav");
        write_test_wav(&wav, 3);
        let store = fixture_store(&dir, &wav);
        let registry = TaskRegistry::new();

        let task_id = start_transcription(
            Arc::clone(&store),
            &registry,
            Arc::new(NullSink),
            test_config(&dir, true),
        )
        .expect("task starts");
        assert_eq!(wait_terminal(&registry, &task_id), TaskState::Succeeded);

        let guard = store.lock().expect("store lock");
        assert_eq!(guard.count_transcript_words("asset-1").expect("count"), 6);
        let status = guard
            .media_asset("asset-1")
            .expect("asset")
            .expect("exists")
            .status;
        assert_eq!(status, "transcribed");
        drop(guard);
        registry.shutdown();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancel_preserves_the_previous_active_transcript() {
        let Some(_) = resolve_python(None, Path::new("/nonexistent-venv")) else {
            eprintln!("skip: python3 not found");
            return;
        };
        let dir = temp_dir("cancel");
        let wav = dir.join("prepared.wav");
        write_test_wav(&wav, 30);
        let store = fixture_store(&dir, &wav);
        {
            let guard = store.lock().expect("store lock");
            guard
                .insert_transcript_words(&[
                    NewTranscriptWord {
                        word_id: "old-0".to_string(),
                        asset_id: "asset-1".to_string(),
                        ordinal: 0,
                        raw_text: "旧文本".to_string(),
                        display_text: "旧文本".to_string(),
                        language: Some("zh".to_string()),
                        start_sample: 0,
                        end_sample: 24_000,
                        confidence: Some(0.99),
                    },
                    NewTranscriptWord {
                        word_id: "old-1".to_string(),
                        asset_id: "asset-1".to_string(),
                        ordinal: 1,
                        raw_text: "保留".to_string(),
                        display_text: "保留".to_string(),
                        language: Some("zh".to_string()),
                        start_sample: 24_000,
                        end_sample: 48_000,
                        confidence: Some(0.99),
                    },
                ])
                .expect("old transcript");
        }
        let previous_run = store
            .lock()
            .expect("store lock")
            .active_transcript_run_id("asset-1")
            .expect("active run")
            .expect("old active run");
        let registry = TaskRegistry::new();

        let task_id = start_transcription(
            Arc::clone(&store),
            &registry,
            Arc::new(NullSink),
            test_config(&dir, true),
        )
        .expect("task starts");
        // 等候选版本至少写入一批词后取消；它不应出现在活动文本里。
        let mut waited = 0;
        loop {
            let has_candidate_words = {
                let guard = store.lock().expect("store lock");
                guard
                    .transcript_runs("asset-1")
                    .expect("runs")
                    .iter()
                    .any(|run| {
                        run.id != previous_run && run.status == "running" && run.word_count > 0
                    })
            };
            if has_candidate_words || waited > 100 {
                break;
            }
            waited += 1;
            std::thread::sleep(Duration::from_millis(50));
        }
        registry.cancel(&task_id);
        assert_eq!(wait_terminal(&registry, &task_id), TaskState::Cancelled);

        let guard = store.lock().expect("store lock");
        let kept = guard.transcript_words("asset-1").expect("words");
        assert_eq!(kept.len(), 2, "旧版本必须仍可编辑");
        assert_eq!(kept[0].display_text, "旧文本");
        assert_eq!(
            guard
                .active_transcript_run_id("asset-1")
                .expect("active run"),
            Some(previous_run),
            "取消不得切换候选版本"
        );
        assert!(
            guard
                .transcript_runs("asset-1")
                .expect("runs")
                .iter()
                .any(|run| run.status == "cancelled" && run.word_count > 0)
        );
        let status = guard
            .media_asset("asset-1")
            .expect("asset")
            .expect("exists")
            .status;
        assert_eq!(status, "transcribed", "旧成功版本的状态必须保留");
        drop(guard);
        registry.shutdown();
        std::fs::remove_dir_all(&dir).ok();
    }
}
