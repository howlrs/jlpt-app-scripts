use std::{env, time::Instant};

use log::{error, info, warn};

mod utils;

#[tokio::main]
async fn main() {
    utils::init_logger();

    let start = Instant::now();
    let count: u32 = env::var("GENERATE_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let max_retries: u32 = 3;
    let request_interval_secs: u64 = env::var("REQUEST_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    let (key, model) = utils::get_key_and_model();
    info!("model: {}, count/level: {}, interval: {}s", model, count, request_interval_secs);

    let mut total_success = 0u32;
    let mut total_fail = 0u32;
    let mut total_invalid_json = 0u32;

    for level in utils::LEVELS {
        let prompt = match build_prompt(level) {
            Ok(p) => p,
            Err(e) => {
                error!("[{}] プロンプト構築失敗: {}", level, e);
                continue;
            }
        };

        let system_instruction = build_system_instruction();
        let output_dir = utils::level_dir(level);
        if let Err(e) = utils::ensure_dir(&output_dir) {
            error!("[{}] 出力ディレクトリ作成失敗: {}", level, e);
            continue;
        }

        let mut level_success = 0u32;
        let mut level_fail = 0u32;
        let mut level_invalid_json = 0u32;

        info!("[{}] 生成開始 ({}件)", level, count);

        for i in 0..count {
            let result = request_with_retry(
                &key,
                &model,
                &prompt,
                &system_instruction,
                max_retries,
            )
            .await;

            match result {
                Some(text) => {
                    let cleaned = utils::remove_ai_json_syntax(&text);

                    if serde_json::from_str::<serde_json::Value>(&cleaned).is_ok() {
                        let timestamp = chrono::Utc::now().timestamp();
                        let filepath = output_dir.join(format!("{}.json", timestamp));
                        utils::write_file(filepath, &cleaned);
                        level_success += 1;

                        if (i + 1) % 50 == 0 || i == 0 {
                            info!(
                                "[{}/{}] {}件完了, 成功={}, 失敗={}, 無効JSON={}, Elapsed: {:?}",
                                level, count, i + 1, level_success, level_fail, level_invalid_json, start.elapsed()
                            );
                        }
                    } else {
                        level_invalid_json += 1;
                        warn!(
                            "[{}/{}] 無効なJSON (先頭80文字: {})",
                            level,
                            i,
                            cleaned.chars().take(80).collect::<String>()
                        );
                    }
                }
                None => {
                    level_fail += 1;
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(request_interval_secs)).await;
        }

        info!(
            "=== {} 完了: 成功={}, 失敗={}, 無効JSON={} ===",
            level, level_success, level_fail, level_invalid_json
        );

        total_success += level_success;
        total_fail += level_fail;
        total_invalid_json += level_invalid_json;
    }

    info!(
        "=== 全体完了: 成功={}, 失敗={}, 無効JSON={}, 総Elapsed: {:?} ===",
        total_success, total_fail, total_invalid_json, start.elapsed()
    );
}

/// プロンプトを構築する
fn build_prompt(level: &str) -> Result<String, String> {
    let current_dir = env::current_dir().map_err(|e| e.to_string())?;
    let prompt_dir = current_dir.join(utils::PROMPT_DIR);
    let level_dir = prompt_dir.join(level);

    let files = [
        prompt_dir.join("create-question_to_json.md"),
        prompt_dir.join("base-info.md"),
        level_dir.join("ja-question.md"),
        level_dir.join("ja-categories.md"),
        level_dir.join("ja-answer.md"),
        level_dir.join("ja-hint.md"),
        level_dir.join("ja-select.md"),
    ];

    let mut parts = Vec::new();
    for file in &files {
        if !file.exists() {
            // ja-answer, ja-hint, ja-select はオプション（レベルによっては不在）
            let filename = file.file_name().unwrap_or_default().to_string_lossy();
            if filename.starts_with("ja-answer")
                || filename.starts_with("ja-hint")
                || filename.starts_with("ja-select")
            {
                continue;
            }
            return Err(format!("必須ファイル不在: {:?}", file));
        }
        parts.push(std::fs::read_to_string(file).map_err(|e| format!("{:?}: {}", file, e))?);
    }

    let mut prompt = parts.join("\n\n");
    let level_upper = level.to_uppercase();
    prompt = prompt.replace("**LEVEL**", &format!("**{}**", level_upper));

    Ok(prompt)
}

/// system_instructionを構築する
fn build_system_instruction() -> String {
    r#"あなたはJLPT（日本語能力試験）の公式問題作成の専門家です。以下を厳守してください：
1. 出力は必ず有効なJSONのみ（マークダウン記法禁止、説明文禁止）
2. 選択肢は必ず4つ。正解は必ず1つだけ
3. 正解の位置（1〜4）を偏らせない
4. 誤答は「一見正しそうだが明確な理由で不正解」であること
5. 選択肢の長さ・構造・語彙レベルを揃えること
6. 指定されたレベルの語彙・文法範囲を厳守すること"#
        .to_string()
}

/// リトライ付きAPI呼び出し
async fn request_with_retry(
    key: &str,
    model: &str,
    prompt: &str,
    system_instruction: &str,
    max_retries: u32,
) -> Option<String> {
    for attempt in 0..=max_retries {
        match utils::request_gemini_api(
            key.to_string(),
            model.to_string(),
            prompt,
            Some(system_instruction),
        )
        .await
        {
            Ok(text) => return Some(text),
            Err(e) => {
                if attempt >= max_retries {
                    error!("{}回リトライ後も失敗: {}", max_retries, e);
                    return None;
                }
                let wait = 60 * (attempt + 1) as u64;
                warn!(
                    "APIエラー (リトライ {}/{}): {} - {}秒待機",
                    attempt + 1,
                    max_retries,
                    e,
                    wait
                );
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            }
        }
    }
    None
}
