use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use log::{error, info};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Question {
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub level_id: u32,
    pub level_name: String,
    #[serde(default)]
    pub category_id: u32,
    pub category_name: String,

    #[serde(default)]
    pub chapter: String,
    pub sentence: String,
    #[serde(default)]
    pub prerequisites: Option<String>,
    pub sub_questions: Vec<SubQuestion>,
}

type SelectAnswer = HashMap<String, String>;

#[derive(Serialize, Deserialize, Debug)]
pub struct SubQuestion {
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub hint_id: u32,
    #[serde(default)]
    pub answer_id: u32,

    pub sentence: String,
    #[serde(default)]
    pub prerequisites: Option<String>,
    pub select_answer: Vec<SelectAnswer>,
    pub answer: String,
}

/// 対象のディレクトリ走査しファイルを読み込む
/// ファイルを文字列として読み込み、JSONをパースする
/// パースに成功した場合は、Question構造体に変換する
/// パースに失敗した場合は、エラーを出力する
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 処理時間の計測
    let start = std::time::Instant::now();

    let output_dir = "output";
    let target_dir = "questions";
    let target_levels = ["n2", "n3"];
    // let target_file = "concat_all.json";

    // 出力先ファイル
    let new_file = "concat_with_struct.json";
    let is_output = false;

    // レベルごとの実行
    // 対象ディレクトリを指定し、ファイルを読み込む
    for level in target_levels {
        let target_level_dir = {
            let current_dir = env::current_dir().unwrap();
            current_dir.join(output_dir).join(target_dir).join(level)
        };

        let mut all_questions = vec![];
        for file in walk_dir(&target_level_dir).into_iter() {
            let read_content = read_file(file.clone());
            match serde_json::from_str::<Vec<Question>>(&read_content) {
                Ok(questions) => {
                    all_questions.extend(questions);
                }
                Err(e) => {
                    panic!("JSONのパースに失敗しました: {:?},  {}", file, e);
                }
            };
        }
        if all_questions.is_empty() {
            error!("ファイルデータが存在しません: {:?}", target_level_dir);
            continue;
        }

        // 2配列を出力
        info!(
            "length: {}, inner subq len: {}",
            all_questions.len(),
            all_questions
                .iter()
                .fold(0, |acc, q| acc + q.sub_questions.len())
        );

        // save new file
        if is_output {
            let new_file_path = target_level_dir.join(new_file);
            let new_content = serde_json::to_string_pretty(&all_questions).unwrap();
            write_file(new_file_path, &new_content);
        }
    }
    info!("done, elapsed: {:?}", start.elapsed());
}

#[allow(unused)]
// 指定ディレクトリのファイルを走査する
fn walk_dir(dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![];
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            continue;
        } else {
            files.push(path);
        }
    }
    files
}

fn read_file(abs_filename: PathBuf) -> String {
    std::fs::read_to_string(abs_filename).unwrap_or_else(|e| {
        panic!("ファイルの読み込みに失敗しました: {}", e);
    })
}

#[allow(unused)]
fn write_file(abs_filename: PathBuf, content: &str) {
    std::fs::write(abs_filename, content).unwrap_or_else(|e| {
        panic!("ファイルの書き込みに失敗しました: {}", e);
    });
}

#[allow(unused)]
fn replace_target(target: &str, line: &str) -> String {
    line.replace(target, "")
}
