# jlpt-app-scripts パイプライン仕様

## パイプライン全体像

```
[0] AI問題生成 ──▶ [1] JSONパース ──▶ [1.5] バリデーション ──▶ [2] 類似排除
                                                                      │
[99] DB投入 ◀── [5] カテゴリ抽出 ◀── [4] レベル正規化 ◀── [3] ID採番 ◀┘
```

パイプライン全体は `run_pipeline.sh` で一括実行できる。各スクリプトは独立したバイナリとして順番に実行する。

## 生成方式

### カテゴリベース生成

問題生成はカテゴリベースで行う。各レベルに定義されたカテゴリ（文法、語彙、読解など）ごとにプロンプトを構成し、カテゴリに特化した問題を生成する。これにより、カテゴリ分布の偏りを防ぎ、均質な問題セットを生成する。

### モデルフォールバック

`GEMINI_MODELS` 環境変数にカンマ区切りで2つのモデルを指定する。プライマリモデルが失敗した場合、セカンダリモデルにフォールバックする。各問題の `generated_by` フィールドに実際に使用されたモデル名が記録される。

### チェックポイント/リジューム

問題生成（Script 0）はチェックポイント機能を備える。中断しても既に生成済みのファイルをスキップし、未完了分から再開できる。

## スクリプト詳細

### Script 0: `create_questions` (0_create_questions.rs)

**機能:** Google Gemini APIを使ったJLPT問題のカテゴリベース自動生成

**処理内容:**
1. 環境変数 `GOOGLE_GEMINI_API_KEY`, `GEMINI_MODELS`, `GENERATE_COUNT`, `REQUEST_INTERVAL` を読込
2. プロンプトファイルを読込：
   - `prompts/create-question_to_json.md` - 生成指示
   - `prompts/base-info.md` - JLPT基本情報
   - `prompts/{level}/ja-question.md` - レベル別詳細
3. N1〜N5の各レベル・各カテゴリに対しAPIリクエスト
4. `REQUEST_INTERVAL` 秒間隔でリクエスト、失敗時フォールバックモデルに切替、さらに失敗時120秒待機後リトライ
5. `output/questions/{level}/{timestamp}.json` に保存
6. 各問題に `generated_by` フィールドとしてモデル名を記録

**出力例:** `output/questions/n3/1709744000.json`

---

### Script 1: `json_read_to_struct` (1_json_read_to_struct.rs)

**機能:** 生成JSONの構造体パース

**処理内容:**
1. 個別JSONファイルを読込
2. `Vec<Question>` にデシリアライズ
3. AIのマークダウン記法（\`\`\`json等）を除去
4. パース失敗ファイルは `err/` ディレクトリにコピー
5. パース済みデータを `1_parsed.json` に保存

---

### Script 1.5: バリデーション

**機能:** パース済みデータの構造検証

**処理内容:**
1. `1_parsed.json` を読込
2. 必須フィールドの存在確認、型チェック
3. 検証済みデータを `1_5_validated.json` に保存

---

### Script 2: `duplicate` (2_duplicate.rs)

**機能:** Levenshtein距離に基づく類似排除

**処理内容:**
1. `1_5_validated.json` を読込
2. `SubQuestion.sentence` 間のLevenshtein距離を計算
3. 類似度85%以上のペアを検出し、後方の問題を除去
4. 類似排除後のデータを `2_deduplicated.json` に保存

---

### Script 3: `numbering` (3_numbering.rs)

**機能:** UUID/連番によるID採番

**処理内容:**
1. 類似排除済みデータを読込
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

---

### Script: `check_db`

**機能:** Firestoreの既存データ確認

**処理内容:**
1. Firestoreの `questions` コレクションを読込
2. レベル・カテゴリごとの問題数を集計・表示
3. データの整合性を確認

---

### Script: `clear_and_replace`

**機能:** Firestoreデータの全削除・再投入

**処理内容:**
1. `DRY_RUN` 環境変数で安全確認（デフォルト: true）
2. DRY_RUN=true の場合、削除・投入の対象件数のみ表示
3. DRY_RUN=false の場合、既存データを全削除後、パイプライン出力データを再投入

## 実行方法

### 一括実行（推奨）

```bash
./run_pipeline.sh
```

### 個別実行

```bash
cargo run --bin create_questions         # 0. AI生成（カテゴリベース）
cargo run --bin json_read_to_struct      # 1. パース → 1_parsed.json
                                         # 1.5. バリデーション → 1_5_validated.json
cargo run --bin duplicate                # 2. 類似排除 → 2_deduplicated.json
cargo run --bin numbering                # 3. ID採番
cargo run --bin leveling                 # 4. レベル正規化
cargo run --bin categories_to_meta       # 5. カテゴリ抽出
cargo run --bin questions_to_database    # 99a. 問題DB投入
cargo run --bin categories_to_database   # 99b. カテゴリDB投入
```

### DB操作

```bash
cargo run --bin check_db                 # DB内容確認
DRY_RUN=true cargo run --bin clear_and_replace   # 置換プレビュー
DRY_RUN=false cargo run --bin clear_and_replace  # 実際に置換実行
```

## 出力ファイル一覧

各レベルディレクトリ（`output/questions/{level}/`）配下：

| ファイル名 | 生成スクリプト | 内容 |
|-----------|--------------|------|
| `{timestamp}.json` | Script 0 | API生レスポンス |
| `1_parsed.json` | Script 1 | パース済みJSON |
| `1_5_validated.json` | Script 1.5 | バリデーション済み |
| `2_deduplicated.json` | Script 2 | 類似排除済み |
| `3_numbering_data.json` | Script 3 | ID採番済み |
| `4_leveling_data.json` | Script 4 | レベル正規化済み |
| `5_categories_meta.json` | Script 5 | カテゴリメタ |
| `err/` | Script 1, 99 | エラーファイル |
