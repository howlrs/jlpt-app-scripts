use std::env;

use log::{error, info};

mod utils;
use crate::utils::{read_file, walk_dir, write_file};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let output_dir = "output";
    let target_dir = "questions";
    let target_levels = ["n3", "n2"];

    // 出力先ファイル
    let new_file = "concat_all.md";
    let is_output = false;

    // レベルごとの実行
    // 対象ディレクトリを指定し、ファイルを読み込む
    for level in target_levels {
        let target_level_dir = {
            let current_dir = env::current_dir().unwrap();
            current_dir.join(output_dir).join(target_dir).join(level)
        };

        // ディレクトリ内のファイルを走査
        // ファイル内容を連結
        let mut concat_content = String::new();
        for target_file in walk_dir(&target_level_dir) {
            let read_content = read_file(target_file);
            concat_content.push_str(&read_content);
        }

        if concat_content.is_empty() {
            error!("ファイルデータが存在しません: {:?}", target_level_dir);
            continue;
        }

        if is_output {
            // save new file
            let new_file_path = target_level_dir.join(new_file);
            write_file(new_file_path, &concat_content);
        }
    }
    info!("done");
}
