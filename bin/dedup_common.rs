//! 重複検出用の共通ヘルパー。
//!
//! 複数のバイナリ (`2_duplicate.rs`, `99_report_duplicates.rs`, `99_apply_dedup.rs`) から
//! `#[path = "dedup_common.rs"] mod dedup_common;` のパターンで読み込まれる。

#![allow(dead_code)]

use unicode_normalization::UnicodeNormalization;

/// 選択肢値・正解値などのテキストを正規化する。
///
/// - Unicode NFKC 正規化 (全角→半角、半角カナ→全角カナ等)
/// - 前後空白 trim
/// - 中間空白は保持 (JLPTでは意味を持つケースがあるため)
///
/// ひらがな/漢字の表記揺れは**意図的に吸収しない** (JLPTの出題要素のため)。
pub fn normalize_text(s: &str) -> String {
    s.nfkc().collect::<String>().trim().to_string()
}

/// dedup 判定に必要な SubQuestion の部分ビュー。
///
/// `bin/utils.rs` の `SubQuestion` とは独立に定義し、テストしやすくする。
/// 実際の呼び出し側で `SubQuestion` から変換する。
pub struct SubLike {
    /// (key, value) のペア。順不同。
    pub options: Vec<(String, String)>,
    /// 正解のキー (例: "1"〜"4")
    pub answer: String,
}

/// `dedup_key` が `Err` で返す理由。caller 側で分類に利用できる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySkipReason {
    /// 正規化後の選択肢値がすべて "1","2","3","4" (並び替え問題のプレースホルダ)
    NumericPlaceholder,
    /// `answer` キーに対応する value が選択肢に存在しない、または選択肢が空
    AnswerNotInOptions,
}

/// dedup キーを生成する。
///
/// 形式: `"L{level_id}|OPT[{sorted_normalized_values}]|ANS[{normalized_answer_value}]"`
///
/// 以下のケースでは `Err(KeySkipReason)` を返す (dedup 対象外):
/// - `NumericPlaceholder`: 選択肢値が正規化後すべて `"1"`, `"2"`, `"3"`, `"4"` (並び替え問題)
/// - `AnswerNotInOptions`: `answer` キーに対応する value が選択肢に存在しない
///
/// ## 制約
/// Caller は正規化後の選択肢値および正解値が `,`, `|`, `[`, `]` を含まないことを保証すること。
/// これらは key format の delimiter として使われ、エスケープされない。JLPT 問題データでは
/// 通常これらが含まれることはないが、将来的にデータソースが変わる場合は検証が必要。
pub fn dedup_key(level_id: u32, sub: &SubLike) -> Result<String, KeySkipReason> {
    // すべての選択肢値を正規化
    let normalized: Vec<(String, String)> = sub.options.iter()
        .map(|(k, v)| (k.clone(), normalize_text(v)))
        .collect();

    // 値を sort
    let mut sorted_values: Vec<String> = normalized.iter().map(|(_, v)| v.clone()).collect();
    sorted_values.sort();

    // '1'〜'4' のみからなる選択肢は除外
    if sorted_values == vec!["1".to_string(), "2".to_string(), "3".to_string(), "4".to_string()] {
        return Err(KeySkipReason::NumericPlaceholder);
    }

    // answer キーから value を引く
    let answer_value = normalized.iter()
        .find(|(k, _)| k == &sub.answer)
        .map(|(_, v)| v.clone())
        .ok_or(KeySkipReason::AnswerNotInOptions)?;

    Ok(format!(
        "L{}|OPT[{}]|ANS[{}]",
        level_id,
        sorted_values.join(","),
        answer_value
    ))
}

use chrono::{DateTime, Utc};

/// tiebreaker のための候補情報。
#[derive(Clone, Debug)]
pub struct Candidate {
    pub parent_id: String,
    pub sub_idx: usize,
    pub create_time: DateTime<Utc>,
    pub sentence_len: usize,
}

/// 重複グループ内で「残すレコード」を決めるための比較関数。
///
/// 優先順位: createTime 古い方 → sentence 長い方 → parent_id 辞書順
///
/// `Vec::sort_by(prefer_keep_order)` でソートすると、先頭が「残すべきレコード」になる。
pub fn prefer_keep_order(a: &Candidate, b: &Candidate) -> std::cmp::Ordering {
    // 1. createTime 古い方を先頭に
    match a.create_time.cmp(&b.create_time) {
        std::cmp::Ordering::Equal => {}
        ord => return ord,
    }
    // 2. sentence 長い方を先頭に (降順なので b.len.cmp(a.len))
    match b.sentence_len.cmp(&a.sentence_len) {
        std::cmp::Ordering::Equal => {}
        ord => return ord,
    }
    // 3. parent_id 辞書順
    a.parent_id.cmp(&b.parent_id)
}
