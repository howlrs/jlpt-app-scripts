use std::{collections::HashMap, env};

use log::{error, info};

mod utils;
use crate::utils::{Question, read_file, write_file};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let output_dir = "output";
    let target_dir = "questions";
    let target_levels = ["n1", "n2", "n3", "n4", "n5"];
    let target_file = "concat_with_struct.json";

    // 出力先ファイル
    let is_output = true;
    let new_file = "removed_duplicate_rows_concat_all.json";

    // レベルごとの実行
    // 対象ディレクトリを指定し、ファイルを読み込む
    for level in target_levels {
        let questions = read_questions(output_dir, target_dir, target_file, level);

        let mut removed_duplicate_questions = HashMap::new();
        let mut duplicate_questions = HashMap::new();

        // HashMapのKeyにSubQuestion.sentenceを追加し親Questionを保存
        // 重複があった場合はdupulicate_questionsに保存
        for question in questions {
            // 最初に元の質問文を保存
            removed_duplicate_questions.insert(question.sentence.clone(), question.clone());

            // サブ質問をイテレート
            for sub_question in &question.sub_questions {
                if let Some(sentence) = &sub_question.sentence {
                    match removed_duplicate_questions.entry(sentence.clone()) {
                        std::collections::hash_map::Entry::Occupied(_) => {
                            // 重複があった場合はdupulicate_questionsに保存
                            // 再重複は無視
                            duplicate_questions.insert(sentence.clone(), question.clone());
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            // 重複がない場合はHashMapに追加
                            entry.insert(question.clone());
                        }
                    }
                }
            }
        }

        // 重複を排除したlength, 重複したlengthを出力
        info!(
            "leve: {}: new array: {}, duplicate: {}",
            level,
            removed_duplicate_questions.len(),
            duplicate_questions.len()
        );

        if is_output {
            let target_level_dir = {
                let current_dir = env::current_dir().unwrap();
                current_dir.join(output_dir).join(target_dir).join(level)
            };
            let output_file = target_level_dir.join(new_file);

            let concat_content = removed_duplicate_questions
                .values()
                .chain(duplicate_questions.values())
                .cloned()
                .collect::<Vec<Question>>();
            let to_json_value = serde_json::to_string_pretty(&concat_content).unwrap();

            write_file(output_file, &to_json_value);
        }
    }
    info!("done");
}

fn read_questions(
    output_dir: &str,
    target_dir: &str,
    target_file: &str,
    level: &str,
) -> Vec<crate::utils::Question> {
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
    match serde_json::from_str::<Vec<crate::utils::Question>>(&content) {
        Ok(questions) => questions,
        Err(e) => {
            error!("JSONデータのパースに失敗しました: {:?}", e);
            vec![]
        }
    }
}
