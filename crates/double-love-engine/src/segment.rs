//! 中文句子分段：把词序列切成 TranscriptView 的渲染段落（纯函数，不落表）。
//! 断段规则（命中其一即断）：
//!   1. 句末标点（。！？；…）+ 后随停顿 > 0.8s（双条件）
//!   2. 词间停顿 > 2s（硬断，不看标点）
//!   3. 段长达到 120 字（硬上限）

use crate::contracts::{TranscriptSegment, WordAnchor};

const SENTENCE_PUNCT: [char; 5] = ['。', '！', '？', '；', '…'];
const PAUSE_AFTER_PUNCT_MS: i64 = 800;
const HARD_PAUSE_MS: i64 = 2_000;
const MAX_SEGMENT_CHARS: usize = 120;

fn ends_with_sentence_punct(text: &str) -> bool {
    text.chars()
        .last()
        .is_some_and(|c| SENTENCE_PUNCT.contains(&c))
}

/// 相邻两词的停顿（毫秒，负重叠按 0 计）。
fn gap_ms(current: &WordAnchor, next: &WordAnchor, sample_rate: i64) -> i64 {
    (next.start_sample - current.end_sample).max(0) * 1_000 / sample_rate
}

/// 拼接显示文本：两侧都是 ASCII 字母数字时补空格，其余直接相连。
fn join_text(existing: &mut String, next: &str) {
    let needs_space = existing
        .chars()
        .last()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && next
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric());
    if needs_space {
        existing.push(' ');
    }
    existing.push_str(next);
}

/// 词序列 + 活跃 omit 区间 → 分段。omit_ranges 为词序闭区间。
pub fn segment_words(
    words: &[WordAnchor],
    sample_rate: i64,
    omit_ranges: &[(i64, i64)],
) -> Vec<TranscriptSegment> {
    let mut segments = Vec::new();
    let mut start = 0_usize;
    while start < words.len() {
        let mut end = start;
        let mut text = String::new();
        loop {
            join_text(&mut text, &words[end].display_text);
            let at_last = end + 1 >= words.len();
            let hit_cap = text.chars().count() >= MAX_SEGMENT_CHARS;
            let break_here = at_last || hit_cap || {
                let gap = gap_ms(&words[end], &words[end + 1], sample_rate);
                gap > HARD_PAUSE_MS
                    || (gap > PAUSE_AFTER_PUNCT_MS
                        && ends_with_sentence_punct(&words[end].display_text))
            };
            if break_here {
                break;
            }
            end += 1;
        }

        let first = &words[start];
        let last = &words[end];
        let covered = |ordinal: i64| {
            omit_ranges
                .iter()
                .any(|(s, e)| ordinal >= *s && ordinal <= *e)
        };
        let omitted_count = (first.ordinal..=last.ordinal)
            .filter(|ordinal| covered(*ordinal))
            .count();
        let span = (last.ordinal - first.ordinal + 1) as usize;
        segments.push(TranscriptSegment {
            index: segments.len() as i64,
            start_ordinal: first.ordinal,
            end_ordinal: last.ordinal,
            text,
            start_sample: first.start_sample,
            end_sample: last.end_sample,
            omitted: omitted_count == span,
            partially_omitted: omitted_count > 0 && omitted_count < span,
        });
        start = end + 1;
    }
    segments
}

