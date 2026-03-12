# jlpt-app-scripts 概要

## プロジェクト情報

| 項目 | 内容 |
|------|------|
| 言語 | Rust (Edition 2024) |
| AI連携 | Google Gemini API |
| データベース | Google Firestore |
| 対象レベル | N1〜N5（全5レベル） |

## 概要

JLPT問題の大規模自動生成・加工・データベース投入を行うデータパイプライン。Google Gemini APIでAI生成された問題を、段階的に検証・加工し、Firestoreに格納する。

## 責務

1. AI（Gemini）による大量問題生成（レベルあたり1000リクエスト）
2. JSON→構造体パース・バリデーション
3. 重複排除
4. ID採番（UUID）
5. レベルID正規化
6. カテゴリメタデータ抽出
7. Firestoreへのバッチ投入

## japanese-app との違い

| 項目 | japanese-app | jlpt-app-scripts |
|------|-------------|------------------|
| 対象レベル | N2, N3 | N1〜N5（全レベル） |
| リクエスト数 | 30回/レベル | 1000回/レベル |
| パイプライン | 生成→結合→パース | 生成→パース→重複排除→採番→レベリング→カテゴリ化→DB投入 |
| DB連携 | なし | Firestore投入あり |
| プロンプト | 2レベル分 | 5レベル分（完全版） |

## 関連リポジトリ

| リポジトリ | 役割 |
|-----------|------|
| [japanese-app](../../japanese-app/docs/) | 問題生成スクリプト（初期版・小規模） |
| [jlpt-app-backend](../../jlpt-app-backend/docs/) | 問題配信API |
