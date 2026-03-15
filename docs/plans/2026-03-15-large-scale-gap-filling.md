# 大規模ギャップ補充 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** ユーザー指定の36カテゴリ・合計3,452問の不足を補充し、全カテゴリで目標値を達成する

**Architecture:** Python版ギャップ補充スクリプト(`generate_gaps.py`)で全ギャップのrawファイルを一括生成し、既存パイプライン(`run_pipeline.sh --skip-generate`)で処理後、`clear_and_replace`でDB投入。目標値は従来のTARGET_MIN=100から、カテゴリ別に100〜300の可変目標に拡張。

**Tech Stack:** Python 3 (Gemini API直接呼び出し), Rust (パイプライン), Firestore

---

### Task 1: generate_gaps.py でraw生成

**Files:**
- Create: `generate_gaps.py` (完了)

**Step 1: 生成実行**

```bash
REQUEST_INTERVAL=5 BUFFER_RATIO=2.0 python3 generate_gaps.py
```

推定: 1,370リクエスト, 約2時間

**Step 2: 生成結果確認**

各レベルの新規rawファイル数を確認。

### Task 2: パイプライン処理

```bash
./run_pipeline.sh --skip-generate
```

### Task 3: カバレッジ確認

全36ギャップカテゴリの達成状況を確認。未達があればBUFFER_RATIO上げて再実行。

### Task 4: DB投入

```bash
DRY_RUN=false ./target/release/clear_and_replace
```

### Task 5: コミット & プッシュ

```bash
git add generate_gaps.py docs/plans/2026-03-15-large-scale-gap-filling.md
git commit -m "feat: 大規模ギャップ補充（36カテゴリ・3,452問）"
git push origin master
```

## ギャップ対象一覧

### 赤色カテゴリ（緊急: 12件）

| Level | cat_id | カテゴリ | 不足 |
|---|---|---|---|
| N1 | 21 | 統合理解 | 95 |
| N1 | 1 | 読解における違い | 181 |
| N2 | 10 | 文章の文法 | 162 |
| N2 | 16 | 聴解 | 137 |
| N2 | 17 | 聴解 - 課題理解 | 140 |
| N4 | 10 | 文章の文法 | 166 |
| N4 | 14 | 聴解 | 145 |
| N5 | 8 | 文の文法1 | 178 |
| N5 | 10 | 文章の文法 | 195 |
| N5 | 4 | 言語知識(文字・語彙)-文脈規定 | 98 |
| N5 | 7 | 言語知識(文法)・読解 | 190 |
| N5 | 7 | 言語知識(文法)・読解の違い | 295 |

### 黄色カテゴリ（追加必要: 24件）

| Level | cat_id | カテゴリ | 不足 |
|---|---|---|---|
| N1 | 11 | 内容理解 (短文) | 37 |
| N1 | 9 | 文の文法2 | 100 |
| N1 | 10 | 文章の文法 | 148 |
| N1 | 1 | 言語知識・読解 | 128 |
| N2 | 11 | 内容理解 (短文) | 72 |
| N2 | 9 | 文の文法2 | 65 |
| N2 | 17 | 課題理解 | 41 |
| N3 | 11 | 内容理解 (短文) | 26 |
| N3 | 13 | 内容理解 (長文) | 33 |
| N3 | 9 | 文の文法2 | 87 |
| N3 | 10 | 文章の文法 | 145 |
| N3 | 18 | 概要理解 | 48 |
| N3 | 16 | 課題理解 | 3 |
| N4 | 9 | 文の文法2 | 104 |
| N4 | 17 | 概要理解 | 17 |
| N4 | 15 | 課題理解 | 43 |
| N5 | 16 | ポイント理解 | 27 |
| N5 | 12 | 内容理解 (中文) | 64 |
| N5 | 11 | 内容理解 (短文) | 40 |
| N5 | 9 | 文の文法2 | 145 |
| N5 | 17 | 概要理解 | 20 |
| N5 | 14 | 聴解 | 14 |
| N5 | 1 | 言語知識(文字・語彙) | 40 |
| N5 | 15 | 課題理解 | 23 |
