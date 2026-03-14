/// ポジショニングマップ方式: ギャップカテゴリのみターゲット生成
///
/// 既存パイプライン出力 (4_leveled.json) を分析し、
/// 目標に達していないカテゴリのみ集中的に問題生成する。
///
/// 環境変数:
///   TARGET_MIN=100       各カテゴリの最小子問題数（デフォルト: 100）
///   REQUEST_INTERVAL=10  APIリクエスト間隔秒（デフォルト: 10）
///   BUFFER_RATIO=1.5     dedup/validation による減衰を見込んだバッファ倍率
use std::{env, time::Instant};

use log::{error, info, warn};
use rand::Rng;

mod utils;

/// ギャップ定義: (level, category_id, category_name, 現在の子問題数, 不足数)
struct Gap {
    level: &'static str,
    cat_id: u32,
    cat_name: &'static str,
    current: u32,
    deficit: u32,
}

fn identify_gaps(target_min: u32) -> Vec<Gap> {
    // 各レベルの標準カテゴリ定義: (cat_id, cat_name)
    let standard_cats: Vec<(&str, Vec<(u32, &str)>)> = vec![
        ("n1", vec![
            (2, "漢字読み"), (3, "表記"), (4, "語形成"), (5, "文脈規定"),
            (6, "言い換え類義"), (7, "用法"),
            (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
            (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "統合理解"),
            (14, "主張理解 (長文)"), (15, "情報検索"),
            (17, "課題理解"), (18, "ポイント理解"), (19, "概要理解"), (20, "即時応答"), (22, "発話表現"),
        ]),
        ("n2", vec![
            (2, "漢字読み"), (3, "表記"), (4, "語形成"), (5, "文脈規定"),
            (6, "言い換え類義"), (7, "用法"),
            (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
            (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "統合理解"),
            (14, "主張理解 (長文)"), (15, "情報検索"),
            (17, "課題理解"), (18, "ポイント理解"), (19, "概要理解"), (20, "即時応答"),
        ]),
        ("n3", vec![
            (2, "漢字読み"), (3, "表記"), (4, "文脈規定"), (5, "言い換え類義"), (6, "用法"),
            (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
            (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "内容理解 (長文)"), (14, "情報検索"),
            (16, "課題理解"), (17, "ポイント理解"), (18, "概要理解"), (19, "発話表現"), (20, "即時応答"),
        ]),
        ("n4", vec![
            (2, "漢字読み"), (3, "表記"), (4, "文脈規定"), (5, "言い換え類義"), (6, "用法"),
            (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
            (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "情報検索"),
            (15, "課題理解"), (16, "ポイント理解"), (17, "概要理解"), (18, "発話表現"), (19, "即時応答"),
        ]),
        ("n5", vec![
            (2, "漢字読み"), (3, "表記"), (4, "文脈規定"), (5, "言い換え類義"), (6, "用法"),
            (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
            (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "情報検索"),
            (15, "課題理解"), (16, "ポイント理解"), (17, "概要理解"), (18, "発話表現"), (19, "即時応答"),
        ]),
    ];

    let mut gaps = Vec::new();

    for (level, cats) in &standard_cats {
        // パイプライン最終出力から現在の子問題数を集計
        let actual = match utils::read_questions_from_stage(level, utils::STAGE_4_OUTPUT) {
            Ok(questions) => {
                let mut counts = std::collections::HashMap::new();
                for q in &questions {
                    let cid: u32 = q.category_id.as_ref()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let subs = q.sub_questions.len() as u32;
                    *counts.entry(cid).or_insert(0u32) += subs;
                }
                counts
            }
            Err(e) => {
                warn!("[{}] パイプライン出力読込失敗: {}", level, e);
                std::collections::HashMap::new()
            }
        };

        for &(cat_id, cat_name) in cats {
            let current = actual.get(&cat_id).copied().unwrap_or(0);
            if current < target_min {
                gaps.push(Gap {
                    level,
                    cat_id,
                    cat_name,
                    current,
                    deficit: target_min - current,
                });
            }
        }
    }

    gaps
}

/// カテゴリ特化の高品質プロンプトを構築
fn build_targeted_prompt(level: &str, cat_name: &str, cat_id: u32, deficit: u32, existing_samples: &[String]) -> String {
    let level_upper = level.to_uppercase();

    // カテゴリ別の追加指示
    let category_hints = match cat_name {
        "文章の文法" => r#"
**文章の文法の問題作成ガイド:**
- 200〜300字程度の短い文章を提示し、文中の空欄に入る適切な接続表現・文法形式を選ばせる
- テーマ例: ビジネスメール、新聞記事、エッセイ、学術論文の抜粋、案内文
- 空欄に入るもの: 接続詞（しかし/ところが/それゆえ等）、接続助詞、文末表現、指示語、助詞の使い分け
- 文章全体の論理的つながりを理解しないと解けない問題にすること
- 単純な文法知識ではなく、文脈理解が求められる問題にすること"#,
        "表記" => r#"
**表記の問題作成ガイド:**
- 下線部のひらがなを漢字で書くとき、正しいものを選ぶ形式
- 同音異義語・同訓異字を活用した選択肢
- 日常生活・ビジネスで使われる漢字を中心に
- 送り仮名の誤りも選択肢に含める"#,
        "主張理解 (長文)" => r#"
**主張理解（長文）の問題作成ガイド:**
- 800〜1200字の論説文・意見文を提示
- 筆者の主張・意見を正確に読み取る問題
- テーマ: 社会問題、環境、教育、技術、文化比較等
- 「筆者が最も言いたいことは何か」「筆者の考えに合うものはどれか」等の設問"#,
        "概要理解" => r#"
**概要理解の問題作成ガイド:**
- 聴解形式: 話の全体的な内容や要点を理解する
- まとまった話を聞いて、話し手の意図・主張を把握する問題
- シチュエーション: 講義、スピーチ、ニュース解説、プレゼンテーション等
- 「この話の要点は何か」「話し手が伝えたいことは何か」"#,
        "発話表現" | "発話表明" => r#"
**発話表現の問題作成ガイド:**
- 場面設定を読み、その状況で適切な発話を選ぶ問題
- 敬語の使い分け、場面に応じた表現の選択
- ビジネス、日常会話、フォーマル/インフォーマルの切り替え
- 依頼、断り、謝罪、提案等の機能別表現"#,
        "用法" => r#"
**用法の問題作成ガイド:**
- 語の用法を問う問題（同じ語の異なる使い方）
- 「〜を使った文として最も適切なものはどれか」形式
- 多義語、慣用的用法、比喩的用法を含む
- 選択肢は全て文法的に正しいが、対象語の用法として適切かを問う"#,
        _ => "",
    };

    // 既存問題のサンプルをanti-duplication用に含める
    let anti_dup = if !existing_samples.is_empty() {
        let samples = existing_samples.iter()
            .take(5)
            .map(|s| format!("- {}", s))
            .collect::<Vec<_>>()
            .join("\n");
        format!(r#"

**重要: 以下の既存問題と類似した問題は絶対に生成しないでください:**
{}
上記とは異なるテーマ・場面・文法ポイントを使用してください。"#, samples)
    } else {
        String::new()
    };

    let seed: u32 = rand::random();

    format!(
        r#"あなたはJLPT {}レベルの「{}」カテゴリの問題を作成する専門家です。

以下の条件で{}の問題を5問以上生成してください：

**レベル:** {}
**カテゴリ:** {}（カテゴリID: {}）

{}
{}

**出力フォーマット:**
JSON配列で出力。各要素は以下の構造:
```json
[
  {{
    "level_name": "{}",
    "category_name": "{}",
    "sentence": "大問の問題文（指示文）",
    "prerequisites": "前提となる文章（必要な場合）",
    "sub_questions": [
      {{
        "sentence": "小問の文",
        "prerequisites": "",
        "select_answer": [
          {{"key": "1", "value": "選択肢1"}},
          {{"key": "2", "value": "選択肢2"}},
          {{"key": "3", "value": "選択肢3"}},
          {{"key": "4", "value": "選択肢4"}}
        ],
        "answer": "正解番号(1-4)"
      }}
    ]
  }}
]
```

**品質基準:**
- 選択肢は必ず4つ、正解は必ず1つ
- 正解の位置を1〜4で均等に分散
- 誤答は「一見正しそうだが明確な理由で不正解」
- 選択肢の長さ・構造を揃える
- {}レベルの語彙・文法範囲を厳守

**多様性指示（シード: {}）:**
- 前回と異なるテーマ・場面・語彙を使用
- 同じ文型・表現パターンの繰り返しを避ける

JSONのみを出力してください。マークダウン記法や説明文は不要です。"#,
        level_upper, cat_name,
        cat_name,
        level_upper, cat_name, cat_id,
        category_hints,
        anti_dup,
        level.to_lowercase(), cat_name,
        level_upper,
        seed,
    )
}

/// パイプライン出力から既存問題のサンプル文を取得
fn get_existing_samples(level: &str, cat_id: u32, max_samples: usize) -> Vec<String> {
    let questions = match utils::read_questions_from_stage(level, utils::STAGE_4_OUTPUT) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };

    questions.iter()
        .filter(|q| {
            q.category_id.as_ref()
                .and_then(|s| s.parse::<u32>().ok())
                .map(|id| id == cat_id)
                .unwrap_or(false)
        })
        .flat_map(|q| {
            q.sub_questions.iter()
                .filter_map(|sq| sq.sentence.clone())
                .take(2)
        })
        .take(max_samples)
        .collect()
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let start = Instant::now();
    let target_min: u32 = env::var("TARGET_MIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let buffer_ratio: f64 = env::var("BUFFER_RATIO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.5);
    let request_interval_secs: u64 = env::var("REQUEST_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let max_retries: u32 = 3;

    info!("=== ポジショニングマップ方式: ターゲット生成 ===");
    info!("目標: 各カテゴリ {}子問題以上, バッファ倍率: {}x", target_min, buffer_ratio);

    let gaps = identify_gaps(target_min);

    if gaps.is_empty() {
        info!("ギャップなし。全カテゴリが目標を達成しています。");
        return;
    }

    info!("ギャップ: {}カテゴリ", gaps.len());
    for g in &gaps {
        info!("  {} cat_{} {}: 現在{}問, 不足{}問", g.level, g.cat_id, g.cat_name, g.current, g.deficit);
    }

    let (key, primary_model, fallback_model) = utils::get_key_and_models();
    info!("primary: {}, fallback: {}", primary_model, fallback_model);

    let system_instruction = r#"あなたはJLPT（日本語能力試験）の公式問題作成の専門家です。以下を厳守してください：
1. 出力は必ず有効なJSONのみ（マークダウン記法禁止、説明文禁止）
2. 選択肢は必ず4つ。正解は必ず1つだけ
3. 正解の位置（1〜4）を偏らせない
4. 誤答は「一見正しそうだが明確な理由で不正解」であること
5. 選択肢の長さ・構造・語彙レベルを揃えること
6. 指定されたレベルの語彙・文法範囲を厳守すること
7. 指定されたカテゴリの問題のみを生成すること"#.to_string();

    let mut total_success = 0u32;
    let mut total_fail = 0u32;

    for gap in &gaps {
        // 不足分 × バッファ倍率で生成目標を計算
        // 1リクエストで約5子問題が生成される想定
        let raw_target = (gap.deficit as f64 * buffer_ratio) as u32;
        let requests_needed = (raw_target / 5).max(1);

        let existing_samples = get_existing_samples(gap.level, gap.cat_id, 10);

        info!(
            "[{}/cat_{}] {} — 不足{}問 → {}リクエスト予定",
            gap.level, gap.cat_id, gap.cat_name, gap.deficit, requests_needed
        );

        let output_dir = utils::level_dir(gap.level);
        if let Err(e) = utils::ensure_dir(&output_dir) {
            error!("[{}] 出力ディレクトリ作成失敗: {}", gap.level, e);
            continue;
        }

        for i in 0..requests_needed {
            let prompt = build_targeted_prompt(
                gap.level, gap.cat_name, gap.cat_id, gap.deficit, &existing_samples
            );

            let result = request_with_fallback(
                &key, &primary_model, &fallback_model,
                &prompt, &system_instruction, max_retries,
            ).await;

            match result {
                Some((text, used_model)) => {
                    let cleaned = utils::remove_ai_json_syntax(&text);
                    if let Ok(mut json_val) = serde_json::from_str::<serde_json::Value>(&cleaned) {
                        if let Some(arr) = json_val.as_array_mut() {
                            for item in arr.iter_mut() {
                                if let Some(obj) = item.as_object_mut() {
                                    obj.insert("generated_by".to_string(),
                                        serde_json::Value::String(used_model.clone()));
                                }
                            }
                        }
                        let output_json = serde_json::to_string_pretty(&json_val).unwrap();
                        let timestamp = chrono::Utc::now().timestamp_millis();
                        let filepath = output_dir.join(format!("{}.json", timestamp));
                        utils::write_file(filepath, &output_json);
                        total_success += 1;
                    } else {
                        warn!("[{}/cat_{}] 無効JSON ({}文字)", gap.level, gap.cat_id, cleaned.len());
                        total_fail += 1;
                    }
                }
                None => {
                    total_fail += 1;
                }
            }

            if (i + 1) % 10 == 0 {
                info!(
                    "[{}/cat_{}] {}/{} requests, Elapsed: {:?}",
                    gap.level, gap.cat_id, i + 1, requests_needed, start.elapsed()
                );
            }

            tokio::time::sleep(std::time::Duration::from_secs(request_interval_secs)).await;
        }
    }

    info!("=== ターゲット生成完了 ===");
    info!("成功: {}, 失敗: {}, 総時間: {:?}", total_success, total_fail, start.elapsed());
    info!("次のステップ: ./run_pipeline.sh --skip-generate で後続パイプライン実行");
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
            key.to_string(), primary_model.to_string(),
            prompt, Some(system_instruction),
        ).await {
            Ok(text) => return Some((text, primary_model.to_string())),
            Err(e) => {
                if attempt >= max_retries {
                    warn!("プライマリ({})が{}回失敗。フォールバック試行: {}",
                        primary_model, max_retries, e);
                    break;
                }
                let wait = 60 * (attempt + 1) as u64;
                warn!("[{}] リトライ {}/{}: {} - {}秒待機",
                    primary_model, attempt + 1, max_retries, e, wait);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            }
        }
    }

    match utils::request_gemini_api(
        key.to_string(), fallback_model.to_string(),
        prompt, Some(system_instruction),
    ).await {
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
