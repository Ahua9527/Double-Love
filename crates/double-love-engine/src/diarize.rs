//! 本地说话人分离任务：Silero VAD + WeSpeaker sidecar → 项目级身份/词归属。
//!
//! 声纹向量只在此模块和 `speaker_embedding` 本地表中流转；绝不写日志、OperationResult、
//! Agent payload 或导出文件。sidecar 失败/取消时不触碰旧的 SpeakerSegment 与词归属。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::sidecar::{Sidecar, SidecarCommand, SidecarEvent, SidecarPoll, resolve_python};
use crate::storage::ProjectStore;
use crate::task::{SharedSink, TaskRegistry};
use crate::{
    ProgressEvent, SpeakerDiarizationResult, SpeakerIdentity, SpeakerSegment, TaskState,
    assign_words_to_speakers, merge_proposals_from_embeddings,
};

const EVENT_SILENCE_LIMIT: Duration = Duration::from_secs(120);
const SPEAKER_COLORS: [&str; 8] = [
    "#2563EB", "#0F9D73", "#B45309", "#9333EA", "#DB2777", "#0891B2", "#65A30D", "#DC2626",
];

pub struct DiarizeConfig {
    pub asset_id: String,
    pub mock: bool,
    pub python: Option<PathBuf>,
    pub package_dir: PathBuf,
    pub log_dir: PathBuf,
    /// 模型管理器解析出的 bundled Silero VAD 目录或运行时标识。
    pub vad_model_dir: PathBuf,
    /// 模型管理器解析出的本地 WeSpeaker 权重目录。
    pub speaker_model_dir: PathBuf,
}

/// 启动本地说话人分离；成功前所有结果都只在 worker 内存中，防止失败覆盖已确认身份。
pub fn start_speaker_diarization(
    store: Arc<Mutex<ProjectStore>>,
    registry: &TaskRegistry,
    sink: SharedSink,
    config: DiarizeConfig,
) -> Result<String, String> {
    let (wav_path, source_sample_rate, word_count) = {
        let guard = store.lock().map_err(|_| "存储锁不可用".to_string())?;
        let asset = guard
            .media_asset(&config.asset_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("素材不存在：{}", config.asset_id))?;
        let wav = asset
            .prepared_wav_path
            .clone()
            .ok_or_else(|| "素材尚未抽取准备音频（请先导入媒体）。".to_string())?;
        let count = guard
            .count_transcript_words(&config.asset_id)
            .map_err(|error| error.to_string())?;
        (wav, asset.audio_sample_rate, count)
    };
    if word_count == 0 {
        return Err("请先完成转录，再进行说话人分离。".to_string());
    }
    let python = resolve_python(
        config.python.as_deref(),
        &config.package_dir.join(".venv/bin/python"),
    )
    .ok_or_else(|| {
        "找不到说话人模型 Python 环境（请运行 scripts/prepare-speaker.sh）。".to_string()
    })?;
    let task_id = Uuid::new_v4().to_string();
    let worker_id = task_id.clone();
    let worker_store = Arc::clone(&store);
    registry.spawn(&task_id, sink, move |token, sink| {
        run_diarization(
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
fn run_diarization(
    task_id: &str,
    store: &Arc<Mutex<ProjectStore>>,
    sink: &SharedSink,
    config: &DiarizeConfig,
    python: &std::path::Path,
    wav_path: &str,
    source_sample_rate: i64,
    token: &crate::CancellationToken,
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
    let log_path = config.log_dir.join(format!("speaker-{task_id}.log"));
    let mut sidecar = match Sidecar::spawn_module(
        python,
        &config.package_dir,
        "double_love_speaker",
        config.mock,
        &log_path,
    ) {
        Ok(sidecar) => sidecar,
        Err(error) => {
            report("error", format!("说话人 sidecar 启动失败：{error}"));
            return TaskState::Failed;
        }
    };
    if let Err(error) = sidecar.send(&SidecarCommand::Diarize {
        task_id: task_id.to_string(),
        wav_path: wav_path.to_string(),
        vad_model_dir: config.vad_model_dir.to_string_lossy().into_owned(),
        speaker_model_dir: config.speaker_model_dir.to_string_lossy().into_owned(),
        source_sample_rate,
    }) {
        report("error", format!("说话人命令发送失败：{error}"));
        return TaskState::Failed;
    }

    let mut received_segments = None;
    let mut last_event = Instant::now();
    let mut cancel_sent = false;
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
                "说话人 sidecar 超过 120 秒无事件，已停止。".to_string(),
            );
            return TaskState::Failed;
        }
        let event = match sidecar.next_event(Duration::from_secs(1)) {
            SidecarPoll::Event(event) => event,
            SidecarPoll::TimedOut => continue,
            SidecarPoll::Closed => {
                report(
                    "error",
                    "说话人 sidecar 已关闭 stdout，未给出终态。".to_string(),
                );
                return TaskState::Failed;
            }
        };
        last_event = Instant::now();
        match event {
            Ok(SidecarEvent::Progress {
                completed,
                total,
                message,
                ..
            }) => sink.progress(ProgressEvent {
                task: task_id.to_string(),
                phase: "diarize".to_string(),
                completed,
                total,
                message,
            }),
            Ok(SidecarEvent::SpeakerSegments {
                segments,
                embeddings,
                ..
            }) => received_segments = Some((segments, embeddings)),
            Ok(SidecarEvent::DiarizationDone { segment_count, .. }) => {
                let Some((segments, embeddings)) = received_segments.take() else {
                    report("error", "说话人 sidecar 未返回任何区间。".to_string());
                    return TaskState::Failed;
                };
                if segment_count != segments.len() as u64 {
                    report("error", "说话人区间数量不一致，结果未应用。".to_string());
                    return TaskState::Failed;
                }
                return apply_diarization(store, &config.asset_id, segments, embeddings, &report);
            }
            Ok(SidecarEvent::Cancelled { .. }) => {
                report(
                    "progress",
                    "说话人分离已取消，旧身份与归属保持不变。".to_string(),
                );
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
                    return TaskState::Failed;
                }
            }
            Ok(SidecarEvent::Ready { .. }) => {}
            Ok(other) => {
                report("error", format!("说话人 sidecar 返回了无效事件：{other:?}"));
                return TaskState::Failed;
            }
            Err(reason) => {
                report("error", format!("说话人 sidecar 协议错误：{reason}"));
                return TaskState::Failed;
            }
        }
    }
}

