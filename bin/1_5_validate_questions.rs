use std::collections::HashSet;

use log::{info, warn};
use serde::{Deserialize, Serialize};

mod utils;
use crate::utils::{
    read_questions_from_stage, write_file, write_questions_to_stage, level_dir,
    Question, LEVELS, STAGE_1_OUTPUT, STAGE_1_5_VALIDATED, STAGE_1_5_REJECTED,
};

/// Question構造体にリジェクト理由を付与した構造体
#[derive(Serialize, Deserialize, Debug)]
struct RejectedQuestion {
    question: Question,
    reasons: Vec<String>,
}

/// Read parsed questions from stage 1, validate each Question,
/// write validated to STAGE_1_5_VALIDATED and rejected to STAGE_1_5_REJECTED.
fn main() {
    crate::utils::init_logger();

    let start = std::time::Instant::now();

    for level in LEVELS {
        let questions = match read_questions_from_stage(level, STAGE_1_OUTPUT) {
            Ok(q) => q,
            Err(e) => {
                warn!("level {}: {}", level, e);
                continue;
            }
        };

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

        // Write validated questions
        match write_questions_to_stage(level, STAGE_1_5_VALIDATED, &validated) {
            Ok(_) => info!("level {}: wrote {}", level, STAGE_1_5_VALIDATED),
            Err(e) => warn!("level {}: failed to write validated: {}", level, e),
        }

        // Write rejected questions (custom struct, use write_file directly)
        let rejected_json = serde_json::to_string_pretty(&rejected).unwrap();
        let rejected_path = level_dir(level).join(STAGE_1_5_REJECTED);
        write_file(rejected_path, &rejected_json);
    }

    info!("done, elapsed: {:?}", start.elapsed());
}

/// Questionに対するバリデーションを実行し、失敗理由のリストを返す
/// 空のリストが返れば全バリデーション通過
fn validate_question(question: &Question) -> Vec<String> {
    let mut reasons = Vec::new();

    // required fields: sentence must not be empty
    if question.sentence.trim().is_empty() {
        reasons.push("question sentence is empty".to_string());
    }

    // required fields: category_name must not be empty
    if question.category_name.trim().is_empty() {
        reasons.push("category_name is empty".to_string());
    }

    for (i, sub_q) in question.sub_questions.iter().enumerate() {
        let sub_label = format!("sub_question[{}]", i);

        // select_answer count: each SubQuestion must have exactly 4 options
        if sub_q.select_answer.len() != 4 {
            reasons.push(format!(
                "{}: select_answer count is {} (expected 4)",
                sub_label,
                sub_q.select_answer.len()
            ));
        }

        // answer range: answer must be "1", "2", "3", or "4"
        let valid_answers = ["1", "2", "3", "4"];
        if !valid_answers.contains(&sub_q.answer.as_str()) {
            reasons.push(format!(
                "{}: answer '{}' is not in valid range 1-4",
                sub_label, sub_q.answer
            ));
        }

        // answer-option consistency: answer field value must correspond to an existing key
        let answer_key_exists = sub_q
            .select_answer
            .iter()
            .any(|sa| sa.key == sub_q.answer);
        if !answer_key_exists {
            reasons.push(format!(
                "{}: answer '{}' does not match any key in select_answer",
                sub_label, sub_q.answer
            ));
        }

        // non-empty options: all select_answer values must be non-empty strings
        for (j, sa) in sub_q.select_answer.iter().enumerate() {
            if sa.value.trim().is_empty() {
                reasons.push(format!(
                    "{}: select_answer[{}] key '{}' has empty value",
                    sub_label, j, sa.key
                ));
            }
        }

        // no duplicate options: select_answer values must all be unique within a SubQuestion
        let mut seen_values = HashSet::new();
        for sa in &sub_q.select_answer {
            if !seen_values.insert(sa.value.trim().to_string()) {
                reasons.push(format!(
                    "{}: duplicate select_answer value '{}'",
                    sub_label, sa.value
                ));
            }
        }
    }

    reasons
}
