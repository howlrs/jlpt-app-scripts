use log::{error, info, warn};

mod utils;
use crate::utils::{
    read_questions_from_stage, write_questions_to_stage,
    LEVELS, STAGE_3_OUTPUT, STAGE_4_OUTPUT,
};

/// Parse level_name into a numeric level_id for each Question.
fn main() {
    crate::utils::init_logger();

    for level in LEVELS {
        // FIX: Use read_questions_from_stage with proper error handling
        // instead of raw serde_json::from_str().unwrap()
        let mut questions = match read_questions_from_stage(level, STAGE_3_OUTPUT) {
            Ok(q) => q,
            Err(e) => {
                error!("level {}: failed to read stage input: {}", level, e);
                continue;
            }
        };

        if questions.is_empty() {
            warn!("level {}: no questions found, skipping", level);
            continue;
        }

        // Apply leveling to each question
        questions.iter_mut().for_each(|q| q.leveling());

        // Log tail for quick verification
        let tail_count = questions.len().min(2);
        info!(
            "level {}: total {}, tail: {:?}",
            level,
            questions.len(),
            &questions[questions.len() - tail_count..]
        );

        match write_questions_to_stage(level, STAGE_4_OUTPUT, &questions) {
            Ok(_) => info!("level {}: wrote {}", level, STAGE_4_OUTPUT),
            Err(e) => error!("level {}: failed to write: {}", level, e),
        }
    }

    info!("done");
}
