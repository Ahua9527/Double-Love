//! 删改操作：omit / restore。
//! 删除文字 ≠ 删除底层词——omit 只是活跃标记，restore 可完全或部分回填
//! （部分回填时原 omit 拆为两段，全部写入在同一事务内）。

use uuid::Uuid;

use crate::contracts::EditOperation;
use crate::storage::{ProjectStore, StorageError};
use crate::{DiagnosticLevel, OperationResult};

/// 默认切点缓冲（毫秒）；逐词时间戳误差 ±100–300ms 的内建对策。
pub const DEFAULT_HANDLES_MS: i64 = 120;

fn edit_failure<T>(code: &str, cause: String, action: Option<&str>) -> OperationResult<T> {
    let mut result = OperationResult::failed(code, cause);
    result.diagnostics[0].level = DiagnosticLevel::Error;
    result.diagnostics[0].suggested_action = action.map(str::to_string);
    result
}

fn storage_failure<T>(error: StorageError) -> OperationResult<T> {
    OperationResult::failed("STORAGE_ERROR", error.to_string())
}

/// 标记删除词序闭区间 [start_ordinal, end_ordinal]（连带音视频，ripple）。
pub fn omit_words(
    store: &ProjectStore,
    asset_id: &str,
    start_ordinal: i64,
    end_ordinal: i64,
    handles_before_ms: i64,
    handles_after_ms: i64,
) -> OperationResult<EditOperation> {
    if start_ordinal > end_ordinal || start_ordinal < 0 {
        return edit_failure(
            "EDIT_RANGE_INVALID",
            format!("词序区间非法：[{start_ordinal}, {end_ordinal}]"),
            Some("先选择要删除的文字再删除。"),
        );
    }
    if handles_before_ms < 0 || handles_after_ms < 0 {
        return edit_failure(
            "EDIT_HANDLES_INVALID",
            "handles 不能为负。".to_string(),
            None,
        );
    }
    let word_count = match store.count_transcript_words(asset_id) {
        Ok(count) => count as i64,
        Err(error) => return storage_failure(error),
    };
    if word_count == 0 {
        return edit_failure(
            "EDIT_NO_TRANSCRIPT",
            "该资产还没有转录词。".to_string(),
            Some("先运行转录。"),
        );
    }
    if end_ordinal >= word_count {
        return edit_failure(
            "EDIT_RANGE_INVALID",
            format!("词序越界：end={end_ordinal}，但共有 {word_count} 词"),
            None,
        );
    }

    let op_id = Uuid::new_v4().to_string();
    match store.apply_omit(
        &op_id,
        asset_id,
        start_ordinal,
        end_ordinal,
        handles_before_ms,
        handles_after_ms,
    ) {
        Ok(op) => {
            let mut result = OperationResult::success(op);
            result.revision = store.revision().ok();
            result
        }
        Err(error) => storage_failure(error),
    }
}

