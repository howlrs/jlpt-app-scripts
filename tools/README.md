# tools/

運用監視・診断用のシェルスクリプト群。

## health_check.sh

JLPT 運用監視スクリプト。24-48h 毎の手動実行 or cron で実行する想定。

### 監視項目

1. **ユーザ報告 (reports) 流入** — `GET /api/admin/reports` 経由
   - ユーザが「⚑ 問題を報告」ボタンから上げた報告の集計
   - report_count 降順で top 5 を表示、0件以外なら WARNING

2. **重複データ健全性** — `GET /api/admin/duplicates` 経由
   - 既存掃除後の重複再発状況
   - `removable_subs > 0` なら WARNING (新規生成バッチでの重複発生疑い)

3. **Cloud Run エラーログ** — `gcloud logging read` 経由
   - 過去 N 時間 (デフォルト 24h) の backend/frontend の ERROR 以上を確認

### 使い方

```bash
# 1. gcloud 認証状態を jlpt 構成に
gcloud config configurations activate jlpt
gcloud config set account sharebook.amazon@gmail.com

# 2. Admin JWT を取得 (例: ブラウザで /admin/login してから Cookie の access_token 値をコピー)
export ADMIN_JWT="eyJ0eXAi..."

# 3. 実行
bash tools/health_check.sh

# カスタム time window (例: 48h)
TIME_WINDOW_HOURS=48 bash tools/health_check.sh
```

### 終了コード

| code | 意味 |
|------|------|
| 0 | HEALTHY (問題なし) |
| 1 | WARNING (確認事項あり — reports > 0 or removable_subs > 0 or Cloud Run errors) |
| 2 | ERROR (API応答失敗等) |

### cron 推奨設定 (例: 1日1回)

```cron
# 毎日 AM 3:00 (JST) に実行
0 3 * * * cd /home/o9oem/workspace/mine/jlpt_base/jlpt-app-scripts && ADMIN_JWT="..." bash tools/health_check.sh >> /var/log/jlpt-health.log 2>&1
```

(ADMIN_JWT は expires するため定期更新が必要。将来的には service-account ベースに切り替え推奨)

### 関連 Issue

- backend#20: 管理画面 重複監視 UI (本スクリプトは CLI 代替、UI 実装後は UI 優先)
- 本セッション (2026-04-19) の Phase 6 で作成
