use std::{collections::HashMap, env, time::Instant};

use log::{error, info, warn};

mod utils;

/// カテゴリ別の目標問題数（各カテゴリ定義の問題数 × 倍率）
const TARGET_MULTIPLIER: u32 = 10;
/// カテゴリ別の最小問題数
const MIN_PER_CATEGORY: u32 = 30;

#[tokio::main]
async fn main() {
    utils::init_logger();

    let start = Instant::now();
    let max_retries: u32 = 3;
    let request_interval_secs: u64 = env::var("REQUEST_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let (key, primary_model, fallback_model) = utils::get_key_and_models();
    info!(
        "primary: {}, fallback: {}, interval: {}s, multiplier: {}x, min/cat: {}",
        primary_model, fallback_model, request_interval_secs, TARGET_MULTIPLIER, MIN_PER_CATEGORY
    );

    let mut grand_total = 0u32;
    let mut grand_fail = 0u32;

    for level in utils::LEVELS {
        // カテゴリ定義を読んで目標件数を決定
        let categories = match parse_categories(level) {
            Ok(cats) => cats,
            Err(e) => {
                error!("[{}] カテゴリ定義読込失敗: {}", level, e);
                continue;
            }
        };

        let prompt_base = match build_prompt_base(level) {
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

        // 既存データからカテゴリ別の生成済み件数を集計
        let existing_counts = count_existing_by_category(&output_dir);

        info!("[{}] {}カテゴリ, 既存データ: {:?}", level, categories.len(), existing_counts);

        let mut level_success = 0u32;
        let mut level_fail = 0u32;

        for (cat_name, target_count) in &categories {
            let target = (*target_count * TARGET_MULTIPLIER).max(MIN_PER_CATEGORY);
            let existing = existing_counts.get(cat_name).copied().unwrap_or(0);

            if existing >= target {
                info!("[{}/{}] 目標{}件達成済み(既存{}件), スキップ", level, cat_name, target, existing);
                continue;
            }

            let remaining = target - existing;
            // 1リクエストで約5問生成されるので、必要リクエスト数を計算
            let requests_needed = (remaining / 5).max(1);

            info!(
                "[{}/{}] 目標{}件, 既存{}件, 残り{}件 → {}リクエスト",
                level, cat_name, target, existing, remaining, requests_needed
            );

            for i in 0..requests_needed {
                let category_prompt = format!(
                    "{}\n\n**今回は以下のカテゴリの問題のみを生成してください:**\nカテゴリ: {}\n5問以上生成してください。他のカテゴリの問題は生成しないでください。",
                    prompt_base, cat_name
                );

                let result = request_with_fallback(
                    &key,
                    &primary_model,
                    &fallback_model,
                    &category_prompt,
                    &system_instruction,
                    max_retries,
                )
                .await;

                match result {
                    Some((text, used_model)) => {
                        let cleaned = utils::remove_ai_json_syntax(&text);

                        if let Ok(mut json_val) =
                            serde_json::from_str::<serde_json::Value>(&cleaned)
                        {
                            if let Some(arr) = json_val.as_array_mut() {
                                for item in arr.iter_mut() {
                                    if let Some(obj) = item.as_object_mut() {
                                        obj.insert(
                                            "generated_by".to_string(),
                                            serde_json::Value::String(used_model.clone()),
                                        );
                                    }
                                }
                            }
                            let output_json =
                                serde_json::to_string_pretty(&json_val).unwrap();
                            let timestamp = chrono::Utc::now().timestamp_millis();
                            let filepath =
                                output_dir.join(format!("{}.json", timestamp));
                            utils::write_file(filepath, &output_json);
                            level_success += 1;
                        } else {
                            warn!(
                                "[{}/{}] 無効JSON ({}文字)",
                                level, cat_name, cleaned.len()
                            );
                            level_fail += 1;
                        }
                    }
                    None => {
                        level_fail += 1;
                    }
                }

                // 進捗表示（10リクエストごと）
                if (i + 1) % 10 == 0 {
                    info!(
                        "[{}/{}] {}/{} requests, Elapsed: {:?}",
                        level,
                        cat_name,
                        i + 1,
                        requests_needed,
                        start.elapsed()
                    );
                }

                tokio::time::sleep(std::time::Duration::from_secs(request_interval_secs)).await;
            }
        }

        info!(
            "=== {} 完了: 成功={}, 失敗={}, Elapsed: {:?} ===",
            level, level_success, level_fail, start.elapsed()
        );

        grand_total += level_success;
        grand_fail += level_fail;
    }

    info!(
        "=== 全体完了: 成功={}, 失敗={}, 総Elapsed: {:?} ===",
        grand_total, grand_fail, start.elapsed()
    );
}

/// カテゴリ名と目標問題数のペアを返す
fn parse_categories(level: &str) -> Result<Vec<(String, u32)>, String> {
    let current_dir = env::current_dir().map_err(|e| e.to_string())?;
    let cat_file = current_dir
        .join(utils::PROMPT_DIR)
        .join(level)
        .join("ja-categories.md");

    if !cat_file.exists() {
        return Err(format!("ファイル不在: {:?}", cat_file));
    }

    let content = std::fs::read_to_string(&cat_file).map_err(|e| e.to_string())?;
    let mut categories = Vec::new();

    for line in content.lines() {
        // "1.  **漢字読み:** 高度な漢字で... (6問)" のようなパターン
        if let Some(name_start) = line.find("**") {
            let after_star = &line[name_start + 2..];
            if let Some(name_end) = after_star.find("**") {
                let name = after_star[..name_end]
                    .trim_end_matches(':')
                    .trim_end_matches('：')
                    .trim()
                    .to_string();

                // (N問) パターンから問題数を抽出
                let count = if let Some(paren_start) = line.rfind('(') {
                    let after_paren = &line[paren_start + 1..];
                    after_paren
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .parse::<u32>()
                        .unwrap_or(5)
                } else {
                    5 // デフォルト5問
                };

                if !name.is_empty() {
                    categories.push((name, count));
                }
            }
        }
    }

    Ok(categories)
}

/// ベースプロンプトを構築（カテゴリ指定なし）
fn build_prompt_base(level: &str) -> Result<String, String> {
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

/// 既存の生成済みファイルからカテゴリ別件数を集計
fn count_existing_by_category(output_dir: &std::path::Path) -> HashMap<String, u32> {
    let mut counts: HashMap<String, u32> = HashMap::new();

    for file in utils::walk_dir(output_dir) {
        if file.extension().map(|e| e != "json").unwrap_or(true) {
            continue;
        }
        // ステージファイルはスキップ
        let fname = file.file_name().unwrap_or_default().to_string_lossy();
        if fname.starts_with("1_") || fname.starts_with("2_") || fname.starts_with("3_")
            || fname.starts_with("4_") || fname.starts_with("5_")
        {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(&file) {
            if let Ok(questions) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                for q in questions {
                    if let Some(cat) = q.get("category_name").and_then(|v| v.as_str()) {
                        *counts.entry(cat.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    counts
}

fn build_system_instruction() -> String {
    r#"あなたはJLPT（日本語能力試験）の公式問題作成の専門家です。以下を厳守してください：
1. 出力は必ず有効なJSONのみ（マークダウン記法禁止、説明文禁止）
2. 選択肢は必ず4つ。正解は必ず1つだけ
3. 正解の位置（1〜4）を偏らせない
4. 誤答は「一見正しそうだが明確な理由で不正解」であること
5. 選択肢の長さ・構造・語彙レベルを揃えること
6. 指定されたレベルの語彙・文法範囲を厳守すること
7. 指定されたカテゴリの問題のみを生成すること"#
        .to_string()
}

/// フォールバックモデル付きリトライ
async fn request_with_fallback(
    key: &str,
    primary_model: &str,
    fallback_model: &str,
    prompt: &str,
    system_instruction: &str,
    max_retries: u32,
) -> Option<(String, String)> {
    for attempt in 0..=max_retries {
        match utils::request_gemini_api(
            key.to_string(),
            primary_model.to_string(),
            prompt,
            Some(system_instruction),
        )
        .await
        {
            Ok(text) => return Some((text, primary_model.to_string())),
            Err(e) => {
                if attempt >= max_retries {
                    warn!(
                        "プライマリ({})が{}回失敗。フォールバック試行: {}",
                        primary_model, max_retries, e
                    );
                    break;
                }
                let wait = 60 * (attempt + 1) as u64;
                warn!(
                    "[{}] リトライ {}/{}: {} - {}秒待機",
                    primary_model, attempt + 1, max_retries, e, wait
                );
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            }
        }
    }

    match utils::request_gemini_api(
        key.to_string(),
        fallback_model.to_string(),
        prompt,
        Some(system_instruction),
    )
    .await
    {
        Ok(text) => {
            info!("フォールバック({})で成功", fallback_model);
            Some((text, fallback_model.to_string()))
        }
        Err(e) => {
            error!("プライマリ・フォールバック両方失敗: {}", e);
            None
        }
    }
}
