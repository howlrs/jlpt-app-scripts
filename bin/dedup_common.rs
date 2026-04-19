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

/// dedup キーを生成する。
///
/// 形式: `"L{level_id}|OPT[{sorted_normalized_values}]|ANS[{normalized_answer_value}]"`
///
/// 以下のケースでは `None` を返す (dedup 対象外):
/// - 選択肢値が正規化後すべて `"1"`, `"2"`, `"3"`, `"4"` (並び替え問題のプレースホルダ)
/// - `answer` キーに対応する value が選択肢に存在しない (不正データ)
pub fn dedup_key(level_id: u32, sub: &SubLike) -> Option<String> {
    // すべての選択肢値を正規化
    let normalized: Vec<(String, String)> = sub.options.iter()
        .map(|(k, v)| (k.clone(), normalize_text(v)))
        .collect();

    // 値を sort
    let mut sorted_values: Vec<String> = normalized.iter().map(|(_, v)| v.clone()).collect();
    sorted_values.sort();

    // '1'〜'4' のみからなる選択肢は除外
    if sorted_values == vec!["1".to_string(), "2".to_string(), "3".to_string(), "4".to_string()] {
        return None;
    }

    // answer キーから value を引く
    let answer_value = normalized.iter()
        .find(|(k, _)| k == &sub.answer)
        .map(|(_, v)| v.clone())?;

    Some(format!(
        "L{}|OPT[{}]|ANS[{}]",
        level_id,
        sorted_values.join(","),
        answer_value
    ))
}
