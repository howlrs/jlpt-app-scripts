use std::collections::HashSet;

use log::{info, warn};

mod utils;
use crate::utils::{
    read_questions_from_stage, write_questions_to_stage, Question, LEVELS,
    STAGE_2_OUTPUT,
};

/// 類似度の閾値（0.0〜1.0）。この値以上の類似度を持つ文は重複とみなす。
const SIMILARITY_THRESHOLD: f64 = 0.85;

fn main() {
    crate::utils::init_logger();

    let start = std::time::Instant::now();

    for level in LEVELS {
        // シャッフル済みファイルを優先、なければバリデーション済みを使用
        let input_file = "1_7_shuffled.json";
        let questions = match read_questions_from_stage(level, input_file) {
            Ok(q) => q,
            Err(e) => {
                warn!("[{}] {}", level, e);
                continue;
            }
        };

        if questions.is_empty() {
            warn!("[{}] 問題データなし、スキップ", level);
            continue;
        }

        let original_count = questions.len();
        let original_sub_count: usize = questions.iter().map(|q| q.sub_questions.len()).sum();

        let mut seen_sentences: Vec<String> = Vec::new();
        let mut deduplicated: Vec<Question> = Vec::new();
        let mut removed_exact = 0usize;
        let mut removed_similar = 0usize;

        for question in questions {
            let mut kept_subs = Vec::new();
            for sub_q in question.sub_questions {
                let sentence = sub_q
                    .sentence
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                // 正解の値を取得して複合キーを構成
                let correct_value = sub_q
                    .select_answer
                    .iter()
                    .find(|sa| sa.key == sub_q.answer)
                    .map(|sa| sa.value.trim().to_string())
                    .unwrap_or_default();
                let dedup_key = format!("{}||{}", sentence, correct_value);

                if sentence.is_empty() && correct_value.is_empty() {
                    kept_subs.push(sub_q);
                    continue;
                }

                // 完全一致チェック（問題文+正解値のペア）
                if seen_sentences.iter().any(|s| s == &dedup_key) {
                    removed_exact += 1;
                    continue;
                }

                // 類似度チェック（問題文部分のみで比較）
                let is_similar = !sentence.is_empty() && seen_sentences.iter().any(|existing| {
                    // 既存キーから問題文部分を抽出
                    let existing_sentence = existing.split("||").next().unwrap_or("");
                    let sim = normalized_similarity(&sentence, existing_sentence);
                    sim >= SIMILARITY_THRESHOLD
                });

                if is_similar {
                    removed_similar += 1;
                    continue;
                }

                seen_sentences.push(dedup_key);
                kept_subs.push(sub_q);
            }

            if kept_subs.is_empty() {
                continue;
            }

            deduplicated.push(Question {
                sub_questions: kept_subs,
                ..question
            });
        }

        let remaining_sub: usize = deduplicated.iter().map(|q| q.sub_questions.len()).sum();
        info!(
            "[{}] questions: {} → {}, sub_questions: {} → {} (完全一致除外={}, 類似除外={})",
            level,
            original_count,
            deduplicated.len(),
            original_sub_count,
            remaining_sub,
            removed_exact,
            removed_similar,
        );

        match write_questions_to_stage(level, STAGE_2_OUTPUT, &deduplicated) {
            Ok(_) => info!("[{}] wrote {}", level, STAGE_2_OUTPUT),
            Err(e) => warn!("[{}] 書込失敗: {}", level, e),
        }
    }

    info!("done, elapsed: {:?}", start.elapsed());
}

/// 2つの文字列の正規化類似度を計算（0.0〜1.0、1.0が完全一致）
/// Levenshtein距離ベース
fn normalized_similarity(a: &str, b: &str) -> f64 {
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
fn levenshtein_distance(a: &[char], b: &[char]) -> usize {
    let (m, n) = (a.len(), b.len());

    // 短い方の文字列+1のサイズだけメモリ使用（省メモリ版）
    let mut prev = vec![0usize; n + 1];
    let mut curr = vec![0usize; n + 1];

    for j in 0..=n {
        prev[j] = j;
    }

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1) // 削除
                .min(curr[j - 1] + 1) // 挿入
                .min(prev[j - 1] + cost); // 置換
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}
