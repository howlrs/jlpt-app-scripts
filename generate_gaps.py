#!/usr/bin/env python3
"""
大規模ギャップ補充スクリプト

create_targeted (Rust) のPython版。
カテゴリ別の不足分を指定し、Gemini APIでターゲット生成 → rawファイル出力。
出力後は run_pipeline.sh --skip-generate でパイプライン処理。
"""
import json, os, time, sys, random, re
import urllib.request, urllib.error

# .env 読み込み
def load_env():
    if os.path.exists(".env"):
        with open(".env") as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#") and "=" in line:
                    k, v = line.split("=", 1)
                    os.environ.setdefault(k.strip(), v.strip())

load_env()

API_KEY = os.environ["GOOGLE_GEMINI_API_KEY"]
MODELS = os.environ.get("GEMINI_MODELS", "gemini-3.1-flash-lite-preview,gemini-3-flash-preview").split(",")
PRIMARY_MODEL = MODELS[0].strip()
FALLBACK_MODEL = MODELS[1].strip() if len(MODELS) > 1 else PRIMARY_MODEL
REQUEST_INTERVAL = int(os.environ.get("REQUEST_INTERVAL", "10"))
BUFFER_RATIO = float(os.environ.get("BUFFER_RATIO", "2.0"))

SYSTEM_INSTRUCTION = """あなたはJLPT（日本語能力試験）の公式問題作成の専門家です。以下を厳守してください：
1. 出力は必ず有効なJSONのみ（マークダウン記法禁止、説明文禁止）
2. 選択肢は必ず4つ。正解は必ず1つだけ
3. 正解の位置（1〜4）を偏らせない
4. 誤答は「一見正しそうだが明確な理由で不正解」であること
5. 選択肢の長さ・構造・語彙レベルを揃えること
6. 指定されたレベルの語彙・文法範囲を厳守すること
7. 指定されたカテゴリの問題のみを生成すること"""

