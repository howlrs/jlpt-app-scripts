use core::panic;
use std::vec;
use std::{env, time::Instant};

use log::{error, info};

mod utils;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 経過時間計測
    let start = Instant::now();
    // 各レベルのAPIリクエスト回数
    let count = 1000;

    // 使用ディレクトリ
    let prompt_dir = "prompts";
    // 対象レベル
    let target_levels = ["n1", "n2", "n3", "n4", "n5"];

    // 出力先ディレクトリの作成
    let output_dir = env::current_dir().unwrap().join("output").join("questions");

    for target_level in target_levels.into_iter() {
        // プロンプトファイルの読み込み
        let (first_prompt, base_info, detail_prompt, category_prompt) = {
            let current_dir = env::current_dir().unwrap();
            let prompt_dir = current_dir.join(prompt_dir);

            // 主となる命令文
            let create_filepath = prompt_dir.join("create-question_to_json.md");
            // 出力の基礎背景情報
            let prepare_filepath = prompt_dir.join("base-info.md");

            // レベルごとの詳細情報
            let level_dir = prompt_dir.join(target_level);
            let detail_filepath = level_dir.join("ja-question.md");
            // 出力の詳細情報
            let category_filepath = level_dir.join("ja-categories.md");

            let filepaths = [
                &create_filepath,
                &prepare_filepath,
                &detail_filepath,
                &category_filepath,
            ];

            // ファイル存在チェック
            for filepath in filepaths {
                if !filepath.exists() {
                    panic!("File not found: {:?}", filepath);
                }
            }

            let create_content = crate::utils::read_file(create_filepath);

            (
                crate::utils::replace_level(&create_content, target_level),
                crate::utils::read_file(prepare_filepath),
                crate::utils::read_file(detail_filepath),
                crate::utils::read_file(category_filepath),
            )
        };

        // Gemini API model, keyを取得
        let (key, model) = crate::utils::get_key_and_model();

        // 出力履歴を渡し重複防止を行ったが、会話自己相関があるためか強めの重複が発生した
        // よって、ランダム出力としている
        let prompt = format!(
            "{}\n\n{}\n\n{}\n\n{}",
            first_prompt, base_info, detail_prompt, category_prompt
        );
        let output_level_dir = output_dir.join(target_level);

        for i in 0..count {
            let res = crate::utils::request_gemini_api(key.clone(), model.clone(), &prompt).await;
            match res {
                Ok(r) => {
                    // 結果と文字数を表示
                    // 問題出力を保持し増え続けるため、監視が必要
                    // Token数ではなくあくまで文字数であることに注意
                    info!("success: {}, Elapsed: {:?}", i, start.elapsed());
                    // タイムスタンプでファイルを出力
                    let now_timestamp = chrono::Utc::now();
                    let save_filepath =
                        output_level_dir.join(format!("{}.json", now_timestamp.timestamp()));
                    crate::utils::write_file(save_filepath, r.as_str());
                }
                Err(e) => {
                    error!("Error: {}", e);
                    error!("Retry after 60 seconds, in {}, {}", i, target_level);

                    // APIエラーなどで失敗した場合は待機後リトライ
                    // Gemini API Limitは分ごとの確認があるため、当該回避のみを行う
                    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                    continue;
                }
            };

            // 20秒待つ
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        }

        info!("Done, Elapsed: {:?}", start.elapsed());
    }
}
