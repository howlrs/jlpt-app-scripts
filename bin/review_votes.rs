/// 投票データの集計・低品質問題のリスト化・削除
///
/// 機能:
///   1. votesコレクションを集計し、問題ごとのgood/bad件数を算出
///   2. bad率が閾値以上の問題をリスト化（JSONファイル出力）
///   3. --delete オプションでFirestoreから該当問題を削除
///
/// 使用方法:
///   cargo run --bin review_votes                  # 集計のみ
///   cargo run --bin review_votes -- --delete      # 集計 + 削除実行
///   BAD_THRESHOLD=0.5 cargo run --bin review_votes  # 閾値変更（デフォルト: 0.6）
use std::collections::HashMap;

use futures_util::StreamExt;
use log::{error, info, warn};

mod utils;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let delete_mode = std::env::args().any(|a| a == "--delete");
    let bad_threshold: f64 = std::env::var("BAD_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.6);

    info!(
        "=== 投票データレビュー === (delete: {}, bad閾値: {:.0}%)",
        delete_mode,
        bad_threshold * 100.0
    );

    let project_id = std::env::var("PROJECT_ID").expect("PROJECT_ID must be set");
    let db = match firestore::FirestoreDb::new(&project_id).await {
        Ok(db) => db,
        Err(e) => {
            error!("Firestore接続失敗: {}", e);
            std::process::exit(1);
        }
    };

    // Step 1: 全投票データを取得
    info!("--- Step 1: 投票データ取得 ---");
    let mut vote_stream = match db
        .fluent()
        .list()
        .from("votes")
        .obj::<serde_json::Value>()
        .stream_all_with_errors()
        .await
    {
        Ok(s) => s,
        Err(e) => {
            error!("votes取得失敗: {}", e);
            return;
        }
    };

    // parent_id → { good: N, bad: N }
    let mut vote_counts: HashMap<String, (u32, u32)> = HashMap::new();
    let mut total_votes = 0u32;

    while let Some(item) = vote_stream.next().await {
        match item {
            Ok(doc) => {
                let vote = doc
                    .get("vote")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let parent_id = doc
                    .get("parent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if parent_id.is_empty() {
                    continue;
                }

                let entry = vote_counts.entry(parent_id).or_insert((0, 0));
                match vote {
                    "good" => entry.0 += 1,
                    "bad" => entry.1 += 1,
                    _ => {}
                }
                total_votes += 1;
            }
            Err(e) => warn!("vote読取エラー: {}", e),
        }
    }

    info!(
        "投票総数: {}件, 問題数: {}件",
        total_votes,
        vote_counts.len()
    );

    // Step 2: bad率が閾値以上の問題を特定
    info!("--- Step 2: 低品質問題の特定 (bad率 >= {:.0}%) ---", bad_threshold * 100.0);
    let mut bad_questions: Vec<serde_json::Value> = Vec::new();
    let mut good_only = 0u32;
    let mut mixed = 0u32;

    for (parent_id, (good, bad)) in &vote_counts {
        let total = good + bad;
        let bad_rate = *bad as f64 / total as f64;

        if bad_rate >= bad_threshold {
            bad_questions.push(serde_json::json!({
                "question_id": parent_id,
                "good": good,
                "bad": bad,
                "bad_rate": format!("{:.0}%", bad_rate * 100.0),
            }));
        } else if *bad == 0 {
            good_only += 1;
        } else {
            mixed += 1;
        }
    }

    // bad率でソート
    bad_questions.sort_by(|a, b| {
        let a_bad = a.get("bad").and_then(|v| v.as_u64()).unwrap_or(0);
        let b_bad = b.get("bad").and_then(|v| v.as_u64()).unwrap_or(0);
        b_bad.cmp(&a_bad)
    });

    info!(
        "結果: good_only={}, mixed={}, bad(閾値以上)={}",
        good_only,
        mixed,
        bad_questions.len()
    );

    // リスト出力
    if !bad_questions.is_empty() {
        info!("--- 低品質問題リスト ---");
        for q in &bad_questions {
            info!(
                "  {} - good:{} bad:{} ({})",
                q.get("question_id").and_then(|v| v.as_str()).unwrap_or("?"),
                q.get("good").and_then(|v| v.as_u64()).unwrap_or(0),
                q.get("bad").and_then(|v| v.as_u64()).unwrap_or(0),
                q.get("bad_rate").and_then(|v| v.as_str()).unwrap_or("?"),
            );
        }

        // JSONファイルに保存
        let output_path = std::env::current_dir()
            .unwrap()
            .join("output")
            .join("bad_questions.json");
        let json = serde_json::to_string_pretty(&bad_questions).unwrap();
        std::fs::write(&output_path, &json).unwrap();
        info!("低品質問題リストを保存: {:?}", output_path);
    }

    // Step 3: 削除実行（--deleteオプション時のみ）
    if delete_mode && !bad_questions.is_empty() {
        info!(
            "--- Step 3: {}件の低品質問題を削除 ---",
            bad_questions.len()
        );
        let mut deleted = 0u32;
        for q in &bad_questions {
            let qid = q
                .get("question_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if qid.is_empty() {
                continue;
            }

            match db
                .fluent()
                .delete()
                .from("questions")
                .document_id(qid)
                .execute()
                .await
            {
                Ok(_) => {
                    deleted += 1;
                    info!("  削除: {}", qid);
                }
                Err(e) => warn!("  削除失敗 {}: {}", qid, e),
            }
        }
        info!("{}件削除完了", deleted);
    } else if delete_mode {
        info!("削除対象の問題はありません");
    }

    info!("=== 完了 ===");
}
