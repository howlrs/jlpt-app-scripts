use std::collections::HashSet;

use log::{info, warn};
use serde::{Deserialize, Serialize};

mod utils;
use crate::utils::{
    read_questions_from_stage, write_file, write_questions_to_stage, level_dir,
    Question, LEVELS, STAGE_1_OUTPUT, STAGE_1_5_VALIDATED, STAGE_1_5_REJECTED,
};

/// Question構造体にリジェクト理由を付与した構造体
#[derive(Serialize, Deserialize, Debug)]
struct RejectedQuestion {
    question: Question,
    reasons: Vec<String>,
}

/// Read parsed questions from stage 1, validate each Question,
/// write validated to STAGE_1_5_VALIDATED and rejected to STAGE_1_5_REJECTED.
fn main() {
    crate::utils::init_logger();

    let start = std::time::Instant::now();

    for level in LEVELS {
        let questions = match read_questions_from_stage(level, STAGE_1_OUTPUT) {
            Ok(q) => q,
            Err(e) => {
                warn!("level {}: {}", level, e);
                continue;
            }
        };

        if questions.is_empty() {
            warn!("level {}: no questions found, skipping", level);
            continue;
        }

        let total = questions.len();
        let mut validated: Vec<Question> = Vec::new();
        let mut rejected: Vec<RejectedQuestion> = Vec::new();
        let mut reason_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for question in questions {
            let reasons = validate_question(&question);
            if reasons.is_empty() {
                validated.push(question);
            } else {
                for reason in &reasons {
                    *reason_counts.entry(reason.clone()).or_insert(0) += 1;
                }
                rejected.push(RejectedQuestion { question, reasons });
            }
        }

        // Summary log
        info!("=== level {} validation summary ===", level);
        info!(
            "total: {}, passed: {}, rejected: {}",
            total,
            validated.len(),
            rejected.len()
        );
        if !reason_counts.is_empty() {
            info!("rejection reasons breakdown:");
            let mut sorted_reasons: Vec<_> = reason_counts.iter().collect();
            sorted_reasons.sort_by(|a, b| b.1.cmp(a.1));
            for (reason, count) in sorted_reasons {
                info!("  {}: {}", reason, count);
            }
        }

        // Write validated questions
        match write_questions_to_stage(level, STAGE_1_5_VALIDATED, &validated) {
            Ok(_) => info!("level {}: wrote {}", level, STAGE_1_5_VALIDATED),
            Err(e) => warn!("level {}: failed to write validated: {}", level, e),
        }

        // Write rejected questions (custom struct, use write_file directly)
        let rejected_json = serde_json::to_string_pretty(&rejected).unwrap();
        let rejected_path = level_dir(level).join(STAGE_1_5_REJECTED);
        write_file(rejected_path, &rejected_json);
    }

    info!("done, elapsed: {:?}", start.elapsed());
}

