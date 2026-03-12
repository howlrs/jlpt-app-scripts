use std::{collections::HashSet, env};

use log::{error, info, warn};
use serde::{Deserialize, Serialize};

mod utils;
use crate::utils::{Question, read_file, write_file};

/// Question構造体にリジェクト理由を付与した構造体
#[derive(Serialize, Deserialize, Debug)]
struct RejectedQuestion {
    question: Question,
    reasons: Vec<String>,
}

/// concat_with_struct.json を読み込み、各Questionをバリデーションする
/// バリデーションに成功した場合は 1_5_validated.json に保存する
/// バリデーションに失敗した場合は 1_5_rejected.json に保存する
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let start = std::time::Instant::now();

    let output_dir = "output";
    let target_dir = "questions";
    let target_levels = ["n1", "n2", "n3", "n4", "n5"];
    let target_file = "concat_with_struct.json";

    let validated_file = "1_5_validated.json";
    let rejected_file = "1_5_rejected.json";

    for level in target_levels {
        let questions = read_questions(output_dir, target_dir, target_file, level);
        if questions.is_empty() {
            warn!("level {}: no questions found, skipping", level);
            continue;
        }

        let total = questions.len();
        let mut validated: Vec<Question> = Vec::new();
        let mut rejected: Vec<RejectedQuestion> = Vec::new();
        let mut reason_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for question in questions {
            let reasons = validate_question(&question);
            if reasons.is_empty() {
                validated.push(question);
            } else {
                for reason in &reasons {
                    *reason_counts.entry(reason.clone()).or_insert(0) += 1;
                }
                rejected.push(RejectedQuestion { question, reasons });
            }
        }

        // Summary log
        info!("=== level {} validation summary ===", level);
        info!(
            "total: {}, passed: {}, rejected: {}",
            total,
            validated.len(),
            rejected.len()
        );
        if !reason_counts.is_empty() {
            info!("rejection reasons breakdown:");
            let mut sorted_reasons: Vec<_> = reason_counts.iter().collect();
            sorted_reasons.sort_by(|a, b| b.1.cmp(a.1));
            for (reason, count) in sorted_reasons {
                info!("  {}: {}", reason, count);
            }
        }

        // Write output files
        let target_level_dir = {
            let current_dir = env::current_dir().unwrap();
            current_dir.join(output_dir).join(target_dir).join(level)
        };

        let validated_json = serde_json::to_string_pretty(&validated).unwrap();
        write_file(target_level_dir.join(validated_file), &validated_json);

        let rejected_json = serde_json::to_string_pretty(&rejected).unwrap();
        write_file(target_level_dir.join(rejected_file), &rejected_json);
    }

    info!("done, elapsed: {:?}", start.elapsed());
}

/// Questionに対するバリデーションを実行し、失敗理由のリストを返す
/// 空のリストが返れば全バリデーション通過
fn validate_question(question: &Question) -> Vec<String> {
    let mut reasons = Vec::new();

    // c. required fields: sentence must not be empty
    if question.sentence.trim().is_empty() {
        reasons.push("question sentence is empty".to_string());
    }

    // c. required fields: category_name must not be empty
    if question.category_name.trim().is_empty() {
        reasons.push("category_name is empty".to_string());
    }

    for (i, sub_q) in question.sub_questions.iter().enumerate() {
        let sub_label = format!("sub_question[{}]", i);

        // a. select_answer count: each SubQuestion must have exactly 4 options
        if sub_q.select_answer.len() != 4 {
            reasons.push(format!(
                "{}: select_answer count is {} (expected 4)",
                sub_label,
                sub_q.select_answer.len()
            ));
        }

        // d. answer range: answer must be "1", "2", "3", or "4"
        let valid_answers = ["1", "2", "3", "4"];
        if !valid_answers.contains(&sub_q.answer.as_str()) {
            reasons.push(format!(
                "{}: answer '{}' is not in valid range 1-4",
                sub_label, sub_q.answer
            ));
        }

        // b. answer-option consistency: answer field value must correspond to an existing key
        let answer_key_exists = sub_q
            .select_answer
            .iter()
            .any(|sa| sa.contains_key(&sub_q.answer));
        if !answer_key_exists {
            reasons.push(format!(
                "{}: answer '{}' does not match any key in select_answer",
                sub_label, sub_q.answer
            ));
        }

        // f. non-empty options: all select_answer values must be non-empty strings
        for (j, sa) in sub_q.select_answer.iter().enumerate() {
            for (key, value) in sa {
                if value.trim().is_empty() {
                    reasons.push(format!(
                        "{}: select_answer[{}] key '{}' has empty value",
                        sub_label, j, key
                    ));
                }
            }
        }

        // e. no duplicate options: select_answer values must all be unique within a SubQuestion
        let mut seen_values = HashSet::new();
        for sa in &sub_q.select_answer {
            for value in sa.values() {
                if !seen_values.insert(value.trim().to_string()) {
                    reasons.push(format!(
                        "{}: duplicate select_answer value '{}'",
                        sub_label, value
                    ));
                }
            }
        }
    }

    reasons
}

fn read_questions(
    output_dir: &str,
    target_dir: &str,
    target_file: &str,
    level: &str,
) -> Vec<Question> {
    let target_filepath = {
        let current_dir = env::current_dir().unwrap();
        current_dir
            .join(output_dir)
            .join(target_dir)
            .join(level)
            .join(target_file)
    };
    if !target_filepath.exists() {
        error!("ファイルデータが存在しません: {:?}", target_filepath);
        return vec![];
    }
    let content = read_file(target_filepath);
    match serde_json::from_str::<Vec<Question>>(&content) {
        Ok(questions) => questions,
        Err(e) => {
            error!("JSONデータのパースに失敗しました: {:?}", e);
            vec![]
        }
    }
}
