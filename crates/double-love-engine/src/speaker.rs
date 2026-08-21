//! 说话人投影与本地名称候选。模型后端只负责产生 SpeakerSegment/embedding；身份、词归属
//! 和任何自动候选都由这里以可审阅数据写回项目。

use std::collections::HashMap;

use crate::contracts::{
    SpeakerAssignment, SpeakerMergeProposal, SpeakerNameAgentPayload, SpeakerNameProposal,
    SpeakerSegment, WordAnchor,
};

/// 按最大声学重叠把每个词归属到一个说话人。交叠无法可靠判定时返回空数组，让 UI 显示
/// “待确认”而不是伪造一个名字。
pub fn assign_words_to_speakers(
    words: &[WordAnchor],
    segments: &[SpeakerSegment],
) -> Vec<(String, Vec<SpeakerAssignment>)> {
    words
        .iter()
        .map(|word| {
            let word_duration = (word.end_sample - word.start_sample).max(1) as f64;
            let mut overlaps: Vec<(&SpeakerSegment, i64)> = segments
                .iter()
                .filter_map(|segment| {
                    let start = word.start_sample.max(segment.start_sample);
                    let end = word.end_sample.min(segment.end_sample);
                    (end > start).then_some((segment, end - start))
                })
                .collect();
            overlaps.sort_by_key(|(_, overlap)| std::cmp::Reverse(*overlap));
            let assignment = match overlaps.as_slice() {
                [(segment, overlap), ..] => {
                    let tied = overlaps
                        .get(1)
                        .is_some_and(|(_, runner_up)| *runner_up == *overlap);
                    (!tied).then(|| SpeakerAssignment {
                        speaker_id: segment.speaker_id.clone(),
                        confidence: Some(
                            (*overlap as f64 / word_duration) * segment.confidence.unwrap_or(1.0),
                        ),
                        evidence: "diarization_overlap".to_string(),
                    })
                }
                [] => None,
            };
            (word.word_id.clone(), assignment.into_iter().collect())
        })
        .collect()
}

fn clean_name(candidate: &str) -> Option<String> {
    let candidate = candidate
        .trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, '：' | ':' | '，' | ',')
        })
        .split(|character: char| {
            matches!(
                character,
                '，' | ',' | '。' | '.' | '！' | '!' | '？' | '?' | '\n'
            )
        })
        .next()
        .unwrap_or_default()
        .trim();
    let length = candidate.chars().count();
    (2..=24)
        .contains(&length)
        .then(|| candidate.to_string())
        .filter(|value| !matches!(value.as_str(), "一个" | "主持人" | "嘉宾" | "自己"))
}

/// 不联网的名称候选：仅识别明确的“我是 / 我叫 / my name is”自我介绍，并保留原句。
pub fn local_name_proposals(words: &[WordAnchor]) -> Vec<SpeakerNameProposal> {
    let mut by_speaker: HashMap<&str, Vec<&WordAnchor>> = HashMap::new();
    for word in words {
        if let Some(assignment) = word.speaker_assignments.first() {
            by_speaker
                .entry(&assignment.speaker_id)
                .or_default()
                .push(word);
        }
    }
    let mut proposals = Vec::new();
    for (speaker_id, speaker_words) in by_speaker {
        let text = speaker_words
            .iter()
            .map(|word| word.display_text.as_str())
            .collect::<Vec<_>>()
            .join("");
        let candidate = ["我叫", "我是"]
            .iter()
            .find_map(|prefix| {
                text.find(prefix)
                    .and_then(|index| clean_name(&text[index + prefix.len()..]))
            })
            .or_else(|| {
                let lower = text.to_ascii_lowercase();
                lower
                    .find("my name is")
                    .and_then(|index| clean_name(&text[index + "my name is".len()..]))
            });
        if let Some(candidate_name) = candidate {
            let quote = text.chars().take(80).collect::<String>();
            proposals.push(SpeakerNameProposal {
                speaker_id: speaker_id.to_string(),
                candidate_name,
                quote,
                confidence: 0.72,
                source: "local_self_introduction".to_string(),
                reason: "命中明确的“我是 / 我叫 / my name is”自我介绍。".to_string(),
            });
        }
    }
    proposals.sort_by(|left, right| left.speaker_id.cmp(&right.speaker_id));
    proposals
}

/// 生成用户主动调用 Agent 前可审阅的最小 payload。只收集该匿名说话人的少量发言，
/// 并严格限定总字符数；调用方不得向 payload 中附加音频、路径或声纹向量。
pub fn agent_name_payload_preview(
    words: &[WordAnchor],
    speaker_id: &str,
) -> SpeakerNameAgentPayload {
    const MAX_UTTERANCES: usize = 6;
    const MAX_CHARS: usize = 480;
    let mut utterances = Vec::new();
    let mut current = String::new();
    let mut total_chars = 0_usize;
    for word in words.iter().filter(|word| {
        word.speaker_assignments
            .first()
            .is_some_and(|assignment| assignment.speaker_id == speaker_id)
    }) {
        let next_len = word.display_text.chars().count();
        if total_chars + current.chars().count() + next_len > MAX_CHARS {
            break;
        }
        if text_needs_break(&current, &word.display_text) && !current.is_empty() {
            total_chars += current.chars().count();
            utterances.push(std::mem::take(&mut current));
            if utterances.len() >= MAX_UTTERANCES {
                break;
            }
        }
        current.push_str(&word.display_text);
    }
    if !current.is_empty() && utterances.len() < MAX_UTTERANCES {
        utterances.push(current);
    }
    SpeakerNameAgentPayload {
        speaker_id: speaker_id.to_string(),
        utterances,
        instruction: "仅根据这些匿名说话人的发言，判断是否存在明确自我介绍；如有，返回候选姓名、引用句和置信理由；如无，返回无候选。不得推测身份。"
            .to_string(),
    }
}

