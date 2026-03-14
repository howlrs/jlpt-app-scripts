# jlpt-app-scripts セットアップガイド

## 前提条件

- Rust (Edition 2024 対応版)
- Google Gemini API キー
- Google Cloud プロジェクト（Firestore有効化済み）
- DB操作スクリプト使用時: `gcloud auth application-default login` による認証

## 環境変数

`.env` ファイルを作成：

| 変数名 | 必須 | 説明 |
|--------|------|------|
| `GOOGLE_GEMINI_API_KEY` | Yes | Google Gemini API の認証キー |
| `GEMINI_MODELS` | Yes | 使用するGeminiモデル名（カンマ区切り、2つ指定。1つ目がプライマリ、2つ目がフォールバック） |
| `PROJECT_ID` | Yes | Google Cloud プロジェクトID（Firestore用） |
| `GENERATE_COUNT` | No | レベル×カテゴリあたりの生成リクエスト数 |
| `REQUEST_INTERVAL` | No | APIリクエスト間隔（秒） |
| `DRY_RUN` | No | clear_and_replace用。trueでプレビューのみ（デフォルト: true） |
| `RUST_LOG` | No | ログレベル（デフォルト: INFO） |

## ビルド

```bash
cargo build --release
```

## DB操作の認証

Firestore操作（check_db, clear_and_replace, questions_to_database等）を実行する前に、アプリケーションデフォルト認証を設定する：

```bash
gcloud auth application-default login
```

## パイプライン実行

### 一括実行（推奨）

```bash
./run_pipeline.sh
```

### 個別実行

```bash
# 1. AI問題生成（カテゴリベース、チェックポイント/リジューム対応）
cargo run --bin create_questions

# 2. JSONパース → 1_parsed.json
cargo run --bin json_read_to_struct

# 3. バリデーション → 1_5_validated.json（パースに含まれる場合あり）

# 4. 類似排除（Levenshtein距離85%閾値） → 2_deduplicated.json
cargo run --bin duplicate

# 5. ID採番
cargo run --bin numbering

# 6. レベルID正規化
cargo run --bin leveling

# 7. カテゴリメタデータ抽出
cargo run --bin categories_to_meta

# 8. Firestoreに問題データ投入
cargo run --bin questions_to_database

# 9. Firestoreにカテゴリデータ投入
cargo run --bin categories_to_database
```

### DB確認・置換

```bash
# DB内容の確認（レベル・カテゴリごとの問題数を表示）
cargo run --bin check_db

# 置換プレビュー（削除・投入の対象件数のみ表示）
# ※ levels/categoriesコレクションも自動で投入されます
DRY_RUN=true cargo run --bin clear_and_replace

# 実際にDB全削除→再投入（levels, categories, categories_raw, questionsを一括管理）
DRY_RUN=false cargo run --bin clear_and_replace
```

### 投票データレビュー

```bash
# 投票集計のみ（bad率60%以上の問題をリスト化）
cargo run --bin review_votes

# 集計 + 低品質問題をFirestoreから削除
cargo run --bin review_votes -- --delete

# 閾値を変更して実行（デフォルト: 0.6）
BAD_THRESHOLD=0.5 cargo run --bin review_votes
```

## 出力ディレクトリ

パイプライン実行後、以下の構造で出力される：

```
output/questions/
├── n1/
│   ├── *.json                    # API生レスポンス
│   ├── 1_parsed.json             # パース済み
│   ├── 1_5_validated.json        # バリデーション済み
│   ├── 2_deduplicated.json       # 類似排除済み
│   ├── 3_numbering_data.json     # ID採番済み
│   ├── 4_leveling_data.json      # レベル正規化
│   ├── 5_categories_meta.json    # カテゴリメタ
│   └── err/                      # エラーファイル
├── n2/
├── n3/
├── n4/
└── n5/
```

## 注意事項

- Script 0（問題生成）はカテゴリベースで全レベル・全カテゴリに対しリクエストを送信するため、API利用料に注意
- チェックポイント機能により、中断後に再開可能（生成済みファイルはスキップ）
- モデルフォールバック: プライマリモデル失敗時にセカンダリモデルに自動切替
- エラーが発生したレコードは `err/` ディレクトリに保存され、後から確認可能