fn apply_diarization(
    store: &Arc<Mutex<ProjectStore>>,
    asset_id: &str,
    raw_segments: Vec<crate::SidecarSpeakerSegment>,
    embeddings: Vec<crate::SidecarSpeakerEmbedding>,
    report: &impl Fn(&str, String),
) -> TaskState {
    let mut cluster_ids = raw_segments
        .iter()
        .map(|segment| segment.cluster_id.clone())
        .collect::<Vec<_>>();
    cluster_ids.sort();
    cluster_ids.dedup();
    let identity_by_cluster: HashMap<String, SpeakerIdentity> = cluster_ids
        .iter()
        .enumerate()
        .map(|(index, cluster)| {
            (
                cluster.clone(),
                SpeakerIdentity {
                    id: Uuid::new_v4().to_string(),
                    display_name: format!("说话人 {}", index + 1),
                    aliases: Vec::new(),
                    color: SPEAKER_COLORS[index % SPEAKER_COLORS.len()].to_string(),
                    confirmed: false,
                },
            )
        })
        .collect();
    let segments = raw_segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            identity_by_cluster
                .get(&segment.cluster_id)
                .map(|identity| SpeakerSegment {
                    id: format!("speaker:{asset_id}:{index}"),
                    asset_id: asset_id.to_string(),
                    speaker_id: identity.id.clone(),
                    start_sample: segment.start_sample,
                    end_sample: segment.end_sample,
                    confidence: segment.confidence,
                })
        })
        .collect::<Vec<_>>();
    let identities = cluster_ids
        .iter()
        .filter_map(|cluster| identity_by_cluster.get(cluster).cloned())
        .collect::<Vec<_>>();
    let embedding_updates = embeddings
        .into_iter()
        .filter_map(|embedding| {
            identity_by_cluster
                .get(&embedding.cluster_id)
                .map(|identity| (identity.id.clone(), embedding.values))
        })
        .collect::<Vec<_>>();
    let guard = match store.lock() {
        Ok(guard) => guard,
        Err(_) => {
            report("error", "存储锁不可用。".to_string());
            return TaskState::Failed;
        }
    };
    let words = match guard.transcript_words(asset_id) {
        Ok(words) => words,
        Err(error) => {
            report("error", format!("读取转录词失败：{error}"));
            return TaskState::Failed;
        }
    };
    let assignments = assign_words_to_speakers(&words, &segments);
    let mut all_embeddings = match guard.speaker_embeddings() {
        Ok(existing) => existing.into_iter().collect::<BTreeMap<_, _>>(),
        Err(error) => {
            report("error", format!("读取本地声纹向量失败：{error}"));
            return TaskState::Failed;
        }
    };
    for (speaker_id, values) in &embedding_updates {
        all_embeddings.insert(speaker_id.clone(), values.clone());
    }
    let proposals =
        merge_proposals_from_embeddings(&all_embeddings.into_iter().collect::<Vec<_>>(), 0.82);
    if let Err(error) = guard.apply_speaker_diarization(
        asset_id,
        &identities,
        &segments,
        &assignments,
        &embedding_updates,
        &proposals,
    ) {
        report("error", format!("应用说话人分离结果失败：{error}"));
        return TaskState::Failed;
    }
    report(
        "progress",
        format!(
            "说话人分离完成：{} 个区间、{} 位匿名说话人；跨素材候选必须人工确认。",
            segments.len(),
            identities.len()
        ),
    );
    TaskState::Succeeded
}

