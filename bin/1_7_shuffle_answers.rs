/// Stage 1.7: 選択肢の順序シャッフル
///
/// AIが正解を「1」に偏らせる傾向を修正する。
/// 各SubQuestionの選択肢順序をランダムに並べ替え、
/// answerフィールドも新しいキーに更新する。
use log::{info, warn};
use rand::seq::SliceRandom;

mod utils;
use crate::utils::{
    read_questions_from_stage, write_questions_to_stage, SelectAnswer, LEVELS,
    STAGE_1_5_VALIDATED,
};

const STAGE_OUTPUT: &str = "1_7_shuffled.json";

fn main() {
    utils::init_logger();

    let start = std::time::Instant::now();

    for level in LEVELS {
        let mut questions = match read_questions_from_stage(level, STAGE_1_5_VALIDATED) {
            Ok(q) => q,
            Err(e) => {
                warn!("[{}] {}", level, e);
                continue;
            }
        };

        if questions.is_empty() {
            warn!("[{}] 問題データなし", level);
            continue;
        }

        let mut rng = rand::rng();
        let mut answer_dist = [0u32; 4]; // 1,2,3,4の分布

        for question in questions.iter_mut() {
            for sub_q in question.sub_questions.iter_mut() {
                if sub_q.select_answer.len() != 4 {
                    continue;
                }

                // 現在の正解のvalueを取得
                let correct_value = sub_q
                    .select_answer
                    .iter()
                    .find(|sa| sa.key == sub_q.answer)
                    .map(|sa| sa.value.clone());

                if let Some(correct_val) = correct_value {
                    // 値だけを抽出してシャッフル
                    let mut values: Vec<String> =
                        sub_q.select_answer.iter().map(|sa| sa.value.clone()).collect();
                    values.shuffle(&mut rng);

                    // 新しい選択肢を構築
                    let new_answers: Vec<SelectAnswer> = values
                        .iter()
                        .enumerate()
                        .map(|(i, v)| SelectAnswer {
                            key: (i + 1).to_string(),
                            value: v.clone(),
                        })
                        .collect();

                    // 正解の新しいキーを特定
                    let new_answer = new_answers
                        .iter()
                        .find(|sa| sa.value == correct_val)
                        .map(|sa| sa.key.clone())
                        .unwrap_or(sub_q.answer.clone());

                    // 分布カウント
                    if let Ok(idx) = new_answer.parse::<usize>() {
                        if idx >= 1 && idx <= 4 {
                            answer_dist[idx - 1] += 1;
                        }
                    }

                    sub_q.select_answer = new_answers;
                    sub_q.answer = new_answer;
                }
            }
        }

        let total: u32 = answer_dist.iter().sum();
        info!(
            "[{}] シャッフル完了 - 正解分布: 1={}({:.0}%) 2={}({:.0}%) 3={}({:.0}%) 4={}({:.0}%)",
            level,
            answer_dist[0], answer_dist[0] as f64 / total as f64 * 100.0,
            answer_dist[1], answer_dist[1] as f64 / total as f64 * 100.0,
            answer_dist[2], answer_dist[2] as f64 / total as f64 * 100.0,
            answer_dist[3], answer_dist[3] as f64 / total as f64 * 100.0,
        );

        match write_questions_to_stage(level, STAGE_OUTPUT, &questions) {
            Ok(_) => info!("[{}] wrote {}", level, STAGE_OUTPUT),
            Err(e) => warn!("[{}] 書込失敗: {}", level, e),
        }
    }

    info!("done, elapsed: {:?}", start.elapsed());
}
