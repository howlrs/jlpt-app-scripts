use std::collections::HashSet;

use log::{info, warn};

mod utils;
#[path = "dedup_common.rs"]
mod dedup_common;

use crate::utils::{
    read_questions_from_stage, write_questions_to_stage, Question, LEVELS, STAGE_2_OUTPUT,
};
use crate::dedup_common::{dedup_key, KeySkipReason, SubLike};

fn main() {
    crate::utils::init_logger();

    let start = std::time::Instant::now();

    for level in LEVELS {
        // シャッフル済みファイルを優先
        let input_file = "1_7_shuffled.json";
        // "n1"〜"n5" → 1〜5 に変換。後段 (Task 8/9) の dedup_key との整合のため、
        // stage 2 でも level_id が 0 でなく正しいレベル値で key を生成する。
        let level_id_from_loop: u32 = level
            .trim_start_matches(|c: char| c == 'n' || c == 'N')
            .parse()
            .unwrap_or(0);

        let questions = match read_questions_from_stage(level, input_file) {
            Ok(q) => q,
            Err(e) => {
                warn!("[{}] {}", level, e);
                continue;
            }
        };

        if questions.is_empty() {
            warn!("[{}] 問題データなし、スキップ", level);
            continue;
        }

        let original_count = questions.len();
        let original_sub_count: usize = questions.iter().map(|q| q.sub_questions.len()).sum();

        // dedup_key での重複検出
        let mut seen_keys: HashSet<String> = HashSet::new();
        let mut deduplicated: Vec<Question> = Vec::new();
        let mut removed_as_dup = 0usize;
        let mut excluded_numeric = 0usize;
        let mut invalid = 0usize;

        for question in questions {
            let mut kept_subs = Vec::new();
            for sub_q in question.sub_questions {
                let sub_like = SubLike {
                    options: sub_q.select_answer.iter()
                        .map(|sa| (sa.key.clone(), sa.value.clone()))
                        .collect(),
                    answer: sub_q.answer.clone(),
                };
                match dedup_key(level_id_from_loop, &sub_like) {
                    Ok(key) => {
                        if seen_keys.contains(&key) {
                            removed_as_dup += 1;
                            continue;
                        }
                        seen_keys.insert(key);
                        kept_subs.push(sub_q);
                    }
                    Err(KeySkipReason::NumericPlaceholder) => {
                        excluded_numeric += 1;
                        kept_subs.push(sub_q);
                    }
                    Err(KeySkipReason::AnswerNotInOptions) => {
                        invalid += 1;
                        kept_subs.push(sub_q);
                    }
                }
            }

            if kept_subs.is_empty() {
                continue;
            }

            deduplicated.push(Question {
                sub_questions: kept_subs,
                ..question
            });
        }

        let remaining_sub: usize = deduplicated.iter().map(|q| q.sub_questions.len()).sum();
        info!(
            "[{}] questions: {} → {}, sub_questions: {} → {} (重複除外={}, 数字選択肢スキップ={}, 不正データスキップ={})",
            level,
            original_count,
            deduplicated.len(),
            original_sub_count,
            remaining_sub,
            removed_as_dup,
            excluded_numeric,
            invalid,
        );

        match write_questions_to_stage(level, STAGE_2_OUTPUT, &deduplicated) {
            Ok(_) => info!("[{}] wrote {}", level, STAGE_2_OUTPUT),
            Err(e) => warn!("[{}] 書込失敗: {}", level, e),
        }
    }

    info!("done, elapsed: {:?}", start.elapsed());
}
