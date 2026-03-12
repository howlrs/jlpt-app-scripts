use log::{error, info};

mod utils;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let collection_name = "categories_raw";
    let is_output = true;

    let questions_dir = std::env::current_dir()
        .unwrap()
        .join(utils::OUTPUT_DIR)
        .join(utils::QUESTIONS_DIR);
    let filepath = questions_dir.join(utils::STAGE_5_OUTPUT);

    if !filepath.exists() {
        error!("ファイルが存在しません: {:?}", filepath);
        return;
    }

    let content = utils::read_file(filepath.clone());
    let values: Vec<utils::CatValue> = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            error!("JSONパース失敗: {:?} - {}", filepath, e);
            return;
        }
    };

    info!("{}件のカテゴリをFirestoreに投入", values.len());

    let err_values = utils::to_database_with_uuid(is_output, collection_name, values).await;

    if !err_values.is_empty() {
        info!("{}件の投入に失敗 - err/save_err_to_db.jsonに保存", err_values.len());
        let err_dir = questions_dir.join("err");
        let _ = utils::ensure_dir(&err_dir);
        let to_json = serde_json::to_string_pretty(&err_values).unwrap();
        utils::write_file(err_dir.join("save_err_to_db_categories.json"), &to_json);
    }

    info!("done");
}
