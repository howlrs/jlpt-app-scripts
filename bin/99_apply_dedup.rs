//! `reports/duplicates.json` を入力として、重複 sub_question を Firestore から除去する。
//!
//! --dry-run (デフォルト): 操作ログのみ出力
//! --execute: 実際に Firestore を更新
//!
//! ## Idempotency
//! 再実行しても安全。report 内の sub_idx が既に削除済みの場合は skip する。
//! 中断された場合は再 `--execute` で残り分を処理できる。
//!
//! ## ロジック
//!   1. report.groups から (parent_id, sub_idx) の削除集合を構築
//!   2. 対象 parent_id ごとに、Firestore から現行 Question を取得
//!   3. sub_questions から該当 sub を除外した配列で update
//!   4. sub_questions が空なら delete
//!   5. 件数サマリ出力

use log::{error, info, warn};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::time::Duration;

mod utils;

#[derive(Deserialize, Debug)]
struct ReportRow {
    parent_id: String,
    sub_idx: usize,
    #[serde(default)]
    #[allow(dead_code)]
    sentence: String,
}

#[derive(Deserialize, Debug)]
struct ReportGroup {
    #[serde(default)]
    #[allow(dead_code)]
    dedup_key: String,
    #[allow(dead_code)]
    keep: ReportRow,
    remove: Vec<ReportRow>,
}

#[derive(Deserialize, Debug)]
struct Report {
    removable_subs: usize,
    groups: Vec<ReportGroup>,
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let execute = env::args().any(|a| a == "--execute");
    let mode = if execute { "EXECUTE" } else { "DRY-RUN" };
    info!("=== 99_apply_dedup [{}] ===", mode);

    let report_path = env::args().skip(1).find(|a| !a.starts_with("--"))
        .unwrap_or_else(|| "reports/duplicates.json".to_string());
    info!("レポート読込: {}", report_path);

    let content = match fs::read_to_string(&report_path) {
        Ok(c) => c,
        Err(e) => {
            error!("レポート読込失敗: {:?}", e);
            return;
        }
    };
    let report: Report = match serde_json::from_str(&content) {
        Ok(r) => r,
        Err(e) => {
            error!("レポートJSON parse失敗: {:?}", e);
            return;
        }
    };
    info!("removable_subs (期待値): {}", report.removable_subs);

    // parent_id -> Vec<sub_idx to remove>
    let mut remove_map: HashMap<String, Vec<usize>> = HashMap::new();
    for g in &report.groups {
        for r in &g.remove {
            remove_map.entry(r.parent_id.clone()).or_insert_with(Vec::new).push(r.sub_idx);
        }
    }
    info!("対象 parent 数: {}", remove_map.len());

    let project_id = match env::var("PROJECT_ID") {
        Ok(id) => id,
        Err(_) => { error!("PROJECT_ID must be set"); return; }
    };
    let db = match firestore::FirestoreDb::new(&project_id).await {
        Ok(d) => d,
        Err(e) => { error!("FirestoreDb::new failed: {:?}", e); return; }
    };

    let mut updated = 0usize;
    let mut parents_deleted = 0usize;
    let mut subs_removed = 0usize;
    let mut sub_already_gone = 0usize;
    let mut errors: Vec<(String, String)> = Vec::new();
    let total_parents = remove_map.len();
    let mut processed = 0usize;

