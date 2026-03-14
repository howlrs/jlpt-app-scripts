use firestore::FirestoreDb;
use google_generative_ai_rs::v1::{
    api::Client,
    gemini::{
        request::{GenerationConfig, Request, SystemInstructionContent, SystemInstructionPart},
        Content, Model, Part, Role,
    },
};
use log::{error, info};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const LEVELS: &[&str] = &["n1", "n2", "n3", "n4", "n5"];
pub const OUTPUT_DIR: &str = "output";
pub const QUESTIONS_DIR: &str = "questions";
pub const PROMPT_DIR: &str = "prompts";

// Pipeline stage file names
pub const STAGE_1_OUTPUT: &str = "1_parsed.json";
pub const STAGE_1_5_VALIDATED: &str = "1_5_validated.json";
pub const STAGE_1_5_REJECTED: &str = "1_5_rejected.json";
pub const STAGE_2_OUTPUT: &str = "2_deduplicated.json";
pub const STAGE_3_OUTPUT: &str = "3_numbered.json";
pub const STAGE_4_OUTPUT: &str = "4_leveled.json";
pub const STAGE_5_OUTPUT: &str = "5_categories_meta.json";

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

pub trait DBInsertTrait {
    #[allow(unused)]
    fn id(&self) -> String;
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, Default)]
pub struct Question {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub level_id: u32,
    pub level_name: String,
    #[serde(default)]
    pub category_id: Option<String>,
    pub category_name: String,

    pub sentence: String,
    pub prerequisites: Option<String>,
    pub sub_questions: Vec<SubQuestion>,

    /// 生成に使用したAIモデル名（品質追跡用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
}

// QuestionのDeserializeトレイトの実装を拡張
impl<'de> Deserialize<'de> for Question {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct QuestionHelper {
            #[serde(default)]
            id: Option<String>,
            #[serde(default)]
            level_id: u32,
            level_name: String,
            #[serde(default)]
            category_id: Option<CategoryId>,
            category_name: String,
            sentence: String,
            prerequisites: Option<String>,
            sub_questions: Vec<SubQuestion>,
            #[serde(default)]
            generated_by: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum CategoryId {
            String(String),
            Number(u32),
        }

        let helper = QuestionHelper::deserialize(deserializer)?;

        let category_id = match helper.category_id {
            Some(CategoryId::String(s)) => Some(s),
            Some(CategoryId::Number(n)) => Some(n.to_string()),
            None => None,
        };

