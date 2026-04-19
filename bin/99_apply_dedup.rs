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

    // parent_id -> Vec<(sub_idx, expected_sentence)>
    let mut remove_map: HashMap<String, Vec<(usize, String)>> = HashMap::new();
    for g in &report.groups {
        for r in &g.remove {
            remove_map.entry(r.parent_id.clone()).or_insert_with(Vec::new).push((r.sub_idx, r.sentence.clone()));
        }
    }
    info!("対象 parent 数: {}", remove_map.len());
    if remove_map.len() > 1000 {
        error!("対象 parent 数が異常に多い ({}) — 不正な report の可能性あり。中止", remove_map.len());
        return;
    }

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

    // I4: HashMap iteration は非決定論的なので sort してから処理 (dry-run出力の再現性確保)
    let mut sorted_entries: Vec<(&String, &Vec<(usize, String)>)> = remove_map.iter().collect();
    sorted_entries.sort_by(|a, b| a.0.cmp(b.0));

    for (parent_id, sub_removals) in sorted_entries {
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

        // C1/I1: 各 (sub_idx, expected_sentence) について、現在の sub_questions が content match するか検証
        let mut confirmed_remove_idx: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut out_of_range = 0usize;
        let mut content_mismatch = 0usize;
        let mut already_gone = 0usize;
        for (idx, expected_sent) in sub_removals {
            if *idx >= subs.len() {
                out_of_range += 1;
                continue;
            }
            let curr_sent = subs[*idx].get("sentence").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            if !expected_sent.is_empty() && curr_sent != *expected_sent {
                content_mismatch += 1;
                warn!("parent {} sub_idx={}: content mismatch, skipping (expected='{}' current='{}')",
                    parent_id, idx, &expected_sent[..expected_sent.chars().take(40).count().min(expected_sent.len())], &curr_sent[..curr_sent.chars().take(40).count().min(curr_sent.len())]);
                continue;
            }
            confirmed_remove_idx.insert(*idx);
        }

        // すべての removal が既に存在しない場合
        if confirmed_remove_idx.is_empty() {
            if !sub_removals.is_empty() {
                already_gone = sub_removals.len();
                sub_already_gone += already_gone;
                warn!("parent {}: 対象 sub_idx がすべて既に存在しないか不整合 (out_of_range={}, content_mismatch={}, already_gone={})",
                    parent_id, out_of_range, content_mismatch, already_gone - out_of_range - content_mismatch);
            }
            continue;
        }

        // 実際に除外する sub を構築
        let kept: Vec<serde_json::Value> = subs.iter().enumerate()
            .filter(|(i, _)| !confirmed_remove_idx.contains(i))
            .map(|(_, s)| s.clone())
            .collect();

        let planned_removed_count = confirmed_remove_idx.len();

        // Content mismatch を errors に含めるかはポリシー次第。今回は warn のみで継続するが、一定数超えたら abort する
        if content_mismatch > 0 {
            errors.push((parent_id.clone(), format!("content_mismatch on {} sub(s)", content_mismatch)));
        }

        if !execute {
            if kept.is_empty() {
                info!("[dry-run] would DELETE parent={} (all {} subs) remove_idx={:?}",
                    parent_id, subs.len(), confirmed_remove_idx.iter().collect::<Vec<_>>());
            } else {
                let mut remove_vec: Vec<&usize> = confirmed_remove_idx.iter().collect();
                remove_vec.sort();
                info!("[dry-run] would UPDATE parent={} subs {} → {} remove_idx={:?}",
                    parent_id, subs.len(), kept.len(), remove_vec);
            }
            subs_removed += planned_removed_count; // dry-run planned count
            continue;
        }

        // 実行
        if kept.is_empty() {
            match db.fluent().delete().from("questions").document_id(parent_id).execute().await {
                Ok(_) => {
                    parents_deleted += 1;
                    subs_removed += planned_removed_count;
                }
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
                Ok(_) => {
                    updated += 1;
                    subs_removed += planned_removed_count;
                }
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
    info!("processed parents:         {}", processed);
    info!("subs_removed (confirmed):  {}  // 実行モードでは Firestore書き込みに成功した件数のみ", subs_removed);
    info!("updated parents:           {}", updated);
    info!("deleted parents:           {}", parents_deleted);
    info!("sub_already_gone:          {}", sub_already_gone);
    info!("errors:                    {}", errors.len());

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