    for (parent_id, sub_indices_to_remove) in &remove_map {
        processed += 1;

        // 現行 Question を取得
        let doc = match db.fluent()
            .select()
            .by_id_in("questions")
            .obj::<serde_json::Value>()
            .one(parent_id)
            .await
        {
            Ok(Some(d)) => d,
            Ok(None) => {
                warn!("parent not found: {}", parent_id);
                continue;
            }
            Err(e) => {
                let msg = format!("{:?}", e);
                if msg.contains("RESOURCE_EXHAUSTED") || msg.contains("code: 8") {
                    warn!("rate limited on fetch {}, sleeping 1s", parent_id);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                error!("fetch failed {}: {}", parent_id, msg);
                errors.push((parent_id.clone(), msg));
                continue;
            }
        };

        // sub_questions を抽出
        let subs = doc.get("sub_questions").and_then(|v| v.as_array()).cloned().unwrap_or_default();

        // 残す sub を構築 (sub_idx を除外)
        let remove_set: std::collections::HashSet<usize> = sub_indices_to_remove.iter().cloned().collect();
        let kept: Vec<serde_json::Value> = subs.iter().enumerate()
            .filter(|(i, _)| !remove_set.contains(i))
            .map(|(_, s)| s.clone())
            .collect();

        let removed_count = subs.len() - kept.len();
        if removed_count == 0 {
            sub_already_gone += sub_indices_to_remove.len();
            warn!("parent {}: 対象 sub_idx がすでに存在しない (report 過時か冪等再実行)", parent_id);
            continue;
        }
        subs_removed += removed_count;

        if !execute {
            if kept.is_empty() {
                info!("[dry-run] would DELETE parent={} (all {} subs)", parent_id, subs.len());
            } else {
                info!("[dry-run] would UPDATE parent={} ({} → {} subs)", parent_id, subs.len(), kept.len());
            }
            continue;
        }

        // 実行
        if kept.is_empty() {
            match db.fluent().delete().from("questions").document_id(parent_id).execute().await {
                Ok(_) => { parents_deleted += 1; }
                Err(e) => {
                    let msg = format!("{:?}", e);
                    if msg.contains("RESOURCE_EXHAUSTED") || msg.contains("code: 8") {
                        warn!("rate limited on delete {}, sleeping 1s", parent_id);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    error!("delete {}: {}", parent_id, msg);
                    errors.push((parent_id.clone(), msg));
                }
            }
        } else {
            // 現行ドキュメント全体を clone し、sub_questions のみ置き換え、既存 backend パターン (full object replace)
            let mut new_doc = doc.clone();
            if let Some(obj) = new_doc.as_object_mut() {
                obj.insert("sub_questions".to_string(), serde_json::Value::Array(kept));
            }
            match db.fluent()
                .update()
                .in_col("questions")
                .document_id(parent_id)
                .object(&new_doc)
                .execute::<serde_json::Value>()
                .await
            {
                Ok(_) => { updated += 1; }
                Err(e) => {
                    let msg = format!("{:?}", e);
                    if msg.contains("RESOURCE_EXHAUSTED") || msg.contains("code: 8") {
                        warn!("rate limited on update {}, sleeping 1s", parent_id);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    error!("update {}: {}", parent_id, msg);
                    errors.push((parent_id.clone(), msg));
                }
            }
        }

        // 緩やかなレート制限
        tokio::time::sleep(Duration::from_millis(5)).await;

        if processed % 50 == 0 {
            info!("進捗: {} / {} (updated={}, deleted={}, subs_removed={})",
                processed, total_parents, updated, parents_deleted, subs_removed);
        }
    }

    info!("=== Summary ===");
    info!("processed parents:     {}", processed);
    info!("subs_removed:          {}", subs_removed);
    info!("updated parents:       {}", updated);
    info!("deleted parents:       {}", parents_deleted);
    info!("sub_already_gone:      {}", sub_already_gone);
    info!("errors:                {}", errors.len());

    if !errors.is_empty() {
        let err_path = "reports/apply_dedup_errors.json";
        match serde_json::to_string_pretty(&errors) {
            Ok(s) => {
                if let Err(e) = fs::write(err_path, s) {
                    error!("error log write failed: {:?}", e);
                } else {
                    warn!("エラー詳細: {}", err_path);
                }
            }
            Err(e) => error!("error log serialize failed: {:?}", e),
        }
    }

    if !execute {
        info!("(dry-run: 変更は適用されていません。--execute で実行)");
    }
}
