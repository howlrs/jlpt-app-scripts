#[path = "../bin/dedup_common.rs"]
mod dedup_common;

use dedup_common::normalize_text;

#[test]
fn normalize_text_trims_whitespace() {
    assert_eq!(normalize_text("  hello  "), "hello");
}

#[test]
fn normalize_text_preserves_middle_spaces() {
    // 設計上、中間空白は trim しない (単語区切りとして意味を持つ場合がある)
    assert_eq!(normalize_text("hello world"), "hello world");
}

#[test]
fn normalize_text_nfkc_fullwidth_digit() {
    // 全角 "１" → 半角 "1"
    assert_eq!(normalize_text("１"), "1");
}

#[test]
fn normalize_text_nfkc_halfwidth_kana() {
    // 半角カナ "ｱ" → 全角 "ア" (NFKC は半角カナを全角に合成)
    assert_eq!(normalize_text("ｱ"), "ア");
}

#[test]
fn normalize_text_preserves_hiragana_kanji() {
    assert_eq!(normalize_text("役割"), "役割");
    assert_eq!(normalize_text("やくわり"), "やくわり");
}

#[test]
fn normalize_text_empty() {
    assert_eq!(normalize_text(""), "");
    assert_eq!(normalize_text("   "), "");
}
