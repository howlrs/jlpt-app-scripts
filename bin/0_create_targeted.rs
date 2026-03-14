/// ポジショニングマップ方式: ギャップカテゴリのみターゲット生成
///
/// 既存パイプライン出力 (4_leveled.json) を分析し、
/// 目標に達していないカテゴリのみ集中的に問題生成する。
///
/// 環境変数:
///   TARGET_MIN=100       各カテゴリの最小子問題数（デフォルト: 100）
///   REQUEST_INTERVAL=10  APIリクエスト間隔秒（デフォルト: 10）
///   BUFFER_RATIO=1.5     dedup/validation による減衰を見込んだバッファ倍率
use std::{collections::HashMap, env, time::Instant};

use log::{error, info, warn};

mod utils;

struct Gap {
    level: &'static str,
    cat_id: u32,
    cat_name: &'static str,
    current: u32,
    deficit: u32,
    samples: Vec<String>,
}

/// 各レベルの標準カテゴリ定義
const STANDARD_CATS: &[(&str, &[(u32, &str)])] = &[
    ("n1", &[
        (2, "漢字読み"), (3, "表記"), (4, "語形成"), (5, "文脈規定"),
        (6, "言い換え類義"), (7, "用法"),
        (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
        (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "統合理解"),
        (14, "主張理解 (長文)"), (15, "情報検索"),
        (17, "課題理解"), (18, "ポイント理解"), (19, "概要理解"), (20, "即時応答"), (22, "発話表現"),
    ]),
    ("n2", &[
        (2, "漢字読み"), (3, "表記"), (4, "語形成"), (5, "文脈規定"),
        (6, "言い換え類義"), (7, "用法"),
        (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
        (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "統合理解"),
        (14, "主張理解 (長文)"), (15, "情報検索"),
        (17, "課題理解"), (18, "ポイント理解"), (19, "概要理解"), (20, "即時応答"),
    ]),
    ("n3", &[
        (2, "漢字読み"), (3, "表記"), (4, "文脈規定"), (5, "言い換え類義"), (6, "用法"),
        (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
        (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "内容理解 (長文)"), (14, "情報検索"),
        (16, "課題理解"), (17, "ポイント理解"), (18, "概要理解"), (19, "発話表現"), (20, "即時応答"),
    ]),
    ("n4", &[
        (2, "漢字読み"), (3, "表記"), (4, "文脈規定"), (5, "言い換え類義"), (6, "用法"),
        (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
        (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "情報検索"),
        (15, "課題理解"), (16, "ポイント理解"), (17, "概要理解"), (18, "発話表現"), (19, "即時応答"),
    ]),
    ("n5", &[
        (2, "漢字読み"), (3, "表記"), (4, "文脈規定"), (5, "言い換え類義"), (6, "用法"),
        (8, "文の文法1 (文法形式の判断)"), (9, "文の文法2 (文の組み立て)"), (10, "文章の文法"),
        (11, "内容理解 (短文)"), (12, "内容理解 (中文)"), (13, "情報検索"),
        (15, "課題理解"), (16, "ポイント理解"), (17, "概要理解"), (18, "発話表現"), (19, "即時応答"),
    ]),
];

fn identify_gaps(target_min: u32) -> Vec<Gap> {
    let mut gaps = Vec::new();

    for &(level, cats) in STANDARD_CATS {
        let questions = match utils::read_questions_from_stage(level, utils::STAGE_4_OUTPUT) {
            Ok(q) => q,
            Err(e) => {
                warn!("[{}] パイプライン出力読込失敗: {}", level, e);
                continue;
            }
        };

        // カテゴリ別の子問題数を集計
        let mut counts: HashMap<u32, u32> = HashMap::new();
        for q in &questions {
            let cid: u32 = q.category_id.as_ref()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            *counts.entry(cid).or_insert(0) += q.sub_questions.len() as u32;
        }

        for &(cat_id, cat_name) in cats {
            let current = counts.get(&cat_id).copied().unwrap_or(0);
            if current < target_min {
                // サンプルも同じデータから取得（二重読込を回避）
                let samples: Vec<String> = questions.iter()
                    .filter(|q| {
                        q.category_id.as_ref()
                            .and_then(|s| s.parse::<u32>().ok())
                            == Some(cat_id)
                    })
                    .flat_map(|q| {
                        q.sub_questions.iter()
                            .filter_map(|sq| sq.sentence.clone())
                            .take(2)
                    })
                    .take(10)
                    .collect();

                gaps.push(Gap {
                    level,
                    cat_id,
                    cat_name,
                    current,
                    deficit: target_min - current,
                    samples,
                });
            }
        }
    }

    gaps
}

/// 文章の文法（cat_10）用テーマバリエーション
/// dedupで弾かれにくいよう、テーマ・文法ポイント・文体を細かく指定
const CAT_10_THEMES: &[&str] = &[
    r#"
**テーマ: 接続詞・逆接表現**
- 文章ジャンル: 新聞社説、コラム
- 空欄に入るもの: 逆接の接続詞（しかし/ところが/にもかかわらず/とはいえ/それにしても）
- 文章の論点が転換する箇所に空欄を設置
- 前後の論理関係から適切な逆接表現を選ばせる"#,
    r#"
**テーマ: 順接・因果関係の接続表現**
- 文章ジャンル: 学術論文の抜粋、研究報告
- 空欄に入るもの: 因果の接続表現（したがって/それゆえ/そのため/その結果/こうして）
- 原因→結果の流れの中に空欄を設置
- 論理の必然性を理解しないと解けない構成にする"#,
    r#"
**テーマ: 指示語・照応表現**
- 文章ジャンル: エッセイ、随筆
- 空欄に入るもの: 指示語（これ/それ/あれ/この/その/こうした/そうした/このような）
- 指示語が何を指しているか、前後の文脈から判断させる
- 近称・中称の使い分けがポイントとなる問題にする"#,
    r#"
**テーマ: 文末表現・モダリティ**
- 文章ジャンル: ビジネスメール、公式通知文
- 空欄に入るもの: 文末表現（〜ざるを得ない/〜かねない/〜に違いない/〜べきである/〜わけではない）
- 書き手の判断・態度を表す表現の使い分け
- 丁寧体・常体の文体統一も考慮した問題にする"#,
    r#"
**テーマ: 添加・並列の接続表現**
- 文章ジャンル: 案内文、説明書、ガイドブック
- 空欄に入るもの: 添加表現（また/さらに/そのうえ/加えて/しかも/それに）
- 情報が積み重なる文脈での適切な接続詞選択
- 類似表現の微妙なニュアンス差を問う"#,
    r#"
**テーマ: 条件・仮定の表現**
- 文章ジャンル: 法律文、規約、契約書の抜粋
- 空欄に入るもの: 条件表現（〜場合/〜限り/〜としても/〜ない限り/〜次第で）
- 条件節と帰結節の論理的整合性を問う
- フォーマルな書き言葉特有の条件表現を使用"#,
    r#"
**テーマ: 譲歩・対比の表現**
- 文章ジャンル: 討論記事、書評、比較レポート
- 空欄に入るもの: 譲歩表現（〜ものの/〜とはいうものの/〜にせよ/一方で/他方）
- 対立する二つの立場を提示し、譲歩の構文を問う
- 「確かに〜、しかし〜」型の論理展開を活用"#,
];

fn category_hints(cat_name: &str, theme_index: Option<usize>) -> &'static str {
    match cat_name {
        "文章の文法" => {
            match theme_index {
                Some(idx) => CAT_10_THEMES[idx % CAT_10_THEMES.len()],
                None => CAT_10_THEMES[0],
            }
        }
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
    }
}

fn theme_count_for(cat_name: &str) -> usize {
    match cat_name {
        "文章の文法" => CAT_10_THEMES.len(),
        _ => 1,
    }
}

/// カテゴリ特化の高品質プロンプトを構築
fn build_targeted_prompt(level_upper: &str, level_lower: &str, cat_name: &str, cat_id: u32, anti_dup: &str, theme_index: usize) -> String {
    let hints = category_hints(cat_name, Some(theme_index));
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
    "category_id": "{}",
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
        hints,
        anti_dup,
        level_lower, cat_id, cat_name,
        level_upper,
        seed,
    )
}

/// サンプル文からanti-duplication文字列を事前構築
fn build_anti_dup(samples: &[String]) -> String {
    if samples.is_empty() {
        return String::new();
    }
    let list = samples.iter()
        .take(5)
        .map(|s| format!("- {}", s))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n\n**重要: 以下の既存問題と類似した問題は絶対に生成しないでください:**\n{}\n上記とは異なるテーマ・場面・文法ポイントを使用してください。",
        list
    )
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

    let mut total_success = 0u32;
    let mut total_fail = 0u32;

    for gap in &gaps {
        let raw_target = (gap.deficit as f64 * buffer_ratio) as u32;
        let requests_needed = (raw_target / 5).max(1);

        // ループ外でpre-compute
        let level_upper = gap.level.to_uppercase();
        let level_lower = gap.level.to_lowercase();
        let anti_dup = build_anti_dup(&gap.samples);

        info!(
            "[{}/cat_{}] {} — 不足{}問 → {}リクエスト予定",
            gap.level, gap.cat_id, gap.cat_name, gap.deficit, requests_needed
        );

        let output_dir = utils::level_dir(gap.level);
        if let Err(e) = utils::ensure_dir(&output_dir) {
            error!("[{}] 出力ディレクトリ作成失敗: {}", gap.level, e);
            continue;
        }

        let num_themes = theme_count_for(gap.cat_name);

        for i in 0..requests_needed {
            let theme_index = i as usize % num_themes;
            let prompt = build_targeted_prompt(
                &level_upper, &level_lower, gap.cat_name, gap.cat_id, &anti_dup, theme_index
            );

            let result = utils::request_with_fallback(
                &key, &primary_model, &fallback_model,
                &prompt, utils::SYSTEM_INSTRUCTION, max_retries,
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
