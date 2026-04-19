//! 本番 Firestore の `questions.level_name` が "N5"/"n5" のように大小文字混在しているため、
//! すべて大文字 ("N1" 〜 "N5") に統一する one-shot スクリプト。
//!
//! --dry-run (デフォルト): 対象件数のみを報告、書き込みなし
//! --execute: 実際に Firestore を更新
//!
//! ## Idempotency
//! 再実行しても安全。`level_name.to_uppercase() != level_name` の差分のみ処理するため、
//! 途中で中断された場合は再度 `--execute` で残り分を処理できる。

use futures_util::StreamExt;
use log::{error, info, warn};
use std::env;

mod utils;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let execute = env::args().any(|a| a == "--execute");
    let mode = if execute { "EXECUTE" } else { "DRY-RUN" };
    info!("=== 99_normalize_levels [{}] ===", mode);

    let project_id = match env::var("PROJECT_ID") {
        Ok(id) => id,
        Err(_) => { error!("PROJECT_ID must be set"); return; }
    };
    let db = match firestore::FirestoreDb::new(&project_id).await {
        Ok(d) => d,
        Err(e) => {
            error!("FirestoreDb::new failed: {:?}", e);
            return;
        }
    };

    // 全 questions を取得 (doc 全体を保持)
    let stream_result = db
        .fluent()
        .list()
        .from("questions")
        .obj::<serde_json::Value>()
        .stream_all_with_errors()
        .await;

    let mut stream = match stream_result {
        Ok(s) => s,
        Err(e) => {
            error!("stream_all_with_errors failed: {:?}", e);
            return;
        }
    };

    let mut total = 0usize;
    let mut needs_update: Vec<(String, String, String, serde_json::Value)> = Vec::new();
    // (id, old_level, new_level, full_doc)

    let mut stream_errors = 0usize;
    while let Some(item) = stream.next().await {
        let doc = match item {
            Ok(d) => d,
            Err(e) => {
                warn!("stream item error: {:?}", e);
                stream_errors += 1;
                continue;
            }
        };
        total += 1;
        let id = doc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let old_level = doc.get("level_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let new_level = old_level.to_uppercase();
        if old_level != new_level && !id.is_empty() {
            needs_update.push((id, old_level, new_level, doc));
        }
        if total % 2000 == 0 {
            info!("scanned {}", total);
        }
    }

    info!("total docs: {}, needs update: {}, stream_errors: {}", total, needs_update.len(), stream_errors);

    // 変更内訳を level_name 別にカウント
    let mut breakdown: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (_, old, _, _) in &needs_update {
        *breakdown.entry(old.clone()).or_insert(0) += 1;
    }
    let mut sorted: Vec<_> = breakdown.into_iter().collect();
    sorted.sort();
    for (old, count) in sorted {
        info!("  {} → {}: {}件", old, old.to_uppercase(), count);
    }

    if !execute {
        info!("(dry-run: 変更は適用されません。--execute で実行)");
        return;
    }

    // 実行: 既存 backend パターン (full object replace) を踏襲
    info!("実行中...");
    let mut ok = 0usize;
    let mut err = 0usize;
    use std::time::Duration;
    for (id, _old, new_level, doc) in &needs_update {
        let mut new_doc = doc.clone();
        if let Some(obj) = new_doc.as_object_mut() {
            obj.insert("level_name".to_string(), serde_json::Value::String(new_level.clone()));
        }
        match db
            .fluent()
            .update()
            .in_col("questions")
            .document_id(id)
            .object(&new_doc)
            .execute::<serde_json::Value>()
            .await
        {
            Ok(_) => ok += 1,
            Err(e) => {
                let msg = format!("{:?}", e);
                if msg.contains("RESOURCE_EXHAUSTED") || msg.contains("code: 8") {
                    warn!("rate limited on id={}, sleeping 1s then continuing", id);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                error!("update failed id={}: {}", id, msg);
                err += 1;
            }
        }
        // 緩やかなレート制限 (5ms sleep ≒ 200 writes/sec 上限、Firestoreの500/sec制限を余裕で下回る)
        tokio::time::sleep(Duration::from_millis(5)).await;
        if (ok + err) % 250 == 0 {
            info!("進捗: {} / {}", ok + err, needs_update.len());
        }
    }
    info!("done: ok={}, err={}", ok, err);
}
