use core::panic;
use std::path::PathBuf;
use std::vec;
use std::{env, time::Instant};

use google_generative_ai_rs::v1::{
    api::Client,
    gemini::{Content, Model, Part, Role, request::Request},
};
use log::{error, info};

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 経過時間計測
    let start = Instant::now();

    // 使用ディレクトリ
    let prompt_dir = "prompts";
    // 対象レベル
    let target_levels = vec!["n3", "n2"];
    // APIリクエスト回数
    let count = 1;

    for target_level in target_levels {
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

            let create_content = read_prompt_file(create_filepath);

            (
                replace_level(&create_content, target_level),
                read_prompt_file(prepare_filepath),
                read_prompt_file(detail_filepath),
                read_prompt_file(category_filepath),
            )
        };

        // Gemini API model, keyを取得
        let (key, model) = get_key_and_model();
        info!(
            "key: {}, model: {}\ngenerate: {}",
            key,
            model,
            first_prompt.chars().take(200).collect::<String>()
        );

        // 出力履歴を渡し重複防止を行ったが、会話自己相関があるためか強めの重複が発生した
        // よって、ランダム出力としている
        let prompt = format!(
            "{}\n\n{}\n\n{}\n\n{}",
            first_prompt, base_info, detail_prompt, category_prompt
        );
        let output_level_dir = env::current_dir()
            .unwrap()
            .join("output")
            .join("questions")
            .join(target_level);

        for i in 0..count {
            let res = request_gemini_api(key.clone(), model.clone(), &prompt).await;
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
                    str_to_save_file(r.as_str(), save_filepath);
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

fn read_prompt_file(abs_filename: PathBuf) -> String {
    std::fs::read_to_string(abs_filename).unwrap_or_else(|e| {
        panic!("ファイルの読み込みに失敗しました: {}", e);
    })
}

fn get_key_and_model() -> (String, String) {
    let key = match env::var("GOOGLE_GEMINI_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            panic!("GOOGLE_GEMINI_API_KEY not set");
        }
    };
    let models = match env::var("GEMINI_MODELS") {
        Ok(m) => m,
        Err(_) => {
            panic!("GEMINI_MODELS not set");
        }
    };
    let models = models.split(",").collect::<Vec<&str>>();
    if models.len() != 2 {
        panic!("GEMINI_MODELS must be 2 models");
    }
    let model = models[0];

    (key, model.to_string())
}

// gemini api request
async fn request_gemini_api(key: String, model: String, text: &str) -> Result<String, String> {
    let client = Client::new_from_model(Model::Custom(model.to_string()), key.to_string());
    let request = Request {
        contents: vec![Content {
            role: Role::User,
            parts: vec![Part {
                text: Some(text.to_string()),
                inline_data: None,
                file_data: None,
                video_metadata: None,
            }],
        }],
        tools: vec![],
        safety_settings: vec![],
        generation_config: None,
        system_instruction: None,
    };

    let response = match client.post(75, &request).await {
        Ok(r) => r,
        Err(e) => {
            return Err(format!("Error: {}", e));
        }
    };

    let into_rest = response.rest().unwrap();
    match into_rest
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .and_then(|p| p.text.as_ref())
    {
        Some(t) => Ok(t.to_string()),
        None => Err("Error".to_string()),
    }
}

fn str_to_save_file(text: &str, filename: PathBuf) {
    std::fs::write(filename, text).unwrap_or_else(|e| {
        panic!("ファイルの書き込みに失敗しました: {}", e);
    });
}

fn replace_level(text: &str, target_level: &str) -> String {
    let replace_text = target_level.to_string().to_uppercase();
    text.replace("**LEVEL**", &format!("**{}**", replace_text))
}
