# DB内問題品質監視スクリプト (monitor_quality) 実装計画

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Firestoreに保存済みの問題に対して重複・類似検出→削除→品質レポート出力を行う `monitor_quality` バイナリを実装する（MVP: Step 1）

**Architecture:** 既存の `check_db.rs` / `review_votes.rs` のFirestoreアクセスパターンと `2_duplicate.rs` のLevenshtein重複検出ロジックを組み合わせ、DB上の問題を直接スキャンする。カテゴリ内グルーピング + 文字列長フィルタで O(n²) 比較を削減。DRY_RUNデフォルト、`--execute` で実削除。

**Tech Stack:** Rust, Firestore SDK (`firestore` crate), `futures-util`, Levenshtein距離（既存実装を `utils.rs` に移動して共有）

**GitHub Issue:** #11

---

### Task 1: Levenshteinロジックを `utils.rs` に移動して共有化

**Files:**
- Modify: `bin/utils.rs` — `normalized_similarity` / `levenshtein_distance` を追加
- Modify: `bin/2_duplicate.rs` — ローカル関数を `utils::` 呼び出しに変更

**Step 1: `utils.rs` にLevenshtein関数を追加**

`utils.rs` の末尾に以下を追加:

```rust
// ---------------------------------------------------------------------------
// String similarity (Levenshtein)
// ---------------------------------------------------------------------------

/// 類似度の閾値（0.0〜1.0）。この値以上の類似度を持つ文は重複とみなす。
pub const SIMILARITY_THRESHOLD: f64 = 0.85;

/// 2つの文字列の正規化類似度を計算（0.0〜1.0、1.0が完全一致）
/// Levenshtein距離ベース
pub fn normalized_similarity(a: &str, b: &str) -> f64 {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let max_len = a_chars.len().max(b_chars.len());
    if max_len == 0 {
        return 1.0;
    }
    let dist = levenshtein_distance(&a_chars, &b_chars);
    1.0 - (dist as f64 / max_len as f64)
}

/// Levenshtein距離をDP法で計算
pub fn levenshtein_distance(a: &[char], b: &[char]) -> usize {
    let (m, n) = (a.len(), b.len());
    let mut prev = vec![0usize; n + 1];
    let mut curr = vec![0usize; n + 1];

    for j in 0..=n {
        prev[j] = j;
    }

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}
```

**Step 2: `2_duplicate.rs` のローカル関数を `utils::` に置換**

`2_duplicate.rs` から `normalized_similarity` と `levenshtein_distance` 関数定義を削除し、`use` に `normalized_similarity, SIMILARITY_THRESHOLD` を追加。本体内の呼び出しを `utils::normalized_similarity` に変更。

**Step 3: ビルド確認**

Run: `cargo build --bin duplicate`
Expected: コンパイル成功

**Step 4: コミット**

```bash
git add bin/utils.rs bin/2_duplicate.rs
git commit -m "refactor: Levenshteinロジックをutils.rsに共通化"
```

---

### Task 2: `monitor_quality.rs` — DB取得 & 重複検出コア実装

**Files:**
- Create: `bin/monitor_quality.rs`
- Modify: `Cargo.toml` — `[[bin]]` エントリ追加

**Step 1: `Cargo.toml` にバイナリ追加**

```toml
# DB内問題品質監視・重複検出・削除
[[bin]]
name = "monitor_quality"
path = "bin/monitor_quality.rs"
```

**Step 2: `monitor_quality.rs` を作成**

全体構成:
```
main()
├── 引数パース (--execute, --level)
├── Firestore接続
├── Phase 1: レベル別にDB全問題取得
├── Phase 2: カテゴリ内グルーピング → 重複検出
│   ├── 完全一致（複合キー: sentence||correct_value）
│   ├── 文字列長フィルタ（差20%以上をスキップ）
│   └── Levenshtein類似度 ≥ 0.85
├── Phase 3: レポート出力
├── Phase 4: --execute時のみ削除実行
└── サマリー出力
```

実装コード:

