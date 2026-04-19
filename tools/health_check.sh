#!/bin/bash
# health_check.sh — JLPT 運用監視スクリプト
#
# 24-48h 毎の手動実行 or cron で実行する想定。
# Gemini pro レビュー推奨の3項目を一括確認:
#   1. 新規 reports 流入 (ユーザ報告)
#   2. 重複データの再発率 (/api/admin/duplicates)
#   3. Cloud Run エラーログ (過去24h)
#
# 事前条件:
#   - gcloud config configurations activate jlpt
#   - gcloud config set account sharebook.amazon@gmail.com
#   - ADMIN_JWT 環境変数が設定されていること (admin認証API用)
#
# Usage:
#   export ADMIN_JWT="<admin-jwt-token>"
#   bash tools/health_check.sh
#
# Output: stdout に各セクションの結果、終了コードは最大深刻度に準じる

set -uo pipefail

PROJECT_ID="argon-depth-446413-t0"
BACKEND_URL="https://backend-652691189545.asia-northeast1.run.app"
TIME_WINDOW_HOURS="${TIME_WINDOW_HOURS:-24}"

echo "=========================================="
echo "JLPT Health Check — $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "Time window: past ${TIME_WINDOW_HOURS}h"
echo "=========================================="
echo ""

exit_code=0

# -------------------------------------------------------
# 1. 新規 reports 流入 (ユーザ報告)
# -------------------------------------------------------
echo "## 1. ユーザ報告 (reports) 流入"
if [ -z "${ADMIN_JWT:-}" ]; then
    echo "  SKIP: ADMIN_JWT 未設定 (sign in でJWTを取得して export してください)"
    echo ""
else
    reports_body=$(curl -s -H "Cookie: access_token=${ADMIN_JWT}" "${BACKEND_URL}/api/admin/reports")
    if echo "$reports_body" | grep -q '"status":"success"'; then
        report_count=$(echo "$reports_body" | python3 -c "import sys, json; d = json.load(sys.stdin); print(len(d.get('data', [])))" 2>/dev/null || echo "?")
        echo "  unique question_id 数 (reports テーブル): ${report_count}"
        if [ "$report_count" != "0" ] && [ "$report_count" != "?" ]; then
            echo "  top 5 reported questions:"
            echo "$reports_body" | python3 -c "
import sys, json
d = json.load(sys.stdin)
items = d.get('data', [])[:5]
for i, item in enumerate(items, 1):
    print(f'    {i}. question_id={item[\"question_id\"][:8]}... count={item[\"report_count\"]}')" 2>/dev/null
            exit_code=1
        fi
    else
        echo "  ERROR: API応答が不正"
        echo "$reports_body" | head -c 200
        echo ""
        exit_code=2
    fi
fi
echo ""

# -------------------------------------------------------
# 2. 重複再発率 (/api/admin/duplicates)
# -------------------------------------------------------
echo "## 2. 重複データ健全性"
if [ -z "${ADMIN_JWT:-}" ]; then
    echo "  SKIP: ADMIN_JWT 未設定"
    echo ""
else
    dup_body=$(curl -s -H "Cookie: access_token=${ADMIN_JWT}" "${BACKEND_URL}/api/admin/duplicates")
    if echo "$dup_body" | grep -q '"status":"success"'; then
        echo "$dup_body" | python3 -c "
import sys, json
d = json.load(sys.stdin).get('data', {})
print(f\"  total_parents:                 {d.get('total_parents', '?')}\")
print(f\"  total_sub_questions:           {d.get('total_sub_questions', '?')}\")
print(f\"  dedup_groups:                  {d.get('dedup_groups', '?')}\")
print(f\"  removable_subs:                {d.get('removable_subs', '?')}\")
print(f\"  skipped_numeric_placeholder:   {d.get('skipped_numeric_placeholder', '?')}\")
print(f\"  skipped_answer_not_in_options: {d.get('skipped_answer_not_in_options', '?')}\")
if d.get('removable_subs', 0) > 0:
    print('')
    print('  top groups:')
    for i, g in enumerate(d.get('top_groups', [])[:5], 1):
        print(f'    {i}. count={g.get(\"count\")} key={g.get(\"dedup_key\", \"\")[:60]}')
    sys.exit(1)
" 2>/dev/null
        dedup_exit=$?
        if [ $dedup_exit -eq 1 ]; then
            echo "  WARNING: removable_subs > 0 — 新規生成で重複発生の疑い"
            exit_code=1
        fi
    else
        echo "  ERROR: API応答が不正"
        echo "$dup_body" | head -c 200
        echo ""
        exit_code=2
    fi
fi
echo ""

# -------------------------------------------------------
# 3. Cloud Run エラーログ (過去 N 時間)
# -------------------------------------------------------
echo "## 3. Cloud Run エラーログ (過去${TIME_WINDOW_HOURS}h)"

active_proj=$(gcloud config get-value project 2>/dev/null || echo "")
if [ "$active_proj" != "$PROJECT_ID" ]; then
    echo "  SKIP: gcloud config の project が $PROJECT_ID と不一致 (現在: $active_proj)"
    echo "       先に \`gcloud config configurations activate jlpt\` を実行してください"
    echo ""
else
    # backend errors
    backend_errors=$(gcloud logging read \
        "resource.type=cloud_run_revision resource.labels.service_name=backend severity>=ERROR timestamp>=\"$(date -u -d "${TIME_WINDOW_HOURS} hours ago" +%Y-%m-%dT%H:%M:%SZ)\"" \
        --project=$PROJECT_ID \
        --limit=10 \
        --format="value(timestamp,severity,textPayload)" 2>&1 | head -30)

    # frontend errors
    frontend_errors=$(gcloud logging read \
        "resource.type=cloud_run_revision resource.labels.service_name=frontend severity>=ERROR timestamp>=\"$(date -u -d "${TIME_WINDOW_HOURS} hours ago" +%Y-%m-%dT%H:%M:%SZ)\"" \
        --project=$PROJECT_ID \
        --limit=10 \
        --format="value(timestamp,severity,textPayload)" 2>&1 | head -30)

    backend_count=$(echo "$backend_errors" | grep -c "^" 2>/dev/null || echo "0")
    frontend_count=$(echo "$frontend_errors" | grep -c "^" 2>/dev/null || echo "0")

    if [ -z "$backend_errors" ]; then backend_count=0; fi
    if [ -z "$frontend_errors" ]; then frontend_count=0; fi

    echo "  backend エラー件数:  $backend_count"
    echo "  frontend エラー件数: $frontend_count"

    if [ "$backend_count" != "0" ]; then
        echo ""
        echo "  backend エラー先頭5件:"
        echo "$backend_errors" | head -5 | sed 's/^/    /'
        exit_code=1
    fi
    if [ "$frontend_count" != "0" ]; then
        echo ""
        echo "  frontend エラー先頭5件:"
        echo "$frontend_errors" | head -5 | sed 's/^/    /'
        exit_code=1
    fi
fi
echo ""

# -------------------------------------------------------
# Summary
# -------------------------------------------------------
echo "=========================================="
case $exit_code in
    0) echo "SUMMARY: HEALTHY" ;;
    1) echo "SUMMARY: WARNING (確認事項あり)" ;;
    2) echo "SUMMARY: ERROR (API応答失敗等)" ;;
    *) echo "SUMMARY: UNKNOWN ($exit_code)" ;;
esac
echo "=========================================="

exit $exit_code
