use std::collections::HashMap;

use log::{error, info, warn};

mod utils;
use crate::utils::{
    read_questions_from_stage, write_file, level_dir,
    CatValue, LEVELS, STAGE_4_OUTPUT, STAGE_5_OUTPUT,
};

/// Extract unique category metadata from leveled questions across all levels.
/// Output a single combined JSON file at the questions root directory.
fn main() {
    crate::utils::init_logger();

    let mut vec_catvalue = Vec::new();

    for level in LEVELS {
        let questions = match read_questions_from_stage(level, STAGE_4_OUTPUT) {
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

        // Extract unique categories for this level
        let mut cat_hash = HashMap::new();
        for q in &questions {
            // CRITICAL FIX: Graceful error handling for category_id parsing
            // instead of .as_ref().unwrap().parse().unwrap()
            let category_id_str = match q.category_id.as_ref() {
                Some(id) => id,
                None => {
                    warn!(
                        "level {}: question '{}' has no category_id, skipping",
                        level, q.sentence
                    );
                    continue;
                }
            };

            let category_id: u32 = match category_id_str.parse() {
                Ok(id) => id,
                Err(e) => {
                    warn!(
                        "level {}: question '{}' has invalid category_id '{}': {}, skipping",
                        level, q.sentence, category_id_str, e
                    );
                    continue;
                }
            };

            cat_hash.insert(
                format!("{}-{}", q.level_id, category_id),
                CatValue {
                    level_id: q.level_id,
                    id: category_id,
                    name: q.category_name.clone(),
                },
            );
        }

        for (_, value) in cat_hash {
            vec_catvalue.push(value);
        }
    }

    info!("categories meta: {:?}", vec_catvalue);

    // Write to questions root directory (one level above any specific level)
    let to_json_str = serde_json::to_string_pretty(&vec_catvalue).unwrap();
    // level_dir("..") would be wrong; go up from any level dir to get questions root
    let output_dir = level_dir("n1").parent().unwrap().to_path_buf();
    let output_filepath = output_dir.join(STAGE_5_OUTPUT);
    write_file(output_filepath, &to_json_str);

    info!("done");
}