        Ok(Question {
            id: helper.id,
            level_id: helper.level_id,
            level_name: helper.level_name,
            category_id,
            category_name: helper.category_name,
            sentence: helper.sentence,
            prerequisites: helper.prerequisites,
            sub_questions: helper.sub_questions,
            generated_by: helper.generated_by,
        })
    }
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SelectAnswer {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SubQuestion {
    #[serde(default)]
    pub id: u32,

    pub sentence: Option<String>,
    pub prerequisites: Option<String>,
    pub select_answer: Vec<SelectAnswer>,
    pub answer: String,
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

// ---------------------------------------------------------------------------
// Directory / path helpers
// ---------------------------------------------------------------------------

/// Get the level output directory path: output/questions/{level}
#[allow(unused)]
pub fn level_dir(level: &str) -> PathBuf {
    PathBuf::from(OUTPUT_DIR).join(QUESTIONS_DIR).join(level)
}

/// Ensure directory exists, creating if needed.
#[allow(unused)]
pub fn ensure_dir(path: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir_all(path)
            .map_err(|e| format!("Failed to create directory {}: {}", path.display(), e))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

/// Walk a directory and return all file paths (non-recursive, files only).
#[allow(unused)]
pub fn walk_dir(dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![];
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            error!("Failed to read directory {}: {}", dir.display(), e);
            return files;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_file() {
            files.push(path);
        }
    }
    files
}

/// Read a file to string. Returns `Result` instead of panicking.
pub fn read_file(abs_filename: PathBuf) -> String {
    std::fs::read_to_string(&abs_filename).unwrap_or_else(|e| {
        panic!("ファイルの読み込みに失敗しました: {} – {}", abs_filename.display(), e);
    })
}

/// Write content to a file. Returns `Result` instead of panicking.
#[allow(unused)]
pub fn write_file(abs_filename: PathBuf, content: &str) {
    std::fs::write(&abs_filename, content).unwrap_or_else(|e| {
        panic!("ファイルの書き込みに失敗しました: {} – {}", abs_filename.display(), e);
    });
}

/// Read questions from a stage output file for a given level.
#[allow(unused)]
pub fn read_questions_from_stage(level: &str, stage_file: &str) -> Result<Vec<Question>, String> {
    let path = level_dir(level).join(stage_file);
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str::<Vec<Question>>(&content)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// Write questions to a stage output file for a given level.
#[allow(unused)]
pub fn write_questions_to_stage(
    level: &str,
    stage_file: &str,
    questions: &[Question],
) -> Result<(), String> {
    let dir = level_dir(level);
    ensure_dir(&dir)?;
    let path = dir.join(stage_file);
    let json = serde_json::to_string_pretty(questions)
        .map_err(|e| format!("Failed to serialize questions: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

/// Remove AI markdown syntax (```json ... ```) from JSON responses.
#[allow(unused)]
pub fn remove_ai_json_syntax(content: &str) -> String {
    let trimmed = content.trim();
    let trimmed = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_suffix("```")
        .unwrap_or(trimmed);
    trimmed.trim().to_string()
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Initialize the env_logger.
#[allow(unused)]
pub fn init_logger() {
    env_logger::init();
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

#[allow(unused)]
pub fn replace_level(text: &str, target_level: &str) -> String {
    let replace_text = target_level.to_string().to_uppercase();
    text.replace("**LEVEL**", &format!("**{}**", replace_text))
}

// ---------------------------------------------------------------------------
// Gemini API
// ---------------------------------------------------------------------------

/// APIキーとモデル名を取得（プライマリ, フォールバック）
#[allow(unused)]
pub fn get_key_and_models() -> (String, String, String) {
    let key = env::var("GOOGLE_GEMINI_API_KEY").expect("GOOGLE_GEMINI_API_KEY not set");
    let models_str = env::var("GEMINI_MODELS").expect("GEMINI_MODELS not set");
    let models: Vec<&str> = models_str.split(',').collect();
    if models.len() < 2 {
        panic!("GEMINI_MODELS must have at least 2 models (primary,fallback)");
    }
    (key, models[0].trim().to_string(), models[1].trim().to_string())
}

/// 後方互換: 旧APIとの互換性
#[allow(unused)]
pub fn get_key_and_model() -> (String, String) {
    let (key, primary, _) = get_key_and_models();
    (key, primary)
}

/// Send a request to the Gemini API with generation config and optional system instruction.
#[allow(unused)]
pub async fn request_gemini_api(
    key: String,
    model: String,
    text: &str,
    system_instruction: Option<&str>,
) -> Result<String, String> {
    let client = Client::new_from_model(Model::Custom(model.to_string()), key.to_string());

    let generation_config = Some(GenerationConfig {
        temperature: Some(0.8),
        top_p: None,
        top_k: None,
        candidate_count: None,
        max_output_tokens: Some(8192),
        stop_sequences: None,
        response_mime_type: None,
        response_schema: None,
    });

    let sys_instruction = system_instruction.map(|t| SystemInstructionContent {
        parts: vec![SystemInstructionPart {
            text: Some(t.to_string()),
        }],
    });

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
        generation_config,
        system_instruction: sys_instruction,
    };

    let response = client
        .post(75, &request)
        .await
        .map_err(|e| format!("Gemini API request failed: {}", e))?;

    let rest_response = response
        .rest()
        .ok_or_else(|| "Gemini API returned a non-REST response (streaming?)".to_string())?;

    rest_response
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .and_then(|p| p.text.as_ref())
        .map(|t| t.to_string())
        .ok_or_else(|| "Gemini API response contained no text content".to_string())
}

// ---------------------------------------------------------------------------
// Firestore helpers
// ---------------------------------------------------------------------------

#[allow(unused)]
pub async fn to_database<T>(is_output: bool, collection_name: &str, values: Vec<T>) -> Vec<T>
where
    T: crate::utils::DBInsertTrait + Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    let project_id = match std::env::var("PROJECT_ID") {
        Ok(id) => id,
        Err(_) => {
            error!("PROJECT_ID must be set");
            return values;
        }
    };

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

#[allow(unused)]
pub async fn to_database_with_uuid<T>(
    is_output: bool,
    collection_name: &str,
    values: Vec<T>,
) -> Vec<T>
where
    T: crate::utils::DBInsertTrait + Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    let project_id = match std::env::var("PROJECT_ID") {
        Ok(id) => id,
        Err(_) => {
            error!("PROJECT_ID must be set");
            return values;
        }
    };

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

        let uid = Uuid::new_v4().to_string();

        match firestore
            .fluent()
            .insert()
            .into(collection_name)
            .document_id(uid)
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

// ---------------------------------------------------------------------------
// Gemini API helpers (shared across binaries)
// ---------------------------------------------------------------------------

/// システム指示文（問題生成用）
#[allow(unused)]
pub const SYSTEM_INSTRUCTION: &str = r#"あなたはJLPT（日本語能力試験）の公式問題作成の専門家です。以下を厳守してください：
1. 出力は必ず有効なJSONのみ（マークダウン記法禁止、説明文禁止）
2. 選択肢は必ず4つ。正解は必ず1つだけ
3. 正解の位置（1〜4）を偏らせない
4. 誤答は「一見正しそうだが明確な理由で不正解」であること
5. 選択肢の長さ・構造・語彙レベルを揃えること
6. 指定されたレベルの語彙・文法範囲を厳守すること
7. 指定されたカテゴリの問題のみを生成すること"#;

/// フォールバックモデル付きリトライでGemini APIにリクエスト
#[allow(unused)]
pub async fn request_with_fallback(
    key: &str,
    primary_model: &str,
    fallback_model: &str,
    prompt: &str,
    system_instruction: &str,
    max_retries: u32,
) -> Option<(String, String)> {
    for attempt in 0..=max_retries {
        match request_gemini_api(
            key.to_string(), primary_model.to_string(),
            prompt, Some(system_instruction),
        ).await {
            Ok(text) => return Some((text, primary_model.to_string())),
            Err(e) => {
                if attempt >= max_retries {
                    log::warn!("プライマリ({})が{}回失敗。フォールバック試行: {}",
                        primary_model, max_retries, e);
                    break;
                }
                let wait = 60 * (attempt + 1) as u64;
                log::warn!("[{}] リトライ {}/{}: {} - {}秒待機",
                    primary_model, attempt + 1, max_retries, e, wait);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            }
        }
    }

    match request_gemini_api(
        key.to_string(), fallback_model.to_string(),
        prompt, Some(system_instruction),
    ).await {
        Ok(text) => {
            info!("フォールバック({})で成功", fallback_model);
            Some((text, fallback_model.to_string()))
        }
        Err(e) => {
            error!("プライマリ・フォールバック両方失敗: {}", e);
            None
        }
    }
}
