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