/// 恢复一条活跃 omit 的全部或部分词。
/// 完全覆盖 → 原 omit 被 supersede；部分覆盖 → 原 omit 拆为仍活跃的若干段。
pub fn restore_words(
    store: &ProjectStore,
    operation_id: &str,
    start_ordinal: i64,
    end_ordinal: i64,
) -> OperationResult<EditOperation> {
    let original = match store.edit_operation(operation_id) {
        Ok(Some(op)) => op,
        Ok(None) => {
            return edit_failure(
                "EDIT_OP_NOT_FOUND",
                format!("编辑操作不存在：{operation_id}"),
                None,
            );
        }
        Err(error) => return storage_failure(error),
    };
    if original.superseded_by.is_some() {
        return edit_failure(
            "EDIT_OP_SUPERSEDED",
            "该删除已被更新的操作覆盖，不能再恢复。".to_string(),
            Some("刷新文本视图后重试。"),
        );
    }
    if original.edit_type != crate::contracts::EditType::Omit {
        return edit_failure(
            "EDIT_OP_NOT_OMIT",
            "只能对删除操作执行恢复。".to_string(),
            None,
        );
    }
    if start_ordinal < original.start_ordinal || end_ordinal > original.end_ordinal {
        return edit_failure(
            "EDIT_RANGE_INVALID",
            format!(
                "恢复区间 [{start_ordinal}, {end_ordinal}] 超出原删除区间 [{}, {}]",
                original.start_ordinal, original.end_ordinal
            ),
            Some("在原删除（划线）范围内选择要恢复的文字。"),
        );
    }
    if start_ordinal > end_ordinal {
        return edit_failure(
            "EDIT_RANGE_INVALID",
            format!("词序区间非法：[{start_ordinal}, {end_ordinal}]"),
            None,
        );
    }

    // 拆分：原区间去掉恢复区间后剩余的活跃段（0–2 段）
    let mut pieces = Vec::new();
    if start_ordinal > original.start_ordinal {
        pieces.push((original.start_ordinal, start_ordinal - 1));
    }
    if end_ordinal < original.end_ordinal {
        pieces.push((end_ordinal + 1, original.end_ordinal));
    }
    let piece_ids: Vec<String> = pieces.iter().map(|_| Uuid::new_v4().to_string()).collect();
    let restore_id = Uuid::new_v4().to_string();

    match store.apply_restore(
        &restore_id,
        &original,
        start_ordinal,
        end_ordinal,
        &pieces,
        &piece_ids,
    ) {
        Ok(op) => {
            let mut result = OperationResult::success(op);
            result.revision = store.revision().ok();
            result
        }
        Err(error) => storage_failure(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::EditType;
    use crate::storage::{NewMediaAsset, NewTranscriptWord};

    fn store_with_words(word_count: i64) -> (ProjectStore, std::path::PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("double-love-edit-{unique}.sqlite"));
        let store = ProjectStore::open(&path).expect("store");
        store
            .insert_media_asset(&NewMediaAsset {
                id: "a1".to_string(),
                kind: "video".to_string(),
                original_path: format!("/tmp/synthetic-{unique}.mp4"),
                display_name: "synthetic".to_string(),
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
                ffprobe_json: "{}".to_string(),
            })
            .expect("asset");
        let words: Vec<NewTranscriptWord> = (0..word_count)
            .map(|i| NewTranscriptWord {
                word_id: format!("w{i}"),
                asset_id: "a1".to_string(),
                ordinal: i,
                raw_text: format!("词{i}"),
                display_text: format!("词{i}"),
                language: Some("zh".to_string()),
                start_sample: i * 4800,
                end_sample: i * 4800 + 2400,
                confidence: Some(0.99),
            })
            .collect();
        store.insert_transcript_words(&words).expect("words");
        (store, path)
    }

    #[test]
    fn omit_marks_active_operation() {
        let (store, path) = store_with_words(10);
        let result = omit_words(&store, "a1", 2, 4, 120, 120);
        assert_eq!(result.status, crate::OperationStatus::Success);
        let op = result.data.expect("op");
        assert_eq!(op.edit_type, EditType::Omit);
        assert_eq!((op.start_ordinal, op.end_ordinal), (2, 4));
        assert!(result.revision.expect("revision") >= 1);

        let active = store.active_omit_operations("a1").expect("active");
        assert_eq!(active.len(), 1);
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn omit_validates_range_and_transcript() {
        let (store, path) = store_with_words(10);
        assert_eq!(
            omit_words(&store, "a1", 5, 3, 120, 120).diagnostics[0].code,
            "EDIT_RANGE_INVALID"
        );
        assert_eq!(
            omit_words(&store, "a1", 0, 10, 120, 120).diagnostics[0].code,
            "EDIT_RANGE_INVALID"
        );
        assert_eq!(
            omit_words(&store, "empty-asset", 0, 1, 120, 120).diagnostics[0].code,
            "EDIT_NO_TRANSCRIPT"
        );
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn restore_full_coverage_supersedes_original() {
        let (store, path) = store_with_words(10);
        let omit = omit_words(&store, "a1", 2, 6, 120, 120).data.expect("omit");
        let restore = restore_words(&store, &omit.id, 2, 6);
        assert_eq!(restore.status, crate::OperationStatus::Success);
        assert_eq!(restore.data.expect("restore").edit_type, EditType::Restore);

        let active = store.active_omit_operations("a1").expect("active");
        assert!(active.is_empty(), "全量恢复后无活跃 omit");
        let superseded = store
            .edit_operation(&omit.id)
            .expect("read")
            .expect("exists");
        assert!(superseded.superseded_by.is_some());
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn restore_middle_splits_into_two_active_pieces() {
        let (store, path) = store_with_words(10);
        let omit = omit_words(&store, "a1", 2, 8, 100, 140).data.expect("omit");
        let restore = restore_words(&store, &omit.id, 4, 5);
        assert_eq!(restore.status, crate::OperationStatus::Success);

        let active = store.active_omit_operations("a1").expect("active");
        let ranges: Vec<(i64, i64)> = active
            .iter()
            .map(|op| (op.start_ordinal, op.end_ordinal))
            .collect();
        assert_eq!(ranges, vec![(2, 3), (6, 8)]);
        // 拆分段继承原 handles
        assert!(
            active
                .iter()
                .all(|op| op.handles_before_ms == 100 && op.handles_after_ms == 140)
        );
        // 恢复原区间不能再恢复
        assert_eq!(
            restore_words(&store, &omit.id, 2, 3).diagnostics[0].code,
            "EDIT_OP_SUPERSEDED"
        );
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn restore_edge_leaves_one_piece() {
        let (store, path) = store_with_words(10);
        let omit = omit_words(&store, "a1", 2, 8, 120, 120).data.expect("omit");
        restore_words(&store, &omit.id, 2, 4);
        let active = store.active_omit_operations("a1").expect("active");
        assert_eq!(active.len(), 1);
        assert_eq!((active[0].start_ordinal, active[0].end_ordinal), (5, 8));
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn restore_rejects_out_of_range_and_writes_nothing() {
        let (store, path) = store_with_words(10);
        let omit = omit_words(&store, "a1", 2, 8, 120, 120).data.expect("omit");
        let revision_before = store.revision().expect("revision");
        let result = restore_words(&store, &omit.id, 0, 9);
        assert_eq!(result.diagnostics[0].code, "EDIT_RANGE_INVALID");
        // 校验失败不得写入：revision 不变、原 omit 仍活跃且未被 supersede
        assert_eq!(store.revision().expect("revision"), revision_before);
        let still = store
            .edit_operation(&omit.id)
            .expect("read")
            .expect("exists");
        assert!(still.superseded_by.is_none());
        assert_eq!(store.active_omit_operations("a1").expect("active").len(), 1);
        drop(store);
        std::fs::remove_file(path).ok();
    }
}
