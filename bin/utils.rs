use firestore::FirestoreDb;
use google_generative_ai_rs::v1::{
    api::Client,
    gemini::{Content, Model, Part, Role, request::Request},
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub trait DBInsertTrait {
    #[allow(unused)]
    fn id(&self) -> String;
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Question {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub level_id: u32,
    pub level_name: String,
    #[serde(default)]
    pub category_id: u32,
    pub category_name: String,

    #[serde(default)]
    pub chapter: String,
    pub sentence: String,
    pub prerequisites: Option<String>,
    pub sub_questions: Vec<SubQuestion>,
}

impl DBInsertTrait for Question {
    fn id(&self) -> String {
        self.id
            .clone()
            .unwrap_or(
                Uuid::new_v4()
                    .to_string()
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>(),
            )
            .clone()
    }
}

impl Question {
    #[allow(unused)]
    pub fn numbering(&mut self) {
        self.id = Some(Uuid::new_v4().to_string());

        let mut child_id = 1;
        for sub_question in self.sub_questions.iter_mut() {
            sub_question.id = child_id;
            child_id += 1;
        }
    }

    // レベルを数値に変換
    // 前提: level_nameがある
    // level_name: n1, n2, n3, n4, n5
    #[allow(unused)]
    pub fn leveling(&mut self) {
        let level = self.level_name.clone();
        // n3, n2, n1のnを取り除き、数字のみを取得
        let level_number = level.chars().skip(1).collect::<String>();
        if level_number.is_empty() {
            error!("level_number is empty");
            return;
        }

        match level_number.parse::<u32>() {
            Ok(n) => self.level_id = n,
            Err(e) => {
                error!("level_number parse error: {}", e);
            }
        };
    }
}

type SelectAnswer = HashMap<String, String>;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SubQuestion {
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub hint_id: u32,
    #[serde(default)]
    pub answer_id: u32,

    pub sentence: Option<String>,
    pub prerequisites: Option<String>,
    pub select_answer: Vec<SelectAnswer>,
    pub answer: String,

    #[serde(default)]
    pub voted: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CatValue {
    pub level_id: u32,
    pub id: u32,
    pub name: String,
}

impl DBInsertTrait for CatValue {
    fn id(&self) -> String {
        self.id.to_string()
    }
}

#[allow(unused)]
// 指定ディレクトリのファイルを走査する
pub fn walk_dir(dir: &Path) -> Vec<PathBuf> {
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

pub fn read_file(abs_filename: PathBuf) -> String {
    std::fs::read_to_string(abs_filename).unwrap_or_else(|e| {
        panic!("ファイルの読み込みに失敗しました: {}", e);
    })
}

#[allow(unused)]
pub fn write_file(abs_filename: PathBuf, content: &str) {
    std::fs::write(abs_filename, content).unwrap_or_else(|e| {
        panic!("ファイルの書き込みに失敗しました: {}", e);
    });
}

#[allow(unused)]
pub fn replace_target(target: &str, line: &str) -> String {
    line.replace(target, "")
}

#[allow(unused)]
pub fn replace_level(text: &str, target_level: &str) -> String {
    let replace_text = target_level.to_string().to_uppercase();
    text.replace("**LEVEL**", &format!("**{}**", replace_text))
}

#[allow(unused)]
pub fn get_key_and_model() -> (String, String) {
    let key = match env::var("GOOGLE_GEMINI_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            panic!("GOOGLE_GEMINI_API_KEY not set");
        }
    };
    let models = match env::var("GEMINI_MODELS") {
        Ok(m) => m,
        Err(_) => {
            panic!("GEMINI_MODELS not set");
        }
    };
    let models = models.split(",").collect::<Vec<&str>>();
    if models.len() != 2 {
        panic!("GEMINI_MODELS must be 2 models");
    }
    let model = models[0];

    (key, model.to_string())
}

// gemini api request
#[allow(unused)]
pub async fn request_gemini_api(key: String, model: String, text: &str) -> Result<String, String> {
    let client = Client::new_from_model(Model::Custom(model.to_string()), key.to_string());
    let request = Request {
        contents: vec![Content {
            role: Role::User,
            parts: vec![Part {
                text: Some(text.to_string()),
                inline_data: None,
                file_data: None,
                video_metadata: None,
            }],
        }],
        tools: vec![],
        safety_settings: vec![],
        generation_config: None,
        system_instruction: None,
    };

    let response = match client.post(75, &request).await {
        Ok(r) => r,
        Err(e) => {
            return Err(format!("Error: {}", e));
        }
    };

    let into_rest = response.rest().unwrap();
    match into_rest
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .and_then(|p| p.text.as_ref())
    {
        Some(t) => Ok(t.to_string()),
        None => Err("Error".to_string()),
    }
}

#[allow(unused)]
pub async fn to_database<T>(is_output: bool, collection_name: &str, values: Vec<T>) -> Vec<T>
where
    T: crate::utils::DBInsertTrait + Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    let project_id = std::env::var("PROJECT_ID").expect("PROJECT_ID must be set");

    let firestore = match FirestoreDb::new(project_id).await {
        Ok(firestore) => firestore,
        Err(e) => {
            error!("FirestoreDb::new failed: {:?}", e);
            return values;
        }
    };

    let mut err_values = vec![];

    for (index, value) in values.into_iter().enumerate() {
        if !is_output {
            info!("[not save] insert: {} {}", index, value.id());
            continue;
        }

        // let uid = Uuid::new_v4().to_string();

        match firestore
            .fluent()
            .insert()
            .into(collection_name)
            .document_id(value.id())
            .object(&value)
            .execute::<T>()
            .await
        {
            Ok(_) => info!("inserted: {} {}", index, value.id()),
            Err(e) => {
                error!("insert failed: {} {}, {}", index, value.id(), e);
                err_values.push(value);
            }
        };
    }

    err_values
}
