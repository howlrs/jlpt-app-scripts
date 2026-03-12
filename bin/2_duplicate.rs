use std::collections::HashSet;

use log::{info, warn};

mod utils;
use crate::utils::{
    read_questions_from_stage, write_questions_to_stage,
    Question, LEVELS, STAGE_1_5_VALIDATED, STAGE_2_OUTPUT,
};

/// Read validated questions, deduplicate at the SubQuestion level.
///
/// CRITICAL FIX: The old logic marked an entire Question as duplicate if ANY
/// SubQuestion sentence had been seen before. The new logic instead:
///   1. Tracks seen SubQuestion sentences in a HashSet
///   2. For each Question, filters out only the duplicate SubQuestions
///   3. If a Question has 0 remaining SubQuestions after filtering, discards it
///   4. Keeps Questions that still have valid SubQuestions
fn main() {
    crate::utils::init_logger();

    let start = std::time::Instant::now();

    for level in LEVELS {
        let questions = match read_questions_from_stage(level, STAGE_1_5_VALIDATED) {
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

        let original_count = questions.len();
        let original_sub_count: usize = questions.iter().map(|q| q.sub_questions.len()).sum();

        // Track seen SubQuestion sentences
        let mut seen_sentences: HashSet<String> = HashSet::new();
        let mut deduplicated: Vec<Question> = Vec::new();
        let mut removed_sub_count: usize = 0;

        for question in questions {
            // Filter SubQuestions: keep only those with unseen sentences
            let mut kept_subs = Vec::new();
            for sub_q in question.sub_questions {
                let key = sub_q
                    .sentence
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                if key.is_empty() || seen_sentences.insert(key) {
                    // Empty sentence or first time seeing this sentence -> keep it
                    kept_subs.push(sub_q);
                } else {
                    removed_sub_count += 1;
                }
            }

            // Discard the Question entirely if no SubQuestions remain
            if kept_subs.is_empty() {
                continue;
            }

            deduplicated.push(Question {
                sub_questions: kept_subs,
                ..question
            });
        }

        info!(
            "level {}: questions {} -> {}, sub_questions {} -> {} (removed {} duplicate subs)",
            level,
            original_count,
            deduplicated.len(),
            original_sub_count,
            original_sub_count - removed_sub_count,
            removed_sub_count,
        );

        match write_questions_to_stage(level, STAGE_2_OUTPUT, &deduplicated) {
            Ok(_) => info!("level {}: wrote {}", level, STAGE_2_OUTPUT),
            Err(e) => warn!("level {}: failed to write: {}", level, e),
        }
    }

    info!("done, elapsed: {:?}", start.elapsed());
}
