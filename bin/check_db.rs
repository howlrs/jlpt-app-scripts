use log::info;
use futures_util::StreamExt;

mod utils;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let project_id = std::env::var("PROJECT_ID").expect("PROJECT_ID must be set");
    let db = firestore::FirestoreDb::new(&project_id).await.unwrap();

    info!("=== Firestore データ確認 ===");

    // questionsコレクションをレベル別に集計
    for level_id in 1..=5u32 {
        let stream_result = db
            .fluent()
            .select()
            .from("questions")
            .filter(|q| {
                q.field(firestore::path!(utils::Question::level_id)).eq(level_id)
            })
            .obj::<serde_json::Value>()
            .stream_query_with_errors()
            .await;

        match stream_result {
            Ok(mut stream) => {
                let mut count = 0u32;
                let mut sample: Option<serde_json::Value> = None;
                while let Some(item) = stream.next().await {
                    if let Ok(doc) = item {
                        count += 1;
                        if sample.is_none() {
                            sample = Some(doc);
                        }
                    }
                }
                info!("N{}: {}件", level_id, count);
                if let Some(doc) = sample {
                    if let Some(obj) = doc.as_object() {
                        let fields: Vec<&String> = obj.keys().collect();
                        info!("  fields: {:?}", fields);
                        // sub_questionsの最初のサンプル
                        if let Some(sqs) = obj.get("sub_questions").and_then(|v| v.as_array()) {
                            if let Some(first_sq) = sqs.first() {
                                if let Some(sq_obj) = first_sq.as_object() {
                                    let sq_fields: Vec<&String> = sq_obj.keys().collect();
                                    info!("  sub_question fields: {:?}", sq_fields);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => info!("N{}: クエリエラー: {}", level_id, e),
        }
    }

    // 他コレクション
    for collection in ["levels", "categories", "categories_raw", "votes", "users"] {
        let stream_result = db
            .fluent()
            .list()
            .from(collection)
            .obj::<serde_json::Value>()
            .stream_all()
            .await;

        match stream_result {
            Ok(mut stream) => {
                let mut count = 0u32;
                while let Some(_) = stream.next().await {
                    count += 1;
                }
                info!("{}: {}件", collection, count);
            }
            Err(e) => info!("{}: エラー: {}", collection, e),
        }
    }
}
