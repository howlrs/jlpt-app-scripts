use std::{
    env, fs,
    path::{Path, PathBuf},
};

use google_generative_ai_rs::v1::{
    api::Client,
    gemini::{Content, Model, Part, Role, request::Request},
};

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

pub fn replace_level(text: &str, target_level: &str) -> String {
    let replace_text = target_level.to_string().to_uppercase();
    text.replace("**LEVEL**", &format!("**{}**", replace_text))
}

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