```rust
/// DB内問題の品質監視・重複検出・削除
///
/// 機能:
///   1. Firestoreから全問題を取得
///   2. カテゴリ内でsub_question単位の重複・類似を検出
///   3. 品質レポートを出力
///   4. --execute オプションで重複問題を削除
///
/// 使用方法:
///   cargo run --bin monitor_quality                    # レポートのみ
///   cargo run --bin monitor_quality -- --execute       # レポート + 削除実行
///   cargo run --bin monitor_quality -- --level n3      # N3のみ対象
///   SIMILARITY_THRESHOLD=0.90 cargo run --bin monitor_quality  # 閾値変更
use std::collections::HashMap;

use futures_util::StreamExt;
use log::{error, info, warn};

mod utils;
use utils::{normalized_similarity, Question, SIMILARITY_THRESHOLD};

#[derive(Debug)]
struct DuplicatePair {
    question_id_a: String,
    question_id_b: String,
    sub_sentence_a: String,
    sub_sentence_b: String,
    similarity: f64,
    dup_type: String, // "exact" or "similar"
}

#[derive(Debug, Default)]
struct LevelReport {
    level: String,
    total_questions: usize,
    total_sub_questions: usize,
    duplicates: Vec<DuplicatePair>,
    category_counts: HashMap<String, (String, usize)>, // cat_id -> (cat_name, sub_q_count)
    answer_distribution: [usize; 4], // 正解の分布 [1,2,3,4]
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    utils::init_logger();

    let execute_mode = std::env::args().any(|a| a == "--execute");
    let target_level: Option<String> = std::env::args()
        .skip_while(|a| a != "--level")
        .nth(1);

    let threshold: f64 = std::env::var("SIMILARITY_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(SIMILARITY_THRESHOLD);

    info!("=== JLPT品質監視 === (execute: {}, 類似閾値: {:.0}%)", execute_mode, threshold * 100.0);

    let project_id = std::env::var("PROJECT_ID").expect("PROJECT_ID must be set");
    let db = match firestore::FirestoreDb::new(&project_id).await {
        Ok(db) => db,
        Err(e) => {
            error!("Firestore接続失敗: {}", e);
            std::process::exit(1);
        }
    };

    let levels: Vec<&str> = match &target_level {
        Some(l) => vec![Box::leak(l.clone().into_boxed_str()) as &str],
        None => vec!["n1", "n2", "n3", "n4", "n5"],
    };

    let mut all_reports: Vec<LevelReport> = Vec::new();
    let mut all_delete_ids: Vec<String> = Vec::new();

    for level_name in &levels {
        let level_id: u32 = level_name.chars().last()
            .and_then(|c| c.to_digit(10))
            .unwrap_or(0);

        info!("--- {} (level_id={}) 取得中 ---", level_name.to_uppercase(), level_id);

        // Phase 1: DB全問題取得
        let mut questions: Vec<(String, Question)> = Vec::new(); // (doc_id, question)

        let stream_result = db
            .fluent()
            .select()
            .from("questions")
            .filter(|q| {
                q.field(firestore::path!(Question::level_id)).eq(level_id)
            })
            .obj::<Question>()
            .stream_query_with_errors()
            .await;

        match stream_result {
            Ok(mut stream) => {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(q) => {
                            let doc_id = q.id.clone().unwrap_or_default();
                            questions.push((doc_id, q));
                        }
                        Err(e) => warn!("ドキュメント読取エラー: {}", e),
                    }
                }
            }
            Err(e) => {
                warn!("{}: クエリエラー: {}", level_name, e);
                continue;
            }
        }

        info!("{}: {}件取得", level_name.to_uppercase(), questions.len());

        // Phase 2: 重複検出
        let mut report = LevelReport {
            level: level_name.to_uppercase().to_string(),
            total_questions: questions.len(),
            ..Default::default()
        };

        // カテゴリ別にグルーピング
        // key: category_id, value: Vec<(question_doc_id, sub_sentence, correct_value)>
        let mut category_groups: HashMap<String, Vec<(String, String, String)>> = HashMap::new();

        for (doc_id, q) in &questions {
            let cat_id = q.category_id.clone().unwrap_or_default();
            let cat_name = q.category_name.clone();
            let entry = report.category_counts.entry(cat_id.clone())
                .or_insert((cat_name, 0));

            for sub_q in &q.sub_questions {
                entry.1 += 1;
                report.total_sub_questions += 1;

                // 正解分布
                if let Ok(ans) = sub_q.answer.parse::<usize>() {
                    if ans >= 1 && ans <= 4 {
                        report.answer_distribution[ans - 1] += 1;
                    }
                }

                let sentence = sub_q.sentence.as_deref().unwrap_or("").trim().to_string();
                let correct_value = sub_q.select_answer.iter()
                    .find(|sa| sa.key == sub_q.answer)
                    .map(|sa| sa.value.trim().to_string())
                    .unwrap_or_default();

                if !sentence.is_empty() {
                    category_groups.entry(cat_id.clone())
                        .or_default()
                        .push((doc_id.clone(), sentence, correct_value));
                }
            }
        }

        // カテゴリ内で重複検出
        let mut duplicate_question_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (cat_id, items) in &category_groups {
            let mut seen: Vec<(usize, String)> = Vec::new(); // (index, dedup_key)

            for (idx, (doc_id, sentence, correct_value)) in items.iter().enumerate() {
                let dedup_key = format!("{}||{}", sentence, correct_value);

                // 完全一致チェック
                let exact_match = seen.iter().find(|(_, key)| key == &dedup_key);
                if let Some((orig_idx, _)) = exact_match {
                    let orig_doc_id = &items[*orig_idx].0;
                    report.duplicates.push(DuplicatePair {
                        question_id_a: orig_doc_id.clone(),
                        question_id_b: doc_id.clone(),
                        sub_sentence_a: items[*orig_idx].1.clone(),
                        sub_sentence_b: sentence.clone(),
                        similarity: 1.0,
                        dup_type: "exact".to_string(),
                    });
                    duplicate_question_ids.insert(doc_id.clone());
                    continue;
                }

                // 文字列長フィルタ + Levenshtein類似度チェック
                let sentence_chars = sentence.chars().count();
                let similar_match = seen.iter().find(|(orig_idx, _)| {
                    let orig_sentence = &items[*orig_idx].1;
                    let orig_chars = orig_sentence.chars().count();

                    // 文字列長の差が20%以上なら類似度計算をスキップ
                    let len_ratio = sentence_chars.min(orig_chars) as f64
                        / sentence_chars.max(orig_chars).max(1) as f64;
                    if len_ratio < 0.8 {
                        return false;
                    }

                    let sim = normalized_similarity(sentence, orig_sentence);
                    sim >= threshold
                });

                if let Some((orig_idx, _)) = similar_match {
                    let orig_doc_id = &items[*orig_idx].0;
                    let sim = normalized_similarity(sentence, &items[*orig_idx].1);
                    report.duplicates.push(DuplicatePair {
                        question_id_a: orig_doc_id.clone(),
                        question_id_b: doc_id.clone(),
                        sub_sentence_a: items[*orig_idx].1.clone(),
                        sub_sentence_b: sentence.clone(),
                        similarity: sim,
                        dup_type: "similar".to_string(),
                    });
                    duplicate_question_ids.insert(doc_id.clone());
                    continue;
                }

                seen.push((idx, dedup_key));
            }
        }

        all_delete_ids.extend(duplicate_question_ids);
        all_reports.push(report);
    }

    // Phase 3: レポート出力
    info!("");
    info!("========================================");
    info!("  JLPT品質監視レポート");
    info!("========================================");

    let mut total_duplicates = 0usize;

    for report in &all_reports {
        let exact_count = report.duplicates.iter().filter(|d| d.dup_type == "exact").count();
        let similar_count = report.duplicates.iter().filter(|d| d.dup_type == "similar").count();
        total_duplicates += report.duplicates.len();

        info!("");
        info!("【{}】", report.level);
        info!("  総問題数: {} (sub: {})", report.total_questions, report.total_sub_questions);
        info!("  重複検出: {}件 (完全一致: {}, 類似: {})", report.duplicates.len(), exact_count, similar_count);

        // 正解分布
        let total_ans: usize = report.answer_distribution.iter().sum();
        if total_ans > 0 {
            let dist: Vec<String> = report.answer_distribution.iter().enumerate()
                .map(|(i, c)| format!("{}={:.0}%", i + 1, *c as f64 / total_ans as f64 * 100.0))
                .collect();
            info!("  正解分布: {}", dist.join(" "));
        }

        // カテゴリ別
        let mut cats: Vec<_> = report.category_counts.iter().collect();
        cats.sort_by_key(|(id, _)| id.parse::<u32>().unwrap_or(0));
        for (cat_id, (cat_name, count)) in &cats {
            info!("    cat_{} ({}): {} sub_questions", cat_id, cat_name, count);
        }

        // 重複詳細（最初の10件のみ）
        if !report.duplicates.is_empty() {
            info!("  --- 重複詳細（最大10件表示）---");
            for dup in report.duplicates.iter().take(10) {
                info!("    [{}] {:.0}%: \"{}...\" vs \"{}...\"",
                    dup.dup_type,
                    dup.similarity * 100.0,
                    truncate(&dup.sub_sentence_a, 30),
                    truncate(&dup.sub_sentence_b, 30),
                );
            }
            if report.duplicates.len() > 10 {
                info!("    ... 他 {}件", report.duplicates.len() - 10);
            }
        }
    }

    // 重複レポートをJSONファイルに保存
    let report_json: Vec<serde_json::Value> = all_reports.iter().flat_map(|r| {
        r.duplicates.iter().map(|d| serde_json::json!({
            "level": r.level,
            "type": d.dup_type,
            "similarity": format!("{:.2}", d.similarity),
            "question_id_a": d.question_id_a,
            "question_id_b": d.question_id_b,
            "sentence_a": d.sub_sentence_a,
            "sentence_b": d.sub_sentence_b,
        }))
    }).collect();

    let output_path = std::env::current_dir().unwrap().join("output").join("quality_report.json");
    if let Ok(json) = serde_json::to_string_pretty(&report_json) {
        std::fs::write(&output_path, &json).unwrap_or_else(|e| warn!("レポート保存失敗: {}", e));
        info!("レポート保存: {:?}", output_path);
    }

    // Phase 4: 削除実行
    // 重複IDの一意化
    let unique_delete: Vec<String> = {
        let mut set = std::collections::HashSet::new();
        all_delete_ids.into_iter().filter(|id| set.insert(id.clone())).collect()
    };

    info!("");
    info!("========================================");
    info!("  削除対象: {}件の親問題", unique_delete.len());
    info!("========================================");

    if execute_mode && !unique_delete.is_empty() {
        info!("--- 削除実行 ---");
        let mut deleted = 0u32;
        for qid in &unique_delete {
            match db.fluent().delete().from("questions").document_id(qid).execute().await {
                Ok(_) => {
                    deleted += 1;
                    info!("  削除: {}", qid);
                }
                Err(e) => warn!("  削除失敗 {}: {}", qid, e),
            }
        }
        info!("{}件削除完了", deleted);
    } else if !unique_delete.is_empty() {
        info!("--execute フラグで実行してください");
    } else {
        info!("重複問題なし - 削除不要");
    }

    info!("=== 完了 ===");
}

fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        chars[..max_chars].iter().collect()
    }
}
```

**Step 3: ビルド確認**

Run: `cargo build --bin monitor_quality`
Expected: コンパイル成功

**Step 4: コミット**

```bash
git add bin/monitor_quality.rs Cargo.toml
git commit -m "feat: DB内問題品質監視スクリプト (monitor_quality) MVP"
```

---

### Task 3: 動作確認 & ドキュメント更新

**Files:**
- Modify: `docs/pipeline.md` — monitor_quality の使い方を追記

**Step 1: DRY_RUNテスト**

Run: `RUST_LOG=info cargo run --bin monitor_quality`
Expected: レポート出力、削除なし

**Step 2: レベル指定テスト**

Run: `RUST_LOG=info cargo run --bin monitor_quality -- --level n3`
Expected: N3のみのレポート出力

**Step 3: pipeline.md に追記**

monitor_quality の使い方セクションを追加。

**Step 4: コミット**

```bash
git add docs/pipeline.md
git commit -m "docs: monitor_quality の使い方を追記"
```