/// 当前可展示的说话人结果摘要；向量不在返回值中。
pub fn speaker_diarization_result(
    store: &ProjectStore,
    asset_id: &str,
) -> crate::OperationResult<SpeakerDiarizationResult> {
    let segments = match store.speaker_segments(asset_id) {
        Ok(segments) => segments,
        Err(error) => return crate::OperationResult::failed("STORAGE_ERROR", error.to_string()),
    };
    let all = match store.speaker_identities() {
        Ok(identities) => identities,
        Err(error) => return crate::OperationResult::failed("STORAGE_ERROR", error.to_string()),
    };
    let active_ids = segments
        .iter()
        .map(|segment| segment.speaker_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let speakers = all
        .into_iter()
        .filter(|identity| active_ids.contains(identity.id.as_str()))
        .collect();
    let merge_proposals = match store.speaker_merge_proposals() {
        Ok(proposals) => proposals,
        Err(error) => return crate::OperationResult::failed("STORAGE_ERROR", error.to_string()),
    };
    crate::OperationResult::success(SpeakerDiarizationResult {
        asset_id: asset_id.to_string(),
        segment_count: segments.len() as u64,
        speakers,
        merge_proposals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{NewMediaAsset, NewTranscriptWord};

    fn temp_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("double-love-diarize-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn write_wav(path: &std::path::Path) {
        let samples = 16_000_u32 * 3;
        let bytes = samples * 2;
        let mut wav = Vec::with_capacity(44 + bytes as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&32_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&bytes.to_le_bytes());
        wav.resize(44 + bytes as usize, 0);
        std::fs::write(path, wav).expect("wav");
    }

    struct Sink;
    impl crate::ProgressSink for Sink {
        fn progress(&self, _event: ProgressEvent) {}
        fn task_state(&self, _task_id: &str, _state: TaskState) {}
    }

    #[test]
    fn mock_diarization_assigns_words_without_exposing_embeddings() {
        let Some(_) = resolve_python(None, std::path::Path::new("/nonexistent")) else {
            eprintln!("skip: python3 not found");
            return;
        };
        let dir = temp_dir();
        let wav = dir.join("prepared.wav");
        write_wav(&wav);
        let store = ProjectStore::open(&dir.join("project.sqlite")).expect("store");
        store
            .insert_media_asset(&NewMediaAsset {
                id: "a".to_string(),
                kind: "video".to_string(),
                original_path: "/tmp/a.mov".to_string(),
                display_name: "a.mov".to_string(),
                duration_samples: 144_000,
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
            .expect("asset");
        store
            .set_asset_prepared("a", &wav.to_string_lossy())
            .expect("prepared");
        store
            .insert_transcript_words(&[NewTranscriptWord {
                word_id: "w".to_string(),
                asset_id: "a".to_string(),
                ordinal: 0,
                raw_text: "你好".to_string(),
                display_text: "你好".to_string(),
                language: Some("zh".to_string()),
                start_sample: 0,
                end_sample: 48_000,
                confidence: Some(0.99),
            }])
            .expect("words");
        let store = Arc::new(Mutex::new(store));
        let registry = TaskRegistry::new();
        let config = DiarizeConfig {
            asset_id: "a".to_string(),
            mock: true,
            python: None,
            package_dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../sidecars/speaker")
                .canonicalize()
                .expect("speaker sidecar"),
            log_dir: dir.join("logs"),
            vad_model_dir: dir.join("models/silero-vad"),
            speaker_model_dir: dir.join("models/wespeaker-zh"),
        };
        let task_id =
            start_speaker_diarization(Arc::clone(&store), &registry, Arc::new(Sink), config)
                .expect("starts");
        let state = (0..200)
            .find_map(|_| {
                let state = registry.state(&task_id)?;
                if matches!(state, TaskState::Pending | TaskState::Running) {
                    std::thread::sleep(Duration::from_millis(20));
                    None
                } else {
                    Some(state)
                }
            })
            .expect("terminal state");
        assert_eq!(state, TaskState::Succeeded);
        let guard = store.lock().expect("store");
        let result = speaker_diarization_result(&guard, "a")
            .data
            .expect("result");
        assert_eq!(result.segment_count, 1);
        assert_eq!(result.speakers.len(), 1);
        assert_eq!(
            guard.transcript_words("a").expect("words")[0]
                .speaker_assignments
                .len(),
            1
        );
        assert_eq!(
            guard.speaker_embeddings().expect("local embeddings").len(),
            1
        );
        assert_eq!(
            guard.revision_history(1).expect("history")[0].operation,
            "speaker_diarize"
        );
        assert!(guard.revision_history(1).expect("history")[0].restorable);
        drop(guard);
        registry.shutdown();
        std::fs::remove_dir_all(dir).ok();
    }
}
