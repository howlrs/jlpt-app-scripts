use std::env;

use log::{error, info};

mod utils;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let output_dir = "output";
    let target_dir = "questions";
    let target_levels = ["n2", "n3"];
    let target_filename = "4_leveling_data.json";

    // データベース登録関数はTであるため、読み込む型に応じた変数を用意する
    let mut values = Vec::<crate::utils::Question>::new();

    // 出力の可否
    let is_output = true;
    let collection_name = "questions";

    // レベルごとの実行
    // 対象ディレクトリを指定し、ファイルを読み込む
    for level in target_levels {
        let target_level_dir = {
            let current_dir = env::current_dir().unwrap();
            current_dir.join(output_dir).join(target_dir).join(level)
        };

        let target_filepath = target_level_dir.join(target_filename);
        if !target_filepath.exists() {
            error!("ファイルが存在しません: {:?}", target_filepath);
            continue;
        }

        let content = crate::utils::read_file(target_filepath.clone());
        values = match serde_json::from_str(&content) {
            Ok(values) => values,
            Err(e) => {
                error!("ファイルが存在しません: {:?}, {:?}", target_filepath, e);
                continue;
            }
        };

        let err_values = crate::utils::to_database(is_output, collection_name, values).await;

        if !err_values.is_empty() {
            info!(
                "エラーが発生しました、保存できなかったQをerr.jsonに保存します: {:?}",
                err_values.len()
            );
            let to_json = serde_json::to_string_pretty(&err_values).unwrap();
            let output_filepath = target_level_dir.join("save_err_to_db.json");
            crate::utils::write_file(output_filepath, to_json.as_str());
        }
    }
    info!("done");
}
