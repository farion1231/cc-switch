# Codex ローカルモデルルーティングガイド

> CC Switch 3.16.0+ 向け。

このドキュメントは、以前の DeepSeek 専用ガイドを汎用化したものです。Codex は 1 つの CC Switch Rust ローカルプロキシに接続し、CC Switch が `body.model` を見て上流 route を選択します。

## 目的

Codex クライアントは OpenAI Responses API を送信します。一方で、多くの上流プロバイダーは Chat Completions または Messages 形式を提供します。上流 URL を直接 `~/.codex/config.toml` に書くと、`/responses` の 404/400、モデル一覧の不一致、ストリーミング解析エラーが起きることがあります。

ローカルモデルルーティングでは、Codex は `http://127.0.0.1:15721/v1/responses` のような CC Switch ローカルプロキシだけを参照し、実際の上流選択は CC Switch 内で行います。

## 実行フロー

1. Codex が CC Switch ローカルプロキシにリクエストします。
2. CC Switch がリクエスト本文の `model` を読みます。
3. `settings_config.codexRouting.routes[]` の exact model または prefix で route を解決します。
4. route の base URL、API format、auth、model mapping、capability から effective provider を作ります。
5. 既存 forwarder でプロトコルを変換します。
   - `openai_responses`: Responses をそのまま転送。
   - `openai_chat`: Responses を Chat Completions に変換し、応答を Responses に戻す。
   - `openai_messages`: route が対応している場合に Messages 形式へ変換。

## Route 設定

Codex provider フォームの **Local model routing** で route を追加します。

- Match: `match.models`、`match.prefixes`。
- Upstream: `upstream.baseUrl`、`upstream.apiFormat`。
- Auth source:
  - `provider_config`: route または現在の provider の API key を使用。
  - `managed_codex_oauth`: CC Switch 管理の Codex OAuth を使用。
  - `managed_account`: 管理アカウントの auth binding。現時点では Codex OAuth として扱います。
- Model mapping: `upstream.modelMap`。例: `codex-model=upstream-model`。
- Capability: text-only、image、reasoning。

初期版では `reuse_provider:<id>` は未対応です。

## Schema

```json
{
  "settings_config": {
    "codexRouting": {
      "enabled": true,
      "defaultRouteId": "openai",
      "routes": [
        {
          "id": "deepseek",
          "label": "DeepSeek",
          "enabled": true,
          "match": {
            "models": ["deepseek-v4-flash"],
            "prefixes": ["deepseek-"]
          },
          "upstream": {
            "baseUrl": "https://api.deepseek.com",
            "apiFormat": "openai_chat",
            "auth": { "source": "provider_config" },
            "modelMap": { "deepseek-v4-flash": "deepseek-v4-flash" }
          },
          "capabilities": {
            "textOnly": true,
            "inputModalities": ["text"],
            "supportsReasoning": true
          }
        }
      ]
    }
  }
}
```

`settings_config.codexRouting` が新しい主スキーマです。`settings_config.codexModelRoutes` と `settings_config.modelRoutes` は既存設定を読むためのフォールバックで、UI で保存すると新スキーマへ書き戻されます。

## 注意

- text-only route は catalog に `input_modalities=["text"]` を生成します。
- Responses -> Chat 変換は route capability を参照し、text-only 上流に `image_url` を送らないようにします。
- チャット画面でモデルを切り替えるだけで route が選ばれるため、GUI で先に provider を切り替える必要はありません。