fn text_needs_break(current: &str, next: &str) -> bool {
    current
        .chars()
        .last()
        .is_some_and(|character| matches!(character, '。' | '！' | '？' | '.' | '!' | '?' | '\n'))
        || next.chars().count() > 100
}

/// 根据本地声纹向量给出跨素材合并候选。向量仅在调用过程中使用，不序列化到日志或导出物。
pub fn merge_proposals_from_embeddings(
    embeddings: &[(String, Vec<f32>)],
    minimum_similarity: f64,
) -> Vec<SpeakerMergeProposal> {
    let mut proposals = Vec::new();
    for (index, (left_id, left)) in embeddings.iter().enumerate() {
        for (right_id, right) in embeddings.iter().skip(index + 1) {
            if left.len() != right.len() || left.is_empty() {
                continue;
            }
            let (dot, left_norm, right_norm) = left.iter().zip(right).fold(
                (0.0_f64, 0.0_f64, 0.0_f64),
                |(dot, left_norm, right_norm), (left, right)| {
                    let left = *left as f64;
                    let right = *right as f64;
                    (
                        dot + left * right,
                        left_norm + left * left,
                        right_norm + right * right,
                    )
                },
            );
            let similarity = dot / (left_norm.sqrt() * right_norm.sqrt()).max(f64::EPSILON);
            if similarity >= minimum_similarity {
                proposals.push(SpeakerMergeProposal {
                    id: format!("merge:{left_id}:{right_id}"),
                    left_speaker_id: left_id.clone(),
                    right_speaker_id: right_id.clone(),
                    similarity,
                    evidence: "local_embedding_cosine_similarity".to_string(),
                    status: "pending".to_string(),
                });
            }
        }
    }
    proposals.sort_by(|left, right| right.similarity.total_cmp(&left.similarity));
    proposals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(id: &str, text: &str, start: i64, end: i64) -> WordAnchor {
        WordAnchor {
            word_id: id.to_string(),
            asset_id: "a".to_string(),
            ordinal: 0,
            raw_text: text.to_string(),
            display_text: text.to_string(),
            language: Some("zh".to_string()),
            start_sample: start,
            end_sample: end,
            confidence: Some(0.9),
            synthetic: false,
            source_word_ids: None,
            speaker_assignments: Vec::new(),
        }
    }

    #[test]
    fn assigns_words_by_largest_non_tied_overlap() {
        let words = vec![word("w1", "你好", 0, 100), word("w2", "世界", 100, 200)];
        let segments = vec![
            SpeakerSegment {
                id: "seg1".to_string(),
                asset_id: "a".to_string(),
                speaker_id: "s1".to_string(),
                start_sample: 0,
                end_sample: 120,
                confidence: Some(0.9),
            },
            SpeakerSegment {
                id: "seg2".to_string(),
                asset_id: "a".to_string(),
                speaker_id: "s2".to_string(),
                start_sample: 120,
                end_sample: 200,
                confidence: Some(0.8),
            },
        ];
        let assignments = assign_words_to_speakers(&words, &segments);
        assert_eq!(assignments[0].1[0].speaker_id, "s1");
        assert_eq!(assignments[1].1[0].speaker_id, "s2");
    }

    #[test]
    fn local_self_introduction_stays_a_proposal() {
        let mut words = vec![word("w1", "大家好，我叫李明。", 0, 100)];
        words[0].speaker_assignments = vec![SpeakerAssignment {
            speaker_id: "s1".to_string(),
            confidence: Some(1.0),
            evidence: "test".to_string(),
        }];
        let proposal = local_name_proposals(&words).pop().expect("proposal");
        assert_eq!(proposal.speaker_id, "s1");
        assert_eq!(proposal.candidate_name, "李明");
        assert_eq!(proposal.source, "local_self_introduction");
        assert!(!proposal.reason.is_empty());
    }

    #[test]
    fn agent_payload_contains_only_the_requested_anonymous_speaker() {
        let mut words = vec![
            word("w1", "我是李明。", 0, 100),
            word("w2", "不要发送", 100, 200),
        ];
        words[0].speaker_assignments = vec![SpeakerAssignment {
            speaker_id: "s1".to_string(),
            confidence: Some(1.0),
            evidence: "test".to_string(),
        }];
        words[1].speaker_assignments = vec![SpeakerAssignment {
            speaker_id: "s2".to_string(),
            confidence: Some(1.0),
            evidence: "test".to_string(),
        }];
        let payload = agent_name_payload_preview(&words, "s1");
        assert_eq!(payload.speaker_id, "s1");
        assert_eq!(payload.utterances, vec!["我是李明。"]);
        assert!(!payload.instruction.contains("路径"));
    }

    #[test]
    fn embeddings_only_propose_when_similar_enough() {
        let proposals = merge_proposals_from_embeddings(
            &[
                ("s1".to_string(), vec![1.0, 0.0]),
                ("s2".to_string(), vec![0.99, 0.01]),
                ("s3".to_string(), vec![0.0, 1.0]),
            ],
            0.95,
        );
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].left_speaker_id, "s1");
        assert_eq!(proposals[0].right_speaker_id, "s2");
    }
}
