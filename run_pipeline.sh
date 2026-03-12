#!/bin/bash
set -euo pipefail

# JLPT問題生成パイプライン一括実行スクリプト
#
# 使用方法:
#   ./run_pipeline.sh              # 全工程実行（生成100件/レベル）
#   ./run_pipeline.sh --skip-generate  # 生成スキップ（パース以降のみ）
#   GENERATE_COUNT=500 ./run_pipeline.sh  # 500件/レベル生成
#
# 前提条件:
#   - .envファイルが設定済み
#   - cargo build --release 済み

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# .env読込
if [ -f .env ]; then
    set -a
    source .env
    set +a
else
    echo "ERROR: .envファイルが見つかりません"
    exit 1
fi

SKIP_GENERATE=false
for arg in "$@"; do
    case $arg in
        --skip-generate) SKIP_GENERATE=true ;;
    esac
done

# リリースビルド確認
if [ ! -f target/release/create_questions ]; then
    echo "リリースビルドが必要です。ビルド中..."
    cargo build --release 2>&1 | tail -3
fi

echo "=========================================="
echo " JLPT問題生成パイプライン"
echo "=========================================="
echo " GENERATE_COUNT: ${GENERATE_COUNT:-100}"
echo " REQUEST_INTERVAL: ${REQUEST_INTERVAL:-20}s"
echo " SKIP_GENERATE: $SKIP_GENERATE"
echo "=========================================="
echo ""

PIPELINE_START=$(date +%s)

# ===== Stage 0: AI問題生成 =====
if [ "$SKIP_GENERATE" = false ]; then
    echo "[Stage 0] AI問題生成..."
    STAGE_START=$(date +%s)
    ./target/release/create_questions
    STAGE_END=$(date +%s)
    echo "[Stage 0] 完了 ($((STAGE_END - STAGE_START))秒)"
    echo ""
fi

# ===== Stage 1: JSONパース =====
echo "[Stage 1] JSONパース → 1_parsed.json"
STAGE_START=$(date +%s)
./target/release/json_read_to_struct
STAGE_END=$(date +%s)
echo "[Stage 1] 完了 ($((STAGE_END - STAGE_START))秒)"
echo ""

# ===== Stage 1.5: バリデーション =====
echo "[Stage 1.5] バリデーション → 1_5_validated.json"
STAGE_START=$(date +%s)
./target/release/validate_questions
STAGE_END=$(date +%s)
echo "[Stage 1.5] 完了 ($((STAGE_END - STAGE_START))秒)"
echo ""

# ===== Stage 2: 重複排除 =====
echo "[Stage 2] 重複排除（類似度85%） → 2_deduplicated.json"
STAGE_START=$(date +%s)
./target/release/duplicate
STAGE_END=$(date +%s)
echo "[Stage 2] 完了 ($((STAGE_END - STAGE_START))秒)"
echo ""

# ===== Stage 3: ID採番 =====
echo "[Stage 3] ID採番（UUID） → 3_numbered.json"
STAGE_START=$(date +%s)
./target/release/numbering
STAGE_END=$(date +%s)
echo "[Stage 3] 完了 ($((STAGE_END - STAGE_START))秒)"
echo ""

# ===== Stage 4: レベリング =====
echo "[Stage 4] レベルID正規化 → 4_leveled.json"
STAGE_START=$(date +%s)
./target/release/leveling
STAGE_END=$(date +%s)
echo "[Stage 4] 完了 ($((STAGE_END - STAGE_START))秒)"
echo ""

# ===== Stage 5: カテゴリ抽出 =====
echo "[Stage 5] カテゴリメタデータ → 5_categories_meta.json"
STAGE_START=$(date +%s)
./target/release/to_meta
STAGE_END=$(date +%s)
echo "[Stage 5] 完了 ($((STAGE_END - STAGE_START))秒)"
echo ""

PIPELINE_END=$(date +%s)
TOTAL_TIME=$((PIPELINE_END - PIPELINE_START))

echo "=========================================="
echo " パイプライン完了"
echo " 総時間: ${TOTAL_TIME}秒 ($((TOTAL_TIME / 60))分)"
echo "=========================================="
echo ""

# 結果サマリー
echo "=== レベル別最終データ件数 ==="
for level in n1 n2 n3 n4 n5; do
    LEVELED="output/questions/$level/4_leveled.json"
    if [ -f "$LEVELED" ]; then
        COUNT=$(python3 -c "import json; print(len(json.load(open('$LEVELED'))))" 2>/dev/null || echo "?")
        echo "  $level: ${COUNT}件"
    else
        echo "  $level: ファイルなし"
    fi
done

echo ""
echo "次のステップ:"
echo "  DB投入: ./target/release/questions_to_database"
echo "  カテゴリ: ./target/release/categories_to_database"
echo "  一括入替: DRY_RUN=false ./target/release/clear_and_replace"
