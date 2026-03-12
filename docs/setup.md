# jlpt-app-scripts セットアップガイド

## 前提条件

- Rust (Edition 2024 対応版)
- Google Gemini API キー
- Google Cloud プロジェクト（Firestore有効化済み）

## 環境変数

`.env` ファイルを作成：

| 変数名 | 必須 | 説明 |
|--------|------|------|
| `GOOGLE_GEMINI_API_KEY` | Yes | Google Gemini API の認証キー |
| `GEMINI_MODELS` | Yes | 使用するGeminiモデル名（カンマ区切り、2つ指定） |
| `PROJECT_ID` | Yes | Google Cloud プロジェクトID（Firestore用） |
| `RUST_LOG` | No | ログレベル（デフォルト: INFO） |

## ビルド

```bash
cargo build --release
```

## パイプライン実行

順番に実行する：

```bash
# 1. AI問題生成（非常に長時間かかる: 1000リクエスト × 5レベル）
cargo run --bin create_questions

# 2. ファイル結合
cargo run --bin template

# 3. JSON構造化・バリデーション
cargo run --bin json_read_to_struct

# 4. 重複排除
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

## 出力ディレクトリ

パイプライン実行後、以下の構造で出力される：

```
output/questions/
├── n1/
│   ├── *.json                              # API生レスポンス
│   ├── concat_with_struct.json             # 構造化済み
│   ├── removed_duplicate_rows_concat_all.json  # 重複除去
│   ├── 3_numbering_data.json               # ID採番済み
│   ├── 4_leveling_data.json                # レベル正規化
│   ├── 5_categories_meta.json              # カテゴリメタ
│   └── err/                                # エラーファイル
├── n2/
├── n3/
├── n4/
└── n5/
```

## 注意事項

- Script 0（問題生成）は全レベル合計5000リクエストを送信するため、API利用料に注意
- 20秒間隔でリクエストを送信、失敗時は120秒待機後リトライ
- エラーが発生したレコードは `err/` ディレクトリに保存され、後から確認可能