# ギャップ定義: (level, cat_id, category_name, current, target, extra_hint)
GAPS = [
    # === 赤色（緊急） ===
    ("n1", 21, "統合理解", 5, 100,
     "複数のテキスト（2〜3の短い文章や図表）を読み比べて、共通点・相違点・関係性を理解する問題。新聞記事と意見文の比較、複数の案内文の比較等。"),
    ("n1", 1, "読解における違い", 19, 200,
     "異なる立場・視点から書かれた2つの文章を読み、筆者間の意見の違いを正確に読み取る問題。対比的な論説文、書評と反論等。category_nameは「読解における違い」を使用。"),
    ("n2", 10, "文章の文法", 138, 300, ""),
    ("n2", 16, "聴解", 13, 150,
     "音声を聞いて内容を理解する聴解問題。会話や説明を聞いて、要点・詳細・話し手の意図を把握する。シチュエーション: 職場の会話、店での問い合わせ、ニュース等。"),
    ("n2", 17, "聴解 - 課題理解", 10, 150,
     "聴解の課題理解形式。まず質問を読み、その後音声を聞いて答える。具体的な行動や選択を求められる場面。category_nameは「課題理解」を使用。"),
    ("n4", 10, "文章の文法", 134, 300, ""),
    ("n4", 14, "聴解", 5, 150,
     "音声を聞いて内容を理解する問題。日常的な会話、アナウンス、簡単な説明。N4レベルの語彙・文法を使用。"),
    ("n5", 8, "文の文法1 (文法形式の判断)", 122, 300, ""),
    ("n5", 10, "文章の文法", 105, 300, ""),
    ("n5", 4, "言語知識（文字・語彙）-文脈規定", 2, 100,
     "文の中の（　）に入る最も適切な語を選ぶ問題。N5レベルの基本語彙（名詞・動詞・形容詞）を文脈から判断。category_nameは「言語知識（文字・語彙）-文脈規定」を使用。"),
    ("n5", 7, "言語知識（文法）・読解", 110, 300,
     "文法知識と読解力を総合的に問う問題。短い文章を読んで文法的に正しい表現を選ぶ、または内容理解を問う。"),
    ("n5", 7, "言語知識（文法）・読解の違い", 5, 300,
     "文法・読解の複合問題で、似た表現の違いを理解する問題。category_nameは「言語知識（文法）・読解の違い」を使用。"),
    # === 黄色 ===
    ("n1", 11, "内容理解 (短文)", 163, 200, ""),
    ("n1", 9, "文の文法2 (文の組み立て)", 200, 300, ""),
    ("n1", 10, "文章の文法", 152, 300, ""),
    ("n1", 1, "言語知識（文字・語彙・文法）・読解", 172, 300,
     "文字・語彙・文法の総合的な知識と読解力を問う問題。文章中の語句の意味、文法形式の判断、内容理解を複合的に出題。category_nameは「言語知識（文字・語彙・文法）・読解」を使用。"),
    ("n2", 11, "内容理解 (短文)", 128, 200, ""),
    ("n2", 9, "文の文法2 (文の組み立て)", 235, 300, ""),
    ("n2", 17, "課題理解", 109, 150,
     "聴解の課題理解。音声を聞いて、具体的な課題の解決方法や次にすべき行動を答える。"),
    ("n3", 11, "内容理解 (短文)", 174, 200, ""),
    ("n3", 13, "内容理解 (長文)", 167, 200, ""),
    ("n3", 9, "文の文法2 (文の組み立て)", 213, 300, ""),
    ("n3", 10, "文章の文法", 155, 300, ""),
    ("n3", 18, "概要理解", 102, 150, ""),
    ("n3", 16, "課題理解", 147, 150, ""),
    ("n4", 9, "文の文法2 (文の組み立て)", 196, 300, ""),
    ("n4", 17, "概要理解", 133, 150, ""),
    ("n4", 15, "課題理解", 107, 150, ""),
    ("n5", 16, "ポイント理解", 123, 150, ""),
    ("n5", 12, "内容理解 (中文)", 136, 200, ""),
    ("n5", 11, "内容理解 (短文)", 160, 200, ""),
    ("n5", 9, "文の文法2 (文の組み立て)", 155, 300, ""),
    ("n5", 17, "概要理解", 130, 150, ""),
    ("n5", 14, "聴解", 136, 150, ""),
    ("n5", 1, "言語知識（文字・語彙）", 60, 100, ""),
    ("n5", 15, "課題理解", 127, 150, ""),
]

def build_prompt(level, cat_id, cat_name, extra_hint, seed):
    level_upper = level.upper()
    hints = f"\n{extra_hint}\n" if extra_hint else ""
    return f"""あなたはJLPT {level_upper}レベルの「{cat_name}」カテゴリの問題を作成する専門家です。

以下の条件で{cat_name}の問題を5問以上生成してください：

**レベル:** {level_upper}
**カテゴリ:** {cat_name}（カテゴリID: {cat_id}）
{hints}
**出力フォーマット:**
JSON配列で出力。各要素は以下の構造:
```json
[
  {{
    "level_name": "{level}",
    "category_id": "{cat_id}",
    "category_name": "{cat_name}",
    "sentence": "大問の問題文（指示文）",
    "prerequisites": "前提となる文章（必要な場合）",
    "sub_questions": [
      {{
        "sentence": "小問の文",
        "prerequisites": "",
        "select_answer": [
          {{"key": "1", "value": "選択肢1"}},
          {{"key": "2", "value": "選択肢2"}},
          {{"key": "3", "value": "選択肢3"}},
          {{"key": "4", "value": "選択肢4"}}
        ],
        "answer": "正解番号(1-4)"
      }}
    ]
  }}
]
```

**品質基準:**
- 選択肢は必ず4つ、正解は必ず1つ
- 正解の位置を1〜4で均等に分散
- 誤答は「一見正しそうだが明確な理由で不正解」
- 選択肢の長さ・構造を揃える
- {level_upper}レベルの語彙・文法範囲を厳守

**多様性指示（シード: {seed}）:**
- 前回と異なるテーマ・場面・語彙を使用
- 同じ文型・表現パターンの繰り返しを避ける

JSONのみを出力してください。マークダウン記法や説明文は不要です。"""


