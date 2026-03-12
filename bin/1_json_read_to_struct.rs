use std::collections::HashMap;

use log::{error, info};

mod utils;
use crate::utils::{
    ensure_dir, level_dir, read_file, remove_ai_json_syntax, walk_dir, write_file,
    write_questions_to_stage, Question, LEVELS, STAGE_1_OUTPUT,
};

/// 対象のディレクトリ走査しファイルを読み込む
/// ファイルを文字列として読み込み、JSONをパースする
/// パースに成功した場合は、Question構造体に変換する
/// パースに失敗した場合は、エラーを出力する
fn main() {
    crate::utils::init_logger();

    let start = std::time::Instant::now();

    // エラー確認用
    // 読み込み失敗ファイル配列
    // JSONファイルパースでエラーが発生した場合、ファイル名とソースパスを格納する
    let mut error_files: HashMap<std::path::PathBuf, String> = HashMap::new();

    // レベルごとの実行
    for level in LEVELS {
        let target_level_dir = level_dir(level);

        let mut all_questions = vec![];
        for file in walk_dir(&target_level_dir) {
            let read_content = read_file(file.clone());
            let cleaned_content = remove_ai_json_syntax(&read_content);

            match serde_json::from_str::<Vec<Question>>(&cleaned_content) {
                Ok(questions) => {
                    all_questions.extend(questions);
                }
                Err(e) => {
                    error_files.insert(file, e.to_string());
                    continue;
                }
            };
        }
        if all_questions.is_empty() {
            error!("No question data found: {:?}", target_level_dir);
            continue;
        }

        info!(
            "level {}: questions: {}, sub_questions: {}",
            level,
            all_questions.len(),
            all_questions
                .iter()
                .fold(0, |acc, q| acc + q.sub_questions.len())
        );

        // Save parsed output
        match write_questions_to_stage(level, STAGE_1_OUTPUT, &all_questions) {
            Ok(_) => info!("level {}: wrote {}", level, STAGE_1_OUTPUT),
            Err(e) => error!("level {}: failed to write {}: {}", level, STAGE_1_OUTPUT, e),
        }
    }

    info!("done, elapsed: {:?}", start.elapsed());

    // Copy failed files to err/ directory for inspection
    if !error_files.is_empty() {
        let err_dir = level_dir("..").join("err");
        if let Err(e) = ensure_dir(&err_dir) {
            error!("Failed to create err directory: {}", e);
            return;
        }

        for (file, e) in &error_files {
            error!("Error file: {:?}, {:?}", file, e);

            // FIX: Use file_name() instead of string split for path separator portability
            if let Some(filename) = file.file_name() {
                let output_file = err_dir.join(filename);
                // FIX: Use the full source path (file) for fs::copy
                if let Err(copy_err) = std::fs::copy(file, &output_file) {
                    error!("Failed to copy error file {:?}: {}", file, copy_err);
                }
            } else {
                error!("Could not extract filename from path: {:?}", file);
            }
        }
        error!("Total error files: {}", error_files.len());
    }
}