/// Questionに対するバリデーションを実行し、失敗理由のリストを返す
/// 空のリストが返れば全バリデーション通過
fn validate_question(question: &Question) -> Vec<String> {
    let mut reasons = Vec::new();

    // required fields: sentence must not be empty
    if question.sentence.trim().is_empty() {
        reasons.push("question sentence is empty".to_string());
    }

    // required fields: category_name must not be empty
    if question.category_name.trim().is_empty() {
        reasons.push("category_name is empty".to_string());
    }

    // Phase 11: parent.sentence に HTML タグ or 「（　　）」以外の空括弧を含まない
    if contains_html_tag(&question.sentence) {
        reasons.push("question sentence contains HTML tags (<u>/</u> etc.)".to_string());
    }
    if contains_bad_empty_paren(&question.sentence) {
        reasons.push("question sentence contains non-canonical empty parentheses (use '（　　）')".to_string());
    }

    // category_id別チェック: 漢字読み(2)・表記(3)で空括弧はNG
    let cat_id_num = question.category_id.as_deref().unwrap_or("0")
        .parse::<u32>().unwrap_or(0);

    for (i, sub_q) in question.sub_questions.iter().enumerate() {
        let sub_label = format!("sub_question[{}]", i);

        // Phase 11: sub.sentence に HTML タグ or 非標準空括弧を含まない
        if let Some(sent) = &sub_q.sentence {
            if contains_html_tag(sent) {
                reasons.push(format!("{}: sentence contains HTML tags", sub_label));
            }
            if contains_bad_empty_paren(sent) {
                reasons.push(format!("{}: sentence contains non-canonical empty parentheses (use '（　　）')", sub_label));
            }
        }

        // 漢字読み・表記カテゴリで空括弧チェック
        if cat_id_num == 2 || cat_id_num == 3 {
            let sent = sub_q.sentence.as_deref().unwrap_or("");
            if sent.contains("（　　）") || sent.contains("（）")
                || sent.contains("（  ）") || sent.contains("（ ）")
            {
                reasons.push(format!(
                    "{}: empty parentheses in kanji reading/notation category",
                    sub_label
                ));
            }
        }

        // select_answer count: each SubQuestion must have exactly 4 options
        if sub_q.select_answer.len() != 4 {
            reasons.push(format!(
                "{}: select_answer count is {} (expected 4)",
                sub_label,
                sub_q.select_answer.len()
            ));
        }

        // answer range: answer must be "1", "2", "3", or "4"
        let valid_answers = ["1", "2", "3", "4"];
        if !valid_answers.contains(&sub_q.answer.as_str()) {
            reasons.push(format!(
                "{}: answer '{}' is not in valid range 1-4",
                sub_label, sub_q.answer
            ));
        }

        // answer-option consistency: answer field value must correspond to an existing key
        let answer_key_exists = sub_q
            .select_answer
            .iter()
            .any(|sa| sa.key == sub_q.answer);
        if !answer_key_exists {
            reasons.push(format!(
                "{}: answer '{}' does not match any key in select_answer",
                sub_label, sub_q.answer
            ));
        }

        // non-empty options: all select_answer values must be non-empty strings
        for (j, sa) in sub_q.select_answer.iter().enumerate() {
            if sa.value.trim().is_empty() {
                reasons.push(format!(
                    "{}: select_answer[{}] key '{}' has empty value",
                    sub_label, j, sa.key
                ));
            }
        }

        // Issue #16: 並び替え問題 (category=9) 以外で、選択肢 value が数字のみ ("1","2","3","4") は不正
        // 並び替え問題は value が位置番号を表すため例外
        if cat_id_num != 9 && sub_q.select_answer.len() == 4 {
            let mut sorted_values: Vec<String> = sub_q.select_answer.iter()
                .map(|sa| sa.value.trim().to_string())
                .collect();
            sorted_values.sort();
            if sorted_values == vec!["1".to_string(), "2".to_string(), "3".to_string(), "4".to_string()] {
                reasons.push(format!(
                    "{}: select_answer values are all numeric placeholders ({:?}) — incorrect format for category_id={}",
                    sub_label, sorted_values, cat_id_num
                ));
            }
        }

        // no duplicate options: select_answer values must all be unique within a SubQuestion
        let mut seen_values = HashSet::new();
        for sa in &sub_q.select_answer {
            if !seen_values.insert(sa.value.trim().to_string()) {
                reasons.push(format!(
                    "{}: duplicate select_answer value '{}'",
                    sub_label, sa.value
                ));
            }
        }

        // Phase 12: 並び替え問題 (cat=9) で選択肢 value が「N-N-N-N」形式のみ = 語句不在
        // 正当な並び替え問題は value に語句テキストが入り、key に並び順番号が入る
        if cat_id_num == 9 && sub_q.select_answer.len() == 4 {
            let all_numeric_seq = sub_q.select_answer.iter().all(|sa| {
                let v = sa.value.trim();
                // 「N-N-N-N」 or 純粋な数字のみ
                v.chars().all(|c| c.is_ascii_digit() || c == '-' || c.is_whitespace())
            });
            if all_numeric_seq {
                reasons.push(format!(
                    "{}: 並び替え問題 (cat=9) の選択肢 value が数字列のみ。並べるべき語句データが欠落している",
                    sub_label
                ));
            }
        }

        // Phase 12: 漢字読み問題 (cat=2) で sub.sentence に漢字 (CJK Unified Ideographs) がない
        if cat_id_num == 2 {
            let sent = sub_q.sentence.as_deref().unwrap_or("");
            let has_kanji = sent.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
            if !has_kanji {
                reasons.push(format!(
                    "{}: 漢字読み問題 (cat=2) に漢字がない - 読み方を問えない",
                    sub_label
                ));
            }
        }

        // Phase 12: 読解・聴解系 (cat=10-13 読解 / 14-17 聴解) で本文・会話が parent.prerequisites に
        // 存在しない場合は欠陥データ。cat=18 発話表現、cat=19 即時応答は短文で OK なので除外
        if matches!(cat_id_num, 10..=17) {
            let parent_has_content = !question.prerequisites.as_deref().unwrap_or("").trim().is_empty()
                || question.sentence.chars().count() > 40;  // 指示文以上の長さなら本文扱い
            let sub_has_content = sub_q.sentence.as_deref()
                .map(|s| s.chars().count() > 40)
                .unwrap_or(false);
            if !parent_has_content && !sub_has_content {
                reasons.push(format!(
                    "{}: 読解/聴解問題 (cat={}) に本文/会話データがない",
                    sub_label, cat_id_num
                ));
            }
        }

        // Phase 10: レベル一貫性チェック (初級 N5/N4 向け)
        // 選択肢の文字数差が大きい or 長すぎる問題は、初級レベルとして不適切
        // (N5/N4 では選択肢長が揃っていて短いのが JLPT 公式の特徴)
        let level_name_upper = question.level_name.to_uppercase();
        let is_beginner = level_name_upper == "N5" || level_name_upper == "N4";
        if is_beginner && sub_q.select_answer.len() == 4 {
            let lens: Vec<usize> = sub_q.select_answer.iter()
                .map(|sa| sa.value.chars().count())
                .collect();
            let max_len = lens.iter().max().copied().unwrap_or(0);
            let min_len = lens.iter().min().copied().unwrap_or(0);
            // 初級では選択肢長の差が 5 char 以上あると中級以上の文法が混入している可能性
            if max_len.saturating_sub(min_len) >= 5 {
                reasons.push(format!(
                    "{}: 初級({})の選択肢文字数差が大きい (max={}, min={}, diff={}) - 中級以上の文法混入の疑い",
                    sub_label, level_name_upper, max_len, min_len, max_len - min_len
                ));
            }
            // 初級では 1 つの選択肢が 12 char を超えると長すぎる
            if max_len >= 12 {
                reasons.push(format!(
                    "{}: 初級({})の選択肢が長すぎる (max={} chars) - 中級以上の表現混入の疑い",
                    sub_label, level_name_upper, max_len
                ));
            }
        }
    }

    reasons
}

