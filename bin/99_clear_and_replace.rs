/// 旧データ削除 → 新データ投入の一括スクリプト
///
/// 手順:
///   1. questionsコレクションの全ドキュメントを削除
///   2. categories/categories_raw/levelsコレクションの全ドキュメントを削除
///   2b. levelsコレクションにN1〜N5のレベルドキュメントを投入
///   3. パイプライン最終出力(4_leveled.json)から新データを投入
///   4. カテゴリメタデータ(5_categories_meta.json)をcategories_rawに投入
///   4b. 4_leveled.jsonからsub_question数を集計し、categoriesコレクションに投入
///
/// 実行前に `gcloud auth application-default login` が必要
///
/// 環境変数:
///   DRY_RUN=true  → 削除・投入をシミュレーション（デフォルト: true）
///   DRY_RUN=false → 実際に削除・投入を実行
use std::collections::HashMap;

use futures_util::StreamExt;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};

mod utils;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let dry_run = std::env::var("DRY_RUN")
        .unwrap_or("true".to_string())
        .to_lowercase()
        != "false";

    if dry_run {
        info!("=== DRY RUN モード（実際のDB操作は行いません）===");
        info!("実行するには DRY_RUN=false を設定してください");
    } else {
        warn!("=== 本番モード: DBデータを削除・投入します ===");
    }

    let project_id = std::env::var("PROJECT_ID").expect("PROJECT_ID must be set");
    let db = match firestore::FirestoreDb::new(&project_id).await {
        Ok(db) => db,
        Err(e) => {
            error!("Firestore接続失敗: {}", e);
            std::process::exit(1);
        }
    };

    // Step 1: 旧questionsを削除
    info!("--- Step 1: 旧questions削除 ---");
    let deleted_q = delete_collection(&db, "questions", dry_run).await;
    info!("questions: {}件削除{}", deleted_q, if dry_run { "（予定）" } else { "" });

    // Step 2: 旧categories系・levels削除
    info!("--- Step 2: 旧categories/levels削除 ---");
    let deleted_c = delete_collection(&db, "categories", dry_run).await;
    let deleted_cr = delete_collection(&db, "categories_raw", dry_run).await;
    let deleted_l = delete_collection(&db, "levels", dry_run).await;
    info!("categories: {}件, categories_raw: {}件, levels: {}件 削除{}", deleted_c, deleted_cr, deleted_l, if dry_run { "（予定）" } else { "" });

    // Step 2b: levelsコレクションにN1〜N5を投入
    info!("--- Step 2b: levels投入 ---");
    for i in 1..=5u32 {
        let level_doc = serde_json::json!({
            "id": i,
            "name": format!("N{}", i),
        });
        if !dry_run {
            match db
                .fluent()
                .insert()
                .into("levels")
                .document_id(i.to_string())
                .object(&level_doc)
                .execute::<serde_json::Value>()
                .await
            {
                Ok(_) => info!("levels/{} 投入完了", i),
                Err(e) => warn!("levels/{} 投入失敗: {}", i, e),
            }
        } else {
            info!("levels/{} 投入予定", i);
        }
    }

    // Step 3: 新questions投入
    info!("--- Step 3: 新questions投入 ---");
    let mut total_inserted = 0u32;
    for level in utils::LEVELS {
        match utils::read_questions_from_stage(level, utils::STAGE_4_OUTPUT) {
            Ok(questions) => {
                let count = questions.len();
                if !dry_run {
                    let err_values =
                        utils::to_database(true, "questions", questions).await;
                    let inserted = count - err_values.len();
                    total_inserted += inserted as u32;
                    if !err_values.is_empty() {
                        warn!("[{}] {}件の投入に失敗", level, err_values.len());
                    }
                    info!("[{}] {}件投入完了", level, inserted);
                } else {
                    total_inserted += count as u32;
                    info!("[{}] {}件投入予定", level, count);
                }
            }
            Err(e) => {
                warn!("[{}] データ読込失敗（スキップ）: {}", level, e);
            }
        }
    }
    info!("questions合計: {}件投入{}", total_inserted, if dry_run { "（予定）" } else { "" });

    // Step 4: 新categories投入
    info!("--- Step 4: 新categories投入 ---");
    let cat_path = std::env::current_dir()
        .unwrap()
        .join(utils::OUTPUT_DIR)
        .join(utils::QUESTIONS_DIR)
        .join(utils::STAGE_5_OUTPUT);

    if cat_path.exists() {
        let content = utils::read_file(cat_path);
        match serde_json::from_str::<Vec<utils::CatValue>>(&content) {
            Ok(categories) => {
                let count = categories.len();
                if !dry_run {
                    let err = utils::to_database_with_uuid(true, "categories_raw", categories).await;
                    info!("categories_raw: {}件投入完了（{}件失敗）", count - err.len(), err.len());
                } else {
                    info!("categories_raw: {}件投入予定", count);
                }
            }
            Err(e) => warn!("categories JSONパース失敗: {}", e),
        }
    } else {
        warn!("カテゴリファイル不在: {:?}（スキップ）", cat_path);
    }

    // Step 4b: categoriesコレクションにreten（問題数）付きで投入
    info!("--- Step 4b: categories（reten付き）投入 ---");
    // (level_id, category_id) → (category_name, sub_question_count)
    let mut cat_counts: HashMap<(u32, String), (String, u32)> = HashMap::new();

    for level in utils::LEVELS {
        match utils::read_questions_from_stage(level, utils::STAGE_4_OUTPUT) {
            Ok(questions) => {
                for q in &questions {
                    let cat_id = q.category_id.clone().unwrap_or_default();
                    if cat_id.is_empty() {
                        continue;
                    }
                    let sub_q_count = q.sub_questions.len() as u32;
                    let entry = cat_counts
                        .entry((q.level_id, cat_id))
                        .or_insert((q.category_name.clone(), 0));
                    entry.1 += sub_q_count;
                }
            }
            Err(e) => {
                warn!("[{}] categories集計用データ読込失敗（スキップ）: {}", level, e);
            }
        }
    }

    let mut cat_inserted = 0u32;
    for ((level_id, category_id), (category_name, reten)) in &cat_counts {
        let doc = CategoryWithReten {
            level_id: *level_id,
            id: category_id.clone(),
            name: category_name.clone(),
            reten: *reten,
        };
        let doc_id = format!("{}_{}", level_id, category_id);
        if !dry_run {
            match db
                .fluent()
                .insert()
                .into("categories")
                .document_id(&doc_id)
                .object(&doc)
                .execute::<CategoryWithReten>()
                .await
            {
                Ok(_) => {
                    cat_inserted += 1;
                    info!("categories/{} 投入完了", doc_id);
                }
                Err(e) => warn!("categories/{} 投入失敗: {}", doc_id, e),
            }
        } else {
            cat_inserted += 1;
            info!("categories/{} 投入予定 (reten={})", doc_id, reten);
        }
    }
    info!("categories: {}件投入{}", cat_inserted, if dry_run { "（予定）" } else { "" });

    info!("=== 完了 ===");
}

