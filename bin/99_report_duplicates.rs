//! 本番 Firestore から全 questions を取得し、dedup_common::dedup_key でグルーピング。
//! tiebreaker (createTime 古い方 > sentence 長 > qid 辞書順) で残すレコードを決め、
//! `reports/duplicates.json` に削除候補を出力する (読み取り専用)。

use chrono::{DateTime, Utc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

mod utils;
#[path = "dedup_common.rs"]
mod dedup_common;

use crate::dedup_common::{dedup_key, prefer_keep_order, Candidate, SubLike};

/// Firestore REST API のドキュメントラッパー
#[derive(Deserialize, Debug)]
struct RestDocument {
    name: String,
    #[serde(default)]
    fields: serde_json::Value,
    #[serde(rename = "createTime")]
    create_time: String,
}

#[derive(Deserialize, Debug)]
struct RestListResponse {
    #[serde(default)]
    documents: Vec<RestDocument>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Serialize)]
struct ReportRow {
    parent_id: String,
    sub_idx: usize,
    sentence: String,
    correct_value: String,
    create_time: String,
}

#[derive(Serialize)]
struct ReportGroup {
    dedup_key: String,
    keep: ReportRow,
    remove: Vec<ReportRow>,
}

#[derive(Serialize)]
struct Report {
    generated_at: String,
    source: String,
    total_parents: usize,
    total_sub_questions: usize,
    dedup_groups: usize,
    rows_in_dup_groups: usize,
    removable_subs: usize,
    skipped_numeric_placeholder: usize,
    skipped_answer_not_in_options: usize,
    groups: Vec<ReportGroup>,
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let project_id = match env::var("PROJECT_ID") {
        Ok(id) => id,
        Err(_) => { error!("PROJECT_ID must be set"); return; }
    };
    let token = match get_access_token() {
        Ok(t) => t,
        Err(e) => { error!("gcloud auth print-access-token failed: {}", e); return; }
    };

    info!("=== 99_report_duplicates ===");
    info!("Firestore REST API で questions を取得中...");

    let docs = fetch_all_questions(&project_id, &token).await;
    info!("取得完了: {} docs", docs.len());

    // 全 sub_question を dedup_key でグルーピング
    let mut total_subs = 0usize;
    let mut skipped_numeric = 0usize;
    let mut skipped_answer_missing = 0usize;
    let mut groups: HashMap<String, Vec<(Candidate, String, String)>> = HashMap::new();
    //                          key    (cand, sentence, correct_value)