/// Phase 11: 文字列に HTMLタグ (<u>, </u>, <b>, <i> 等) が含まれているかチェック.
/// AI生成で付与された装飾タグは、フロントで JSX エスケープされて素のテキストとして
/// ユーザに見えてしまうため、生成時点で拒否する。
fn contains_html_tag(s: &str) -> bool {
    // <[a-zA-Z] で始まり > で終わる部分を検索
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            // 次が '/' or ASCII アルファベット なら HTMLタグ候補
            if let Some(&next) = chars.peek() {
                if next == '/' || next.is_ascii_alphabetic() {
                    // '>' まで読み進めて閉じるかチェック
                    for c2 in chars.by_ref() {
                        if c2 == '>' {
                            return true;
                        }
                        // タグ内に '<' が出たらタグでない
                        if c2 == '<' {
                            break;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Phase 11: 「（　　）」(全角括弧+全角スペース×2) 以外の空括弧パターンを検出.
/// 例: 「（）」「（ ）」「（　）」「( )」など、ユーザに見える穴埋め形式の揺れ。
fn contains_bad_empty_paren(s: &str) -> bool {
    // 全角: 「（」+ 空白0〜1文字 + 「）」 は NG (2文字空白が正規)
    // 半角: 「(」+ 空白0〜2文字 + 「)」も NG
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '（' {
            // 次に 0, 1, 2 文字の空白 (全角/半角) を許容しながら '）' を探す
            let mut j = i + 1;
            let mut spaces = 0;
            while j < n && (chars[j] == ' ' || chars[j] == '\u{3000}') {
                spaces += 1;
                j += 1;
            }
            if j < n && chars[j] == '）' {
                // 空括弧発見。スペース数が 2 (全角2個) 以外は NG
                if spaces != 2 {
                    return true;
                }
                // スペース内容が全角×2 ちょうどでないと NG
                // (例えば全角1+半角1 混在も NG)
                let all_ideographic = chars[i+1..j].iter().all(|&c| c == '\u{3000}');
                if !all_ideographic {
                    return true;
                }
            }
            i = j.max(i + 1);
        } else if c == '(' {
            let mut j = i + 1;
            while j < n && (chars[j] == ' ' || chars[j] == '\u{3000}') {
                j += 1;
            }
            if j < n && chars[j] == ')' {
                return true; // 半角括弧は常に NG
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    false
}
