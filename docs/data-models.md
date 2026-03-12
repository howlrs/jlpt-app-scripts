# jlpt-app-scripts データ構造

## コアデータモデル

### Question

```rust
struct Question {
    id: Option<String>,               // UUID v4（Script 3で採番）
    level_id: u32,                    // 1-5（Script 4で設定）
    level_name: String,               // "n1"〜"n5"（AI生成時に設定）
    category_id: Option<String>,      // カテゴリ識別子
    category_name: String,            // カテゴリ名（"文法", "語彙" 等）
    chapter: String,                  // 章・セクション
    sentence: String,                 // 大問の問題文
    prerequisites: Option<String>,    // 前提条件・コンテキスト
    sub_questions: Vec<SubQuestion>,  // 小問リスト
}
```

### SubQuestion

```rust
struct SubQuestion {
    id: u32,                          // 連番（Script 3で採番）
    hint_id: u32,                     // ヒント参照ID（デフォルト: 0）
    answer_id: u32,                   // 回答参照ID（デフォルト: 0）
    sentence: Option<String>,         // 小問文
    prerequisites: Option<String>,    // 小問の前提条件
    select_answer: Vec<SelectAnswer>, // 選択肢（4択）
    answer: String,                   // 正解（"1"〜"4"）
    voted: Option<i32>,               // ユーザー評価（オプション）
}
```

### SelectAnswer

```rust
struct SelectAnswer {
    key: String,    // 選択肢番号 ("1"〜"4")
    value: String,  // 選択肢テキスト
}
```

### CatValue（カテゴリメタデータ）

```rust
struct CatValue {
    level_id: u32,   // レベルID (1-5)
    id: u32,         // カテゴリID
    name: String,    // カテゴリ名
}
```

## データ変換の流れ

```
[AI生成 JSON]
  ├── id: なし
  ├── level_id: 0
  ├── level_name: "n3"
  └── sub_questions[].id: 0
         │
         ▼ Script 1: パース
[構造化 JSON]
  └── Rustの型に適合
         │
         ▼ Script 2: 重複排除
[ユニーク JSON]
  └── SubQuestion.sentence 単位で重複除去
         │
         ▼ Script 3: ID採番
[採番済み JSON]
  ├── id: "550e8400-e29b-41d4-a716-446655440000"
  └── sub_questions[].id: 1, 2, 3...
         │
         ▼ Script 4: レベリング
[最終 JSON]
  └── level_id: 3  ("n3" → 3)
```

## Firestoreコレクション

### `questions` コレクション

パイプライン最終出力。`Question` 構造体をそのまま格納。
ドキュメントIDは `Question.id`（UUID v4）。

### `categories_raw` コレクション

カテゴリメタデータ。`CatValue` を格納。
ドキュメントIDは自動生成UUID v4。
