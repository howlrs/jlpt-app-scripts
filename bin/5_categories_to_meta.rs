use std::{collections::HashMap, env};

use log::{error, info};

mod utils;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let output_dir = "output";
    let target_dir = "questions";
    let target_levels = ["n1", "n2", "n3", "n4", "n5"];
    let target_filename = "4_leveling_data.json";

    // 出力ファイル -> レベル統一したカテゴリカルデータ
    let is_output = true;
    let output_file = "5_categories_meta.json";

    let mut vec_catvalue = Vec::new();

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

        // read file to string
        let content = crate::utils::read_file(target_filepath);
        let questions = serde_json::from_str::<Vec<crate::utils::Question>>(&content).unwrap();

        // ユニークなカテゴリを取得
        let mut cat_hash = HashMap::new();
        for q in &questions {
            cat_hash.insert(
                format!("{}-{}", q.level_id, q.category_id.as_ref().unwrap()),
                crate::utils::CatValue {
                    level_id: q.level_id,
                    id: q.category_id.as_ref().unwrap().parse().unwrap(),
                    name: q.category_name.clone(),
                },
            );
        }

        for (_, value) in cat_hash {
            vec_catvalue.push(value);
        }
    }

    info!("categories meta: {:?}", vec_catvalue);

    if is_output {
        // レベル統一
        let to_json_str = serde_json::to_string_pretty(&vec_catvalue).unwrap();
        let target_dir = env::current_dir()
            .unwrap()
            .join(output_dir)
            .join(target_dir);
        let output_filepath = target_dir.join(output_file);
        crate::utils::write_file(output_filepath, to_json_str.as_str());
    }

    info!("done");
}
