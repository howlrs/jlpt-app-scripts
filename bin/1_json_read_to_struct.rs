use std::{collections::HashMap, env};

use log::{debug, error, info};

mod utils;

/// 対象のディレクトリ走査しファイルを読み込む
/// ファイルを文字列として読み込み、JSONをパースする
/// パースに成功した場合は、Question構造体に変換する
/// パースに失敗した場合は、エラーを出力する
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 処理時間の計測
    let start = std::time::Instant::now();

    let output_dir = "output";
    let target_dir = "questions";
    let target_levels = ["n2", "n3"];
    // let target_file = "concat_all.json";

    // 出力先ファイル
    let new_file = "concat_with_struct.json";
    let is_output = true;

    // エラー確認用
    // 読み込み失敗ファイル配列
    // JSONファイルパースでエラーが発生した場合、ファイル名を格納する
    let mut error_files = HashMap::new();

    // レベルごとの実行
    // 対象ディレクトリを指定し、ファイルを読み込む
    for level in target_levels {
        let target_level_dir = {
            let current_dir = env::current_dir().unwrap();
            current_dir.join(output_dir).join(target_dir).join(level)
        };

        let mut all_questions = vec![];
        for file in crate::utils::walk_dir(&target_level_dir).into_iter() {
            let read_content = crate::utils::read_file(file.clone());
            let cleaned_content = remove_ai_json_syntax(&read_content);

            match serde_json::from_str::<Vec<crate::utils::Question>>(&cleaned_content) {
                Ok(questions) => {
                    all_questions.extend(questions);
                }
                Err(e) => {
                    error!("JSONのパースに失敗しました: {:?},  {}", file, e);
                    error_files.insert(file.display().to_string(), e);
                    continue;
                }
            };
        }
        if all_questions.is_empty() {
            error!("ファイルデータが存在しません: {:?}", target_level_dir);
            continue;
        }

        // 2配列を出力
        info!(
            "length: {}, inner subq len: {}",
            all_questions.len(),
            all_questions
                .iter()
                .fold(0, |acc, q| acc + q.sub_questions.len())
        );

        // save concat file
        if is_output {
            let new_file_path = target_level_dir.join(new_file);
            let new_content = serde_json::to_string_pretty(&all_questions).unwrap();
            crate::utils::write_file(new_file_path, &new_content);
        }
    }
    info!("done, elapsed: {:?}", start.elapsed());
    // エラー確認用
    // 失敗したファイルを別ディレクトリにコピー
    let target_dir = env::current_dir()
        .unwrap()
        .join(output_dir)
        .join(target_dir);
    let output_dir = target_dir.join("err");

    if !error_files.is_empty() {
        error!("Error file: {:?}", error_files.len());
    }
    error_files.iter().for_each(|(file, e)| {
        error!("Error file: {:?}, {:?}", file, e);

        if is_output {
            // ファイルパスからファイル名を取得
            let file = file.split("\\").last().unwrap();
            let output_file = output_dir.join(file);
            std::fs::copy(file, output_file).unwrap();
        }
    });
}

// AI出力のJSON構文宣言を削除する
fn remove_ai_json_syntax(content: &str) -> String {
    if !content.starts_with("```json") && !content.ends_with("```") {
        content.to_string()
    } else {
        debug!("remove ai json syntax");
        // replace ```json
        let content = content.replacen(r"```json", "", 1);
        // replace ```
        content.replacen("```", "", 1)
    }
}