/// 视图装配：词 + 活跃 omit + 分段一次取齐（TranscriptView 的首屏数据）。
pub fn transcript_view(
    store: &crate::storage::ProjectStore,
    asset_id: &str,
) -> crate::OperationResult<crate::contracts::TranscriptViewData> {
    let asset = match store.media_asset(asset_id) {
        Ok(Some(asset)) => asset,
        Ok(None) => {
            return crate::OperationResult::failed(
                "MEDIA_ASSET_MISSING",
                format!("资产不存在：{asset_id}"),
            );
        }
        Err(error) => {
            return crate::OperationResult::failed("STORAGE_ERROR", error.to_string());
        }
    };
    let words = match store.transcript_words(asset_id) {
        Ok(words) => words,
        Err(error) => {
            return crate::OperationResult::failed("STORAGE_ERROR", error.to_string());
        }
    };
    let omits = match store.active_omit_operations(asset_id) {
        Ok(omits) => omits,
        Err(error) => {
            return crate::OperationResult::failed("STORAGE_ERROR", error.to_string());
        }
    };
    let ranges: Vec<(i64, i64)> = omits
        .iter()
        .map(|op| (op.start_ordinal, op.end_ordinal))
        .collect();
    let segments = segment_words(&words, asset.audio_sample_rate, &ranges);
    crate::OperationResult::success(crate::contracts::TranscriptViewData {
        words,
        segments,
        omits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(ordinal: i64, text: &str, start_sample: i64, end_sample: i64) -> WordAnchor {
        WordAnchor {
            word_id: format!("w{ordinal}"),
            asset_id: "a1".to_string(),
            ordinal,
            raw_text: text.to_string(),
            display_text: text.to_string(),
            language: Some("zh".to_string()),
            start_sample,
            end_sample,
            confidence: Some(0.99),
            synthetic: false,
            source_word_ids: None,
        }
    }

    /// 48kHz 下 1 秒 = 48000 采样。
    const SR: i64 = 48_000;

    #[test]
    fn breaks_on_sentence_punct_with_pause() {
        let words = vec![
            word(0, "第一条开始。", 0, 40_000),
            // 停顿 1s（>0.8s）→ 双条件断
            word(1, "再来一条", 88_000, 120_000),
        ];
        let segments = segment_words(&words, SR, &[]);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "第一条开始。");
        assert_eq!(segments[1].start_ordinal, 1);
    }

    #[test]
    fn punct_without_pause_does_not_break() {
        let words = vec![
            word(0, "第一条开始。", 0, 40_000),
            // 停顿 0.2s（<0.8s）→ 不断
            word(1, "继续", 49_600, 60_000),
        ];
        let segments = segment_words(&words, SR, &[]);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "第一条开始。继续");
    }

    #[test]
    fn hard_pause_breaks_without_punct() {
        let words = vec![
            word(0, "导演喊停", 0, 40_000),
            // 停顿 2.5s（>2s）→ 硬断
            word(1, "重新来", 160_000, 180_000),
        ];
        let segments = segment_words(&words, SR, &[]);
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn segment_is_capped_at_120_chars() {
        let words: Vec<WordAnchor> = (0..130)
            .map(|i| word(i, "字", i * 2_400, i * 2_400 + 2_000))
            .collect();
        let segments = segment_words(&words, SR, &[]);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text.chars().count(), 120);
        assert_eq!(segments[1].text.chars().count(), 10);
    }

    #[test]
    fn ascii_words_are_joined_with_space() {
        let words = vec![
            word(0, "action", 0, 20_000),
            word(1, "scene", 21_000, 40_000),
            word(2, "开拍", 41_000, 60_000),
        ];
        let segments = segment_words(&words, SR, &[]);
        assert_eq!(segments[0].text, "action scene开拍");
    }

    #[test]
    fn omit_coverage_sets_flags() {
        let words = vec![
            word(0, "全部删掉。", 0, 40_000),
            word(1, "删掉一半", 88_000, 120_000),
            word(2, "保留这句", 130_000, 160_000),
        ];
        // 段0 全覆盖；段1（词1+2）只覆盖词1
        let segments = segment_words(&words, SR, &[(0, 1)]);
        assert_eq!(segments.len(), 2);
        assert!(segments[0].omitted);
        assert!(!segments[0].partially_omitted);
        assert!(!segments[1].omitted);
        assert!(segments[1].partially_omitted);
    }

    fn view_fixture() -> (crate::storage::ProjectStore, std::path::PathBuf) {
        // macOS 系统时钟只到微秒，并行测试可能同刻：纳秒 + pid + 原子序号三重去重。
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let sequence = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "double-love-segment-{}-{unique}-{sequence}.sqlite",
            std::process::id()
        ));
        let store = crate::storage::ProjectStore::open(&path).expect("store");
        store
            .insert_media_asset(&crate::storage::NewMediaAsset {
                id: "a1".to_string(),
                kind: "video".to_string(),
                original_path: format!("/tmp/segment-view-{unique}-{sequence}.mp4"),
                display_name: "segment.mp4".to_string(),
                duration_samples: 480_000,
                audio_sample_rate: SR,
                fps_num: 25,
                fps_den: 1,
                video_timebase: 25,
                is_ntsc: false,
                width: None,
                height: None,
                audio_channels: Some(2),
                source_tc_start_frame: None,
                ffprobe_json: "{}".to_string(),
            })
            .expect("asset");
        store
            .insert_transcript_words(&[
                crate::storage::NewTranscriptWord {
                    word_id: "w0".to_string(),
                    asset_id: "a1".to_string(),
                    ordinal: 0,
                    raw_text: "第一条开始。".to_string(),
                    display_text: "第一条开始。".to_string(),
                    language: Some("zh".to_string()),
                    start_sample: 0,
                    end_sample: 40_000,
                    confidence: Some(0.99),
                },
                crate::storage::NewTranscriptWord {
                    word_id: "w1".to_string(),
                    asset_id: "a1".to_string(),
                    ordinal: 1,
                    raw_text: "再来一条".to_string(),
                    display_text: "再来一条".to_string(),
                    language: Some("zh".to_string()),
                    start_sample: 88_000,
                    end_sample: 120_000,
                    confidence: Some(0.99),
                },
            ])
            .expect("words");
        (store, path)
    }

    #[test]
    fn transcript_view_assembles_words_segments_and_omits() {
        let (store, db) = view_fixture();
        crate::edit::omit_words(&store, "a1", 1, 1, 120, 120);
        let result = transcript_view(&store, "a1");
        assert_eq!(result.status, crate::OperationStatus::Success);
        let data = result.data.expect("view data");
        assert_eq!(data.words.len(), 2);
        assert_eq!(data.segments.len(), 2);
        assert_eq!(data.omits.len(), 1);
        assert!(!data.segments[0].omitted);
        assert!(data.segments[1].omitted);
        drop(store);
        std::fs::remove_file(db).ok();
    }

    #[test]
    fn transcript_view_missing_asset_fails() {
        let (store, db) = view_fixture();
        let result = transcript_view(&store, "nobody");
        assert_eq!(result.status, crate::OperationStatus::Failed);
        assert_eq!(result.diagnostics[0].code, "MEDIA_ASSET_MISSING");
        drop(store);
        std::fs::remove_file(db).ok();
    }
}