    for doc in &docs {
        let fields = &doc.fields;
        let parent_id = extract_string(fields, "id").unwrap_or_else(|| {
            // fallback: name 末尾
            doc.name.rsplit('/').next().unwrap_or("").to_string()
        });
        let level_id = extract_int(fields, "level_id").unwrap_or(0) as u32;
        let create_time: DateTime<Utc> = doc.create_time.parse().unwrap_or_else(|_| Utc::now());

        let subs_arr = extract_array(fields, "sub_questions");
        for (idx, sub_mv) in subs_arr.iter().enumerate() {
            total_subs += 1;
            let sub_fields = sub_mv.get("mapValue")
                .and_then(|m| m.get("fields"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let sentence = extract_string(&sub_fields, "sentence").unwrap_or_default();
            let answer = extract_string(&sub_fields, "answer").unwrap_or_default();
            let options = extract_options(&sub_fields);

            let sub_like = SubLike {
                options: options.clone(),
                answer: answer.clone(),
            };
            let key = match dedup_key(level_id, &sub_like) {
                Ok(k) => k,
                Err(crate::dedup_common::KeySkipReason::NumericPlaceholder) => {
                    skipped_numeric += 1;
                    continue;
                }
                Err(crate::dedup_common::KeySkipReason::AnswerNotInOptions) => {
                    skipped_answer_missing += 1;
                    continue;
                }
            };

            let correct_value = options.iter()
                .find(|(k, _)| k == &answer)
                .map(|(_, v)| v.clone())
                .unwrap_or_default();

            let cand = Candidate {
                parent_id: parent_id.clone(),
                sub_idx: idx,
                create_time,
                sentence_len: sentence.chars().count(),
            };
            groups.entry(key).or_insert_with(Vec::new).push((cand, sentence, correct_value));
        }
    }

    // 重複グループのみ抽出
    let mut dup_groups_out: Vec<ReportGroup> = Vec::new();
    let mut rows_in_dup_groups = 0usize;
    let mut removable = 0usize;

    for (key, mut rows) in groups {
        if rows.len() <= 1 { continue; }
        rows.sort_by(|a, b| prefer_keep_order(&a.0, &b.0));
        let keep_tuple = rows.remove(0);
        let keep = ReportRow {
            parent_id: keep_tuple.0.parent_id,
            sub_idx: keep_tuple.0.sub_idx,
            sentence: keep_tuple.1,
            correct_value: keep_tuple.2,
            create_time: keep_tuple.0.create_time.to_rfc3339(),
        };
        let remove_rows: Vec<ReportRow> = rows.into_iter().map(|(cand, sent, cv)| ReportRow {
            parent_id: cand.parent_id,
            sub_idx: cand.sub_idx,
            sentence: sent,
            correct_value: cv,
            create_time: cand.create_time.to_rfc3339(),
        }).collect();
        rows_in_dup_groups += remove_rows.len() + 1;
        removable += remove_rows.len();
        dup_groups_out.push(ReportGroup { dedup_key: key, keep, remove: remove_rows });
    }

    // create_time 降順でソート (新しい重複ほど上位、レビューしやすい)
    dup_groups_out.sort_by(|a, b| b.keep.create_time.cmp(&a.keep.create_time));

    let report = Report {
        generated_at: Utc::now().to_rfc3339(),
        source: format!("firestore:{}/(default)", project_id),
        total_parents: docs.len(),
        total_sub_questions: total_subs,
        dedup_groups: dup_groups_out.len(),
        rows_in_dup_groups,
        removable_subs: removable,
        skipped_numeric_placeholder: skipped_numeric,
        skipped_answer_not_in_options: skipped_answer_missing,
        groups: dup_groups_out,
    };

    // 出力
    let out_dir = PathBuf::from("reports");
    if let Err(e) = fs::create_dir_all(&out_dir) {
        error!("failed to create reports dir: {:?}", e);
        return;
    }
    let out_path = out_dir.join("duplicates.json");
    let json = match serde_json::to_string_pretty(&report) {
        Ok(s) => s,
        Err(e) => { error!("serialize failed: {:?}", e); return; }
    };
    if let Err(e) = fs::write(&out_path, json) {
        error!("write failed: {:?}", e);
        return;
    }

    info!("=== Report ===");
    info!("total_parents:                 {}", report.total_parents);
    info!("total_sub_questions:           {}", report.total_sub_questions);
    info!("dedup_groups:                  {}", report.dedup_groups);
    info!("rows_in_dup_groups:            {}", report.rows_in_dup_groups);
    info!("removable_subs:                {}", report.removable_subs);
    info!("skipped_numeric_placeholder:   {}", report.skipped_numeric_placeholder);
    info!("skipped_answer_not_in_options: {}", report.skipped_answer_not_in_options);
    info!("出力: {}", out_path.display());
}

fn get_access_token() -> Result<String, String> {
    let output = std::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .map_err(|e| format!("failed to execute gcloud: {}", e))?;
    if !output.status.success() {
        return Err(format!("gcloud exit status: {}", output.status));
    }
    let s = String::from_utf8(output.stdout)
        .map_err(|e| format!("invalid utf8: {}", e))?;
    Ok(s.trim().to_string())
}

async fn fetch_all_questions(project_id: &str, token: &str) -> Vec<RestDocument> {
    let client = reqwest::Client::new();
    let mut out: Vec<RestDocument> = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let mut url = format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents/questions?pageSize=300",
            project_id
        );
        if let Some(t) = &page_token {
            url.push_str(&format!("&pageToken={}", t));
        }
        let resp = match client.get(&url).bearer_auth(token).send().await {
            Ok(r) => r,
            Err(e) => { error!("fetch failed: {:?}", e); break; }
        };
        let body: RestListResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => { error!("json parse failed: {:?}", e); break; }
        };
        out.extend(body.documents);
        match body.next_page_token {
            Some(ref t) if !t.is_empty() => { page_token = Some(t.clone()); }
            _ => break,
        }
        info!("  取得中... {} docs", out.len());
    }
    out
}

fn extract_string(fields: &serde_json::Value, key: &str) -> Option<String> {
    fields.get(key)?.get("stringValue")?.as_str().map(|s| s.to_string())
}

fn extract_int(fields: &serde_json::Value, key: &str) -> Option<i64> {
    fields.get(key)?.get("integerValue")?.as_str()?.parse().ok()
}

fn extract_array(fields: &serde_json::Value, key: &str) -> Vec<serde_json::Value> {
    fields.get(key)
        .and_then(|v| v.get("arrayValue"))
        .and_then(|v| v.get("values"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn extract_options(sub_fields: &serde_json::Value) -> Vec<(String, String)> {
    let values = extract_array(sub_fields, "select_answer");
    values.iter().filter_map(|opt| {
        let of = opt.get("mapValue")?.get("fields")?;
        let k = of.get("key")?.get("stringValue")?.as_str()?.to_string();
        let v = of.get("value")?.get("stringValue")?.as_str()?.to_string();
        Some((k, v))
    }).collect()
}
