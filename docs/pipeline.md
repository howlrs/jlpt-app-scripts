# jlpt-app-scripts パイプライン仕様

## パイプライン全体像

```
[0] AI問題生成 ──▶ [1] JSON構造化 ──▶ [2] 重複排除 ──▶ [3] ID採番
                                                              │
[99] DB投入 ◀── [5] カテゴリ抽出 ◀── [4] レベル正規化 ◀──────┘
```

各スクリプトは独立したバイナリとして順番に実行する。

## スクリプト詳細

### Script 0: `create_questions` (0_create_questions.rs)

**機能:** Google Gemini APIを使ったJLPT問題の自動生成

**処理内容:**
1. 環境変数 `GOOGLE_GEMINI_API_KEY`, `GEMINI_MODELS` を読込
2. プロンプトファイルを読込：
   - `prompts/create-question_to_json.md` - 生成指示
   - `prompts/base-info.md` - JLPT基本情報
   - `prompts/{level}/ja-question.md` - レベル別詳細
3. N1〜N5の各レベルに対し1000回のAPIリクエスト
4. 20秒間隔でリクエスト、失敗時120秒待機後リトライ
5. `output/questions/{level}/{timestamp}.json` に保存

**出力例:** `output/questions/n3/1709744000.json`

---

### Script 1: `json_read_to_struct` (1_json_read_to_struct.rs)

**機能:** 生成JSONの構造体パース・バリデーション

**処理内容:**
1. `concat_all.json`（結合済みファイル）を読込
2. `Vec<Question>` にデシリアライズ
3. AIのマークダウン記法（\`\`\`json等）を除去
4. パース失敗ファイルは `err/` ディレクトリにコピー
5. 検証済みデータを `concat_with_struct.json` に保存

---

### Script 2: `duplicate` (2_duplicate.rs)

**機能:** 小問の文面に基づく重複排除

**処理内容:**
1. `concat_with_struct.json` を読込
2. `SubQuestion.sentence` をキーにHashMapで重複検出
3. 重複除去後のデータを `removed_duplicate_rows_concat_all.json` に保存

---

### Script 3: `numbering` (3_numbering.rs)

**機能:** UUID/連番によるID採番

**処理内容:**
1. 重複除去済みデータを読込
2. 各 `Question.id` にUUID v4を割当
3. 各 `SubQuestion.id` に連番を割当
4. `3_numbering_data.json` に保存

---

### Script 4: `leveling` (4_leveling.rs)

**機能:** レベル名→レベルIDの正規化

**処理内容:**
1. 採番済みデータを読込
2. `level_name` ("n1"〜"n5") を `level_id` (1〜5) に変換
3. `4_leveling_data.json` に保存

**変換マップ:**

| level_name | level_id |
|-----------|----------|
| n1 | 1 |
| n2 | 2 |
| n3 | 3 |
| n4 | 4 |
| n5 | 5 |

---

### Script 5: `categories_to_meta` (5_categories_to_meta.rs)

**機能:** カテゴリメタデータの抽出・集約

**処理内容:**
1. レベリング済みデータを読込
2. `level_id` + `category_id` のユニーク組合せを集約
3. `5_categories_meta.json` に保存

---

### Script 99a: `questions_to_database` (99_questions_to_database.rs)

**機能:** 問題データのFirestoreバッチ投入

**処理内容:**
1. レベリング済みデータを読込
2. Firestore `questions` コレクションにバッチ挿入
3. 失敗レコードは `err/save_err_to_db.json` に記録

---

### Script 99b: `categories_to_database` (99_categories_to_database.rs)

**機能:** カテゴリメタデータのFirestore投入

**処理内容:**
1. カテゴリメタデータを読込
2. Firestore `categories_raw` コレクションに挿入
3. 各ドキュメントIDにUUID v4を使用

## 実行順序

```bash
cargo run --bin create_questions     # 0. AI生成
cargo run --bin template             # 0.5. ファイル結合
cargo run --bin json_read_to_struct  # 1. 構造化
cargo run --bin duplicate            # 2. 重複排除
cargo run --bin numbering            # 3. ID採番
cargo run --bin leveling             # 4. レベル正規化
cargo run --bin categories_to_meta   # 5. カテゴリ抽出
cargo run --bin questions_to_database    # 99a. 問題DB投入
cargo run --bin categories_to_database   # 99b. カテゴリDB投入
```

## 出力ファイル一覧

各レベルディレクトリ（`output/questions/{level}/`）配下：

| ファイル名 | 生成スクリプト | 内容 |
|-----------|--------------|------|
| `{timestamp}.json` | Script 0 | API生レスポンス |
| `concat_with_struct.json` | Script 1 | 構造化済みJSON |
| `removed_duplicate_rows_concat_all.json` | Script 2 | 重複除去済み |
| `3_numbering_data.json` | Script 3 | ID採番済み |
| `4_leveling_data.json` | Script 4 | レベル正規化済み |
| `5_categories_meta.json` | Script 5 | カテゴリメタ |
| `err/` | Script 1, 99 | エラーファイル |
