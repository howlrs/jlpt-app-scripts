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

use dedup_common::{dedup_key, KeySkipReason, SubLike};

fn mk_sub(options: &[(&str, &str)], answer: &str) -> SubLike {
    SubLike {
        options: options.iter().map(|(k,v)| (k.to_string(), v.to_string())).collect(),
        answer: answer.to_string(),
    }
}

#[test]
fn dedup_key_basic() {
    let sub = mk_sub(&[("1","役割"),("2","役目"),("3","配役"),("4","役者")], "1");
    let key = dedup_key(5, &sub).expect("should produce a key");
    // sorted option values + normalized answer value + level
    assert_eq!(key, "L5|OPT[役割,役目,役者,配役]|ANS[役割]");
}

#[test]
fn dedup_key_same_options_different_answer_produces_different_keys() {
    let sub_a = mk_sub(&[("1","が"),("2","で"),("3","に"),("4","を")], "1");
    let sub_b = mk_sub(&[("1","が"),("2","で"),("3","に"),("4","を")], "4");
    let key_a = dedup_key(5, &sub_a).unwrap();
    let key_b = dedup_key(5, &sub_b).unwrap();
    assert_ne!(key_a, key_b);
}

#[test]
fn dedup_key_excludes_numeric_options() {
    // category=9 並び替え問題は選択肢が '1','2','3','4' で値が意味を持たないため NumericPlaceholder を返す
    let sub = mk_sub(&[("1","1"),("2","2"),("3","3"),("4","4")], "1");
    assert_eq!(dedup_key(3, &sub), Err(dedup_common::KeySkipReason::NumericPlaceholder));
}

#[test]
fn dedup_key_excludes_numeric_options_after_shuffle() {
    // key と value が食い違っていても値がすべて '1'〜'4' ならスキップ
    let sub = mk_sub(&[("1","3"),("2","1"),("3","4"),("4","2")], "2");
    assert_eq!(dedup_key(3, &sub), Err(dedup_common::KeySkipReason::NumericPlaceholder));
}

#[test]
fn dedup_key_returns_answer_not_in_options_when_missing() {
    // answer="5" だが選択肢に key="5" がない → AnswerNotInOptions
    let sub = mk_sub(&[("1","a"),("2","b"),("3","c"),("4","d")], "5");
    assert_eq!(dedup_key(1, &sub), Err(dedup_common::KeySkipReason::AnswerNotInOptions));
}

#[test]
fn dedup_key_respects_level_id() {
    let sub = mk_sub(&[("1","a"),("2","b"),("3","c"),("4","d")], "1");
    let k1 = dedup_key(1, &sub).unwrap();
    let k2 = dedup_key(2, &sub).unwrap();
    assert_ne!(k1, k2);
}

#[test]
fn dedup_key_normalizes_fullwidth() {
    let sub_half = mk_sub(&[("1","1A"),("2","2B"),("3","3C"),("4","4D")], "1");
    let sub_full = mk_sub(&[("1","１Ａ"),("2","２Ｂ"),("3","３Ｃ"),("4","４Ｄ")], "1");
    // ただし値が全て '1'〜'4' で始まるが "1A" など別文字列なので除外ルールには該当しない
    assert_eq!(dedup_key(1, &sub_half), dedup_key(1, &sub_full));
}

#[test]
fn dedup_key_is_order_independent() {
    // 選択肢の入力順が違っても同じキーを生成する (sort してから結合するため)
    let sub1 = mk_sub(&[("1","役割"),("2","役目"),("3","配役"),("4","役者")], "1");
    let sub2 = mk_sub(&[("4","役割"),("3","役目"),("2","配役"),("1","役者")], "4"); // answer も同じ "役割" を指す
    assert_eq!(dedup_key(5, &sub1).unwrap(), dedup_key(5, &sub2).unwrap());
}

#[test]
fn dedup_key_empty_options_returns_answer_not_in_options() {
    // I-1 からの派生: options が空の場合の挙動を明示
    let sub = SubLike { options: vec![], answer: "1".to_string() };
    assert_eq!(dedup_key(1, &sub), Err(KeySkipReason::AnswerNotInOptions));
}

use chrono::{TimeZone, Utc};
use dedup_common::{prefer_keep_order, Candidate};

#[test]
fn prefer_keep_order_older_create_time_wins() {
    let older = Candidate {
        parent_id: "z".into(),
        sub_idx: 0,
        create_time: Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
        sentence_len: 10,
    };
    let newer = Candidate {
        parent_id: "a".into(),
        sub_idx: 0,
        create_time: Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap(),
        sentence_len: 100,
    };
    let mut v = vec![newer.clone(), older.clone()];
    v.sort_by(prefer_keep_order);
    assert_eq!(v[0].parent_id, "z"); // older wins
}

#[test]
fn prefer_keep_order_same_time_longer_sentence_wins() {
    let time = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
    let short = Candidate {
        parent_id: "z".into(), sub_idx: 0, create_time: time, sentence_len: 10,
    };
    let long = Candidate {
        parent_id: "a".into(), sub_idx: 0, create_time: time, sentence_len: 100,
    };
    let mut v = vec![short.clone(), long.clone()];
    v.sort_by(prefer_keep_order);
    assert_eq!(v[0].parent_id, "a"); // longer sentence wins
}

#[test]
fn prefer_keep_order_same_time_same_length_lexical_parent_id() {
    let time = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
    let a = Candidate { parent_id: "a".into(), sub_idx: 0, create_time: time, sentence_len: 10 };
    let b = Candidate { parent_id: "b".into(), sub_idx: 0, create_time: time, sentence_len: 10 };
    let mut v = vec![b.clone(), a.clone()];
    v.sort_by(prefer_keep_order);
    assert_eq!(v[0].parent_id, "a"); // lexical order
}
