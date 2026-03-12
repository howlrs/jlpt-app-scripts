use log::{info, warn};

mod utils;
use crate::utils::{
    read_questions_from_stage, write_questions_to_stage,
    LEVELS, STAGE_2_OUTPUT, STAGE_3_OUTPUT,
};

/// Assign unique IDs to each Question and sequential IDs to their SubQuestions.
fn main() {
    crate::utils::init_logger();

    for level in LEVELS {
        let mut questions = match read_questions_from_stage(level, STAGE_2_OUTPUT) {
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

        // Assign IDs using the existing numbering() method
        for question in questions.iter_mut() {
            question.numbering();
        }

        // Log tail for quick verification
        let tail_count = questions.len().min(2);
        info!(
            "level {}: total {}, tail: {:?}",
            level,
            questions.len(),
            &questions[questions.len() - tail_count..]
        );

        match write_questions_to_stage(level, STAGE_3_OUTPUT, &questions) {
            Ok(_) => info!("level {}: wrote {}", level, STAGE_3_OUTPUT),
            Err(e) => warn!("level {}: failed to write: {}", level, e),
        }
    }

    info!("done");
}