def call_gemini(model, prompt):
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={API_KEY}"
    body = json.dumps({
        "contents": [{"role": "user", "parts": [{"text": prompt}]}],
        "systemInstruction": {"parts": [{"text": SYSTEM_INSTRUCTION}]},
        "generationConfig": {"temperature": 0.8, "maxOutputTokens": 8192}
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=75) as resp:
            data = json.loads(resp.read())
            return data["candidates"][0]["content"]["parts"][0]["text"]
    except Exception as e:
        return None


def request_with_fallback(prompt, max_retries=3):
    for attempt in range(max_retries):
        result = call_gemini(PRIMARY_MODEL, prompt)
        if result:
            return result, PRIMARY_MODEL
        wait = 60 * (attempt + 1)
        print(f"  [retry {attempt+1}/{max_retries}] {wait}s wait...", flush=True)
        time.sleep(wait)
    # fallback
    result = call_gemini(FALLBACK_MODEL, prompt)
    if result:
        return result, FALLBACK_MODEL
    return None, None


def clean_json(text):
    text = text.strip()
    if text.startswith("```json"):
        text = text[7:]
    elif text.startswith("```"):
        text = text[3:]
    if text.endswith("```"):
        text = text[:-3]
    return text.strip()


def main():
    total_success = 0
    total_fail = 0
    total_deficit = sum(max(0, t - c) for _, _, _, c, t, _ in GAPS)

    print(f"=== ギャップ補充生成 ===")
    print(f"ギャップ: {len(GAPS)}カテゴリ, 合計不足: {total_deficit}問")
    print(f"BUFFER_RATIO: {BUFFER_RATIO}, REQUEST_INTERVAL: {REQUEST_INTERVAL}s")
    print(f"primary: {PRIMARY_MODEL}, fallback: {FALLBACK_MODEL}")
    print()

    for level, cat_id, cat_name, current, target, extra_hint in GAPS:
        deficit = target - current
        if deficit <= 0:
            continue

        raw_target = int(deficit * BUFFER_RATIO)
        requests_needed = max(1, raw_target // 5)

        output_dir = f"output/questions/{level}"
        os.makedirs(output_dir, exist_ok=True)

        print(f"[{level.upper()}/cat_{cat_id}] {cat_name} — 不足{deficit}問 → {requests_needed}リクエスト", flush=True)

        for i in range(requests_needed):
            seed = random.randint(0, 2**31)
            prompt = build_prompt(level, cat_id, cat_name, extra_hint, seed)

            text, used_model = request_with_fallback(prompt)

            if text:
                cleaned = clean_json(text)
                try:
                    json_val = json.loads(cleaned)
                    if isinstance(json_val, list):
                        for item in json_val:
                            if isinstance(item, dict):
                                item["generated_by"] = used_model
                                # category_id を確実に付与
                                if "category_id" not in item or item["category_id"] is None:
                                    item["category_id"] = str(cat_id)

                        ts = int(time.time() * 1000)
                        filepath = os.path.join(output_dir, f"{ts}.json")
                        with open(filepath, "w") as f:
                            json.dump(json_val, f, ensure_ascii=False, indent=2)
                        total_success += 1
                    else:
                        print(f"  [warn] non-array JSON", flush=True)
                        total_fail += 1
                except json.JSONDecodeError:
                    print(f"  [warn] invalid JSON ({len(cleaned)} chars)", flush=True)
                    total_fail += 1
            else:
                total_fail += 1

            if (i + 1) % 10 == 0:
                print(f"  [{level.upper()}/cat_{cat_id}] {i+1}/{requests_needed} done", flush=True)

            time.sleep(REQUEST_INTERVAL)

    print(f"\n=== 完了 ===")
    print(f"成功: {total_success}, 失敗: {total_fail}")
    print(f"次のステップ: ./run_pipeline.sh --skip-generate")


if __name__ == "__main__":
    main()
