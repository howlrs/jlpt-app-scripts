use log::{error, info};

mod utils;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let collection_name = "questions";
    let is_output = true;

    for level in utils::LEVELS {
        let questions = match utils::read_questions_from_stage(level, utils::STAGE_4_OUTPUT) {
            Ok(q) => q,
            Err(e) => {
                error!("[{}] 読込失敗: {}", level, e);
                continue;
            }
        };

        info!("[{}] {}件をFirestoreに投入", level, questions.len());

        let err_values = utils::to_database(is_output, collection_name, questions).await;

        if !err_values.is_empty() {
            info!("[{}] {}件の投入に失敗 - err/save_err_to_db.jsonに保存", level, err_values.len());
            let err_dir = utils::level_dir(level).join("err");
            let _ = utils::ensure_dir(&err_dir);
            let to_json = serde_json::to_string_pretty(&err_values).unwrap();
            utils::write_file(err_dir.join("save_err_to_db.json"), &to_json);
        }
    }

    info!("done");
}