#[derive(Serialize, Deserialize, Debug)]
struct CategoryWithReten {
    level_id: u32,
    id: String,
    name: String,
    reten: u32,
}

async fn delete_collection(
    db: &firestore::FirestoreDb,
    collection: &str,
    dry_run: bool,
) -> u32 {
    let stream_result = db
        .fluent()
        .list()
        .from(collection)
        .obj::<serde_json::Value>()
        .stream_all_with_errors()
        .await;

    let mut stream = match stream_result {
        Ok(s) => s,
        Err(e) => {
            warn!("{}コレクション読取失敗: {}", collection, e);
            return 0;
        }
    };

    let mut count = 0u32;
    let mut doc_ids: Vec<String> = Vec::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(doc) => {
                if let Some(id) = doc.get("id").and_then(|v| v.as_str()) {
                    doc_ids.push(id.to_string());
                } else if let Some(id) = doc.get("_firestore_id").and_then(|v| v.as_str()) {
                    doc_ids.push(id.to_string());
                }
                count += 1;
            }
            Err(e) => warn!("ドキュメント読取エラー: {}", e),
        }
    }

    if !dry_run {
        for doc_id in &doc_ids {
            match db
                .fluent()
                .delete()
                .from(collection)
                .document_id(doc_id)
                .execute()
                .await
            {
                Ok(_) => {}
                Err(e) => warn!("削除失敗 {}/{}: {}", collection, doc_id, e),
            }
        }
    }

    count
}
