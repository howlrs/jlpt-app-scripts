use std::env;

use log::{error, info};

mod utils;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let output_dir = "output";
    let target_dir = "questions";
    let target_levels = ["n2", "n3"];
    let target_filename = "removed_duplicate_rows_concat_all.json";

    // 出力ファイル
    let is_output = true;
    let output_file = "3_numbering_data.json";

    // レベルごとの実行
    // 対象ディレクトリを指定し、ファイルを読み込む
    for level in target_levels {
        let target_level_dir = {
            let current_dir = env::current_dir().unwrap();
            let question_dir = current_dir.join(output_dir).join(target_dir);

            question_dir.join(level)
        };

        let target_filepath = target_level_dir.join(target_filename);
        if !target_filepath.exists() {
            error!("ファイルが存在しません: {:?}", target_filepath);
            continue;
        }

        // read file to string
        let content = crate::utils::read_file(target_filepath.clone());
        let mut questions = match serde_json::from_str::<Vec<crate::utils::Question>>(&content) {
            Ok(questions) => questions,
            Err(e) => {
                error!("ファイルが存在しません: {:?}, {:?}", target_filepath, e);
                continue;
            }
        };

        // question.id, sub_question.id を一意にする
        for question in questions.iter_mut() {
            //初期IDはuuidで生成
            question.numbering();
        }

        // 末尾2配列を表示
        info!(
            "tail: {:?}",
            questions
                .iter()
                .skip(questions.len() - 2)
                .collect::<Vec<&crate::utils::Question>>()
        );

        if is_output {
            // concat content
            let to_json = serde_json::to_string_pretty(&questions).unwrap();

            // save new file
            let output_filepath = target_level_dir.join(output_file);
            crate::utils::write_file(output_filepath, to_json.as_str());
        }
    }
    info!("done");
}
