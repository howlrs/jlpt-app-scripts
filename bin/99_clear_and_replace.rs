/// 旧データ削除 → 新データ投入の一括スクリプト
///
/// 手順:
///   1. questionsコレクションの全ドキュメントを削除
///   2. categories/categories_rawコレクションの全ドキュメントを削除
///   3. パイプライン最終出力(4_leveled.json)から新データを投入
///   4. カテゴリメタデータ(5_categories_meta.json)を投入
///
/// 実行前に `gcloud auth application-default login` が必要
///
/// 環境変数:
///   DRY_RUN=true  → 削除・投入をシミュレーション（デフォルト: true）
///   DRY_RUN=false → 実際に削除・投入を実行
use futures_util::StreamExt;
use log::{error, info, warn};

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

    // Step 2: 旧categories系を削除
    info!("--- Step 2: 旧categories削除 ---");
    let deleted_c = delete_collection(&db, "categories", dry_run).await;
    let deleted_cr = delete_collection(&db, "categories_raw", dry_run).await;
    info!("categories: {}件, categories_raw: {}件 削除{}", deleted_c, deleted_cr, if dry_run { "（予定）" } else { "" });

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

    info!("=== 完了 ===");
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
